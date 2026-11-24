//! Imports the library file that iTunes and the Music app export: the songs' files are copied
//! into the music folder, songs whose file is not found stay as songs without a file, and their
//! history, the playlists and the radio stations come along.

use musiccraft_library::itunes::{self, Export, List};
use musiccraft_library::model::Info;
use musiccraft_library::scan::Progress;
use musiccraft_library::{History, Item, Library, import, tags};
use serde_json::{Value, json};
use std::collections::HashMap;
use std::path::Path;
use std::sync::atomic::Ordering;
use unicode_normalization::UnicodeNormalization;

/// What the background work found and copied, for the library to take in.
#[derive(Debug, Default)]
pub struct Prepared {
    /// Songs whose file is now in the music folder.
    files: Vec<(i64, Info)>,
    /// Songs whose file was not found, or that were only streamed.
    streams: Vec<(i64, Info)>,
    history: HashMap<i64, History>,
    playlists: Vec<List>,
    stations: Vec<(String, String)>,
    /// Files found but not copied, with the reason.
    pub failed: Vec<(String, String)>,
    not_music: usize,
}

/// Reads the file and copies the songs' files. Runs on a thread of its own; `progress.found` is
/// the number of songs, `progress.read` how many are done.
pub fn prepare(path: &Path, music: &Path, progress: &Progress) -> Result<Prepared, String> {
    let size = std::fs::metadata(path).map_err(|e| e.to_string())?.len();
    if size > itunes::MAX_FILE {
        return Err("the library file is too large".to_owned());
    }
    let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
    let Export { music_folder, songs, playlists, stations, not_music } = itunes::read(&bytes)?;
    drop(bytes);
    progress.found.store(songs.len(), Ordering::Relaxed);
    let beside = path.parent().unwrap_or(Path::new("."));
    let mut out = Prepared { playlists, stations, not_music, ..Prepared::default() };
    for song in songs {
        if progress.stop.load(Ordering::Relaxed) {
            return Err("the import was stopped".to_owned());
        }
        out.history.insert(song.key, song.history.clone());
        let file = itunes::candidates(&song.location, &music_folder, beside)
            .into_iter()
            .find(|p| p.is_file() && p.file_name().and_then(|n| n.to_str()).is_some_and(musiccraft_library::is_audio));
        match file {
            Some(file) => match import::copy_one(music, &file).and_then(|(target, _)| relative(music, &target)) {
                Ok((target, rel)) => {
                    // The file's own tags, or what the other program knew when they cannot be read.
                    let info = tags::read(&target, &rel).unwrap_or(Info { path: rel, ..song.info });
                    out.files.push((song.key, info));
                }
                Err(e) => {
                    out.failed.push((file.display().to_string(), e));
                    out.streams.push((song.key, song.info));
                }
            },
            None => out.streams.push((song.key, song.info)),
        }
        progress.read.fetch_add(1, Ordering::Relaxed);
    }
    Ok(out)
}

/// The path below the music folder, as the library writes it.
fn relative(music: &Path, target: &Path) -> Result<(std::path::PathBuf, String), String> {
    let rest = target.strip_prefix(music).map_err(|e| e.to_string())?;
    let parts: Option<Vec<String>> = rest.iter().map(|p| p.to_str().map(|s| s.nfc().collect())).collect();
    let rel = parts.ok_or("the file name is not valid text")?.join("/");
    Ok((target.to_owned(), rel))
}

/// Takes what [`prepare`] found into the library. A second import of the same file adds
/// nothing twice: the songs are the same songs, the history only adds, and a playlist of the
/// same name gets the file's songs.
pub fn take(lib: &mut Library, p: Prepared) -> Value {
    let mut ids: HashMap<i64, String> = HashMap::new();
    let (keys, infos): (Vec<i64>, Vec<Info>) = p.files.into_iter().unzip();
    let (file_ids, new_files) = lib.add_files(infos);
    for (k, id) in keys.into_iter().zip(file_ids) {
        if let Some(id) = id {
            ids.insert(k, id);
        }
    }
    let (keys, infos): (Vec<i64>, Vec<Info>) =
        p.streams.into_iter().filter(|(_, i)| !i.title.trim().is_empty()).unzip();
    let (stream_ids, new_streams) = lib.add_songs(infos.into_iter().map(|i| (i, None)).collect());
    ids.extend(keys.into_iter().zip(stream_ids));
    for (k, id) in &ids {
        if let Some(h) = p.history.get(k) {
            lib.add_history(id, h).ok();
        }
    }
    let mut lists = 0;
    let mut smart = 0;
    for list in p.playlists {
        let items: Vec<Item> = list.songs.iter().filter_map(|k| ids.get(k)).map(|id| Item::local(id)).collect();
        if items.is_empty() {
            continue;
        }
        let same = lib.playlists().iter().find(|x| x.name == list.name && x.smart.is_none()).map(|x| x.id.clone());
        let done = match same {
            Some(id) => lib.set_items(&id, items).is_ok(),
            None => {
                lib.create_playlist(&list.name, items, None);
                true
            }
        };
        lists += usize::from(done);
        smart += usize::from(done && list.smart);
    }
    let mut stations = 0;
    for (name, url) in p.stations {
        if !lib.stations().iter().any(|s| s.url == url) {
            lib.add_station(&name, &url, "");
            stations += 1;
        }
    }
    json!({
        "songs": ids.len(),
        "new_with_file": new_files,
        "new_without_file": new_streams,
        "playlists": lists,
        "smart_as_fixed": smart,
        "stations": stations,
        "not_music": p.not_music,
        "failed": p.failed.len(),
    })
}

#[cfg(test)]
mod tests;
