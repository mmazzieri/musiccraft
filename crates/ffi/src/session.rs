//! The requests a phone app makes, and what each one answers.

use crate::done;
use musiccraft_library::model::{Info, Presence, Track};
use musiccraft_library::query::{self, Search, Sort};
use musiccraft_library::scan::ScanResult;
use musiccraft_library::{Item, Library, Playlist, now_ms, tags};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::path::Path;
use unicode_normalization::UnicodeNormalization;

/// At most this many files are taken in by one request.
const MAX_FILES: usize = 4096;
/// At most this many songs are returned by one request.
const MAX_LIST: usize = 100_000;

/// A file of the library folder's `Library/` subfolder, or of the app's own copy of its files.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NamedText {
    pub name: String,
    pub text: String,
}

/// A music file the app found below `Music/`.
#[derive(Debug, Clone, Deserialize)]
pub struct FoundFile {
    /// Path below `Music/`, parts separated by `/`.
    pub path: String,
    #[serde(default)]
    pub size: u64,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum Request {
    /// Starts a library for this phone. `device` is the random id the app made once and keeps.
    Open {
        device: String,
        device_name: String,
    },
    /// Reads the catalogue: every `*.json` file in `Library/` (`files`) and the app's own copy
    /// of this phone's files (`backups`). Answers `{songs, warnings, set_aside}`: the app renames
    /// each file in `set_aside` (this phone's own file, damaged) before it writes that file again.
    Load {
        files: Vec<NamedText>,
        #[serde(default)]
        backups: Vec<NamedText>,
    },
    /// Says which music files are on the phone. Files the library does not know yet are added,
    /// with artist, album and title guessed from the path until a computer reads their tags.
    /// Answers how many songs were added.
    Found {
        files: Vec<FoundFile>,
    },
    /// Songs, in an order, optionally only those matching a search text.
    Songs {
        #[serde(default)]
        search: String,
        #[serde(default)]
        sort: Sort,
        #[serde(default)]
        descending: bool,
    },
    /// Albums, by artist, year and title, each with its songs' ids in order.
    Albums,
    /// One song.
    Song {
        id: String,
    },
    Playlists,
    /// A playlist's songs: its own list, or what its rules choose.
    PlaylistSongs {
        id: String,
    },
    SetLoved {
        id: String,
        loved: bool,
    },
    SetRating {
        id: String,
        stars: u8,
    },
    /// A song played to its end, or almost.
    Played {
        id: String,
    },
    /// A song skipped early.
    Skipped {
        id: String,
    },
    CreatePlaylist {
        name: String,
        #[serde(default)]
        songs: Vec<String>,
    },
    AddToPlaylist {
        id: String,
        songs: Vec<String>,
    },
    RenamePlaylist {
        id: String,
        name: String,
    },
    DeletePlaylist {
        id: String,
    },
    /// This phone's files that changed, to be written in `Library/` and in the app's own copy.
    Unsaved,
    /// The files the app wrote.
    Saved {
        names: Vec<String>,
    },
    /// Whether something is unsaved, and a number that grows with every change.
    State,
}

/// The open library of a phone.
pub struct Session {
    lib: Library,
}

fn no_library() -> String {
    "no library is open".to_owned()
}

fn text(e: impl std::fmt::Display) -> String {
    e.to_string()
}

pub(crate) fn run(slot: &mut Option<Session>, request: Request) -> Result<Value, String> {
    if let Request::Open { device, device_name } = request {
        if device.is_empty() || device.contains(['/', '\\', '.', ':', '\0']) || device.chars().count() > 64 {
            return Err("the device id must be a short name without dots or slashes".to_owned());
        }
        *slot = Some(Session { lib: Library::detached(&device, &device_name) });
        return Ok(done());
    }
    slot.as_mut().ok_or_else(no_library)?.run(request)
}

impl Session {
    fn run(&mut self, request: Request) -> Result<Value, String> {
        let lib = &mut self.lib;
        Ok(match request {
            Request::Open { .. } => done(),
            Request::Load { files, backups } => {
                if files.len() > MAX_FILES || backups.len() > MAX_FILES {
                    return Err("too many catalogue files".to_owned());
                }
                let bytes = |list: Vec<NamedText>| -> Vec<(String, Vec<u8>)> {
                    list.into_iter().map(|f| (f.name, f.text.into_bytes())).collect()
                };
                let set_aside = lib.load_bytes(&bytes(files), &bytes(backups));
                json!({ "songs": lib.len(), "warnings": lib.warnings(), "set_aside": set_aside })
            }
            Request::Found { files } => Value::from(found(lib, files)),
            Request::Songs { search, sort, descending } => {
                let search = Search::new(&search);
                let mut list: Vec<&Track> = lib.tracks().filter(|t| search.matches(t)).collect();
                query::sort(&mut list, sort, descending);
                list.truncate(MAX_LIST);
                songs(list)
            }
            Request::Albums => {
                let albums: Vec<Value> = query::albums(lib.tracks())
                    .into_iter()
                    .map(|a| {
                        json!({
                            "key": a.key, "title": a.title, "artist": a.artist, "year": a.year,
                            "genre": a.genre, "added": a.added, "duration_ms": a.duration_ms, "songs": a.tracks,
                        })
                    })
                    .collect();
                Value::from(albums)
            }
            Request::Song { id } => song(lib.track(&id).ok_or("there is no such song")?),
            Request::Playlists => serde_json::to_value(lib.playlists()).map_err(text)?,
            Request::PlaylistSongs { id } => {
                let list = lib.playlist(&id).ok_or("there is no such playlist")?;
                songs(playlist_songs(lib, list))
            }
            Request::SetLoved { id, loved } => lib.set_loved(&id, loved).map(|()| done()).map_err(text)?,
            Request::SetRating { id, stars } => lib.set_rating(&id, stars).map(|()| done()).map_err(text)?,
            Request::Played { id } => lib.record_play(&id).map(|()| done()).map_err(text)?,
            Request::Skipped { id } => lib.record_skip(&id).map(|()| done()).map_err(text)?,
            Request::CreatePlaylist { name, songs } => {
                Value::from(lib.create_playlist(&name, songs.iter().map(|s| Item::local(s)).collect(), None))
            }
            Request::AddToPlaylist { id, songs } => lib
                .add_to_playlist(&id, songs.iter().map(|s| Item::local(s)).collect())
                .map(|()| done())
                .map_err(text)?,
            Request::RenamePlaylist { id, name } => lib.rename_playlist(&id, &name).map(|()| done()).map_err(text)?,
            Request::DeletePlaylist { id } => lib.delete_playlist(&id).map(|()| done()).map_err(text)?,
            Request::Unsaved => {
                let mut out = Vec::new();
                for (name, bytes) in lib.unsaved().map_err(text)? {
                    out.push(NamedText { name, text: String::from_utf8(bytes).map_err(text)? });
                }
                serde_json::to_value(out).map_err(text)?
            }
            Request::Saved { names } => {
                for name in &names {
                    lib.mark_saved(name);
                }
                done()
            }
            Request::State => json!({ "dirty": lib.is_dirty(), "revision": lib.revision(), "songs": lib.len() }),
        })
    }
}

/// Files on the phone: the library learns which songs are here, and adds files it does not know
/// with names guessed from the path, as it does for a file a sync service has not downloaded.
fn found(lib: &mut Library, files: Vec<FoundFile>) -> usize {
    let known = lib.known();
    let mut scan = ScanResult::default();
    for f in files.into_iter().take(MAX_LIST) {
        let path: String = f.path.nfc().collect();
        if musiccraft_library::store::resolve(Path::new(""), &path).is_none() || !musiccraft_library::is_audio(&path) {
            continue;
        }
        if !known.contains_key(&path) {
            let mut info: Info = tags::from_path(&path);
            info.size = f.size;
            scan.read.push(info);
        }
        scan.presence.insert(path, Presence::Here);
    }
    lib.apply_scan(scan)
}

fn playlist_songs<'a>(lib: &'a Library, list: &Playlist) -> Vec<&'a Track> {
    match &list.smart {
        Some(rules) => rules.select(lib.tracks(), now_ms()),
        None => list
            .items
            .iter()
            .filter(|i| i.source == musiccraft_library::Source::Local)
            .filter_map(|i| lib.track(&i.id))
            .collect(),
    }
}

/// A song as the apps get it: its fields, and `here` when its file is on this phone.
fn song(t: &Track) -> Value {
    let mut v = serde_json::to_value(t).unwrap_or(Value::Null);
    if let Value::Object(map) = &mut v {
        map.insert("here".to_owned(), Value::from(t.presence == Presence::Here));
    }
    v
}

fn songs(list: Vec<&Track>) -> Value {
    Value::from(list.into_iter().map(song).collect::<Vec<_>>())
}
