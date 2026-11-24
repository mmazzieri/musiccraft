//! Tags and pictures of music files, through `lofty`.

use crate::model::Info;
use lofty::config::WriteOptions;
use lofty::file::{AudioFile, TaggedFileExt};
use lofty::picture::PictureType;
use lofty::tag::{Accessor, ItemKey, Tag, TagExt};
use std::path::Path;

/// The largest picture read from a file or a folder.
const MAX_PICTURE: u64 = 24 * 1024 * 1024;

/// Reads the tags of `file`; `rel` is its path below `Music/`. Fields without a tag are filled
/// from the path.
pub fn read(file: &Path, rel: &str) -> Result<Info, String> {
    let tagged = lofty::read_from_path(file).map_err(|e| e.to_string())?;
    let mut info = from_path(rel);
    info.guessed = false;
    let props = tagged.properties();
    info.duration_ms = u64::try_from(props.duration().as_millis()).unwrap_or(0);
    info.bitrate = props.audio_bitrate().or(props.overall_bitrate()).unwrap_or(0);
    info.size = std::fs::metadata(file).map(|m| m.len()).unwrap_or(0);
    let Some(tag) = tagged.primary_tag().or_else(|| tagged.first_tag()) else { return Ok(info) };
    let set = |slot: &mut String, value: Option<String>| {
        if let Some(v) = value.map(|v| v.trim().trim_matches('\0').to_owned()).filter(|v| !v.is_empty()) {
            *slot = v;
        }
    };
    set(&mut info.title, tag.title().map(|v| v.into_owned()));
    set(&mut info.artist, tag.artist().map(|v| v.into_owned()));
    set(&mut info.album, tag.album().map(|v| v.into_owned()));
    set(&mut info.genre, tag.genre().map(|v| v.into_owned()));
    set(&mut info.album_artist, tag.get_string(ItemKey::AlbumArtist).map(str::to_owned));
    set(&mut info.composer, tag.get_string(ItemKey::Composer).map(str::to_owned));
    if let Some(n) = tag.track() {
        info.track = n;
    }
    if let Some(n) = tag.disk() {
        info.disc = n;
    }
    for key in [ItemKey::RecordingDate, ItemKey::Year, ItemKey::OriginalReleaseDate] {
        if let Some(y) = tag.get_string(key).and_then(|v| v.get(..4)).and_then(|v| v.parse().ok()) {
            info.year = y;
            break;
        }
    }
    Ok(info)
}

/// What the path says: `Artist/Album/03 Title.mp3`.
pub fn from_path(rel: &str) -> Info {
    let parts: Vec<&str> = rel.split('/').collect();
    let file = parts.last().copied().unwrap_or("");
    let stem = file.rsplit_once('.').map_or(file, |(s, _)| s);
    let digits: String = stem.chars().take_while(char::is_ascii_digit).collect();
    let rest = stem.get(digits.len()..).unwrap_or("").trim_start_matches([' ', '-', '.', '_']);
    let (track, title) = match digits.parse::<u32>() {
        Ok(n) if digits.len() <= 3 && !rest.is_empty() => (n, rest),
        _ => (0, stem),
    };
    let at = |i: usize| parts.len().checked_sub(i).and_then(|k| parts.get(k)).copied().unwrap_or("").to_owned();
    let (artist, album) = match parts.len() {
        0 | 1 => (String::new(), String::new()),
        2 => (at(2), String::new()),
        _ => (at(3), at(2)),
    };
    Info { path: rel.to_owned(), title: title.to_owned(), artist, album, track, guessed: true, ..Info::default() }
}

/// Writes the person's edits into the file's own tags.
pub fn write(file: &Path, info: &Info) -> Result<(), String> {
    let mut tagged = lofty::read_from_path(file).map_err(|e| e.to_string())?;
    let kind = tagged.primary_tag_type();
    if tagged.primary_tag().is_none() {
        tagged.insert_tag(Tag::new(kind));
    }
    let tag = tagged.primary_tag_mut().ok_or("this kind of file has no tags")?;
    tag.set_title(info.title.clone());
    tag.set_artist(info.artist.clone());
    tag.set_album(info.album.clone());
    tag.set_genre(info.genre.clone());
    for (key, value) in [(ItemKey::AlbumArtist, &info.album_artist), (ItemKey::Composer, &info.composer)] {
        tag.remove_key(key);
        if !value.is_empty() {
            tag.insert_text(key, value.clone());
        }
    }
    for key in [ItemKey::RecordingDate, ItemKey::Year] {
        tag.remove_key(key);
    }
    if info.year > 0 {
        tag.insert_text(ItemKey::RecordingDate, info.year.to_string());
    }
    if info.track > 0 {
        tag.set_track(info.track);
    } else {
        tag.remove_track();
    }
    if info.disc > 0 {
        tag.set_disk(info.disc);
    } else {
        tag.remove_disk();
    }
    tag.save_to_path(file, WriteOptions::default()).map_err(|e| e.to_string())
}

/// The front cover inside the file, or a picture beside it.
pub fn picture(file: &Path) -> Option<Vec<u8>> {
    if let Ok(tagged) = lofty::read_from_path(file) {
        let pictures = || tagged.tags().iter().flat_map(Tag::pictures);
        let best = pictures().find(|p| p.pic_type() == PictureType::CoverFront).or_else(|| pictures().next());
        if let Some(p) = best.filter(|p| u64::try_from(p.data().len()).is_ok_and(|n| n <= MAX_PICTURE)) {
            return Some(p.data().to_vec());
        }
    }
    let dir = file.parent()?;
    for name in ["cover.jpg", "cover.png", "folder.jpg", "front.jpg", "cover.jpeg"] {
        let p = dir.join(name);
        if let Ok(m) = std::fs::metadata(&p)
            && m.len() <= MAX_PICTURE
            && !crate::cloud::online_only(&m)
            && let Ok(bytes) = std::fs::read(&p)
        {
            return Some(bytes);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::from_path;

    /// A song whose file is still in the cloud is listed under the artist, album and number its
    /// path gives, and a title that merely starts with digits keeps them.
    #[test]
    fn a_path_gives_artist_album_number_and_title() {
        let i = from_path("Ada Verdi/Maree/03 - Scirocco.flac");
        assert_eq!(
            (i.artist.as_str(), i.album.as_str(), i.track, i.title.as_str()),
            ("Ada Verdi", "Maree", 3, "Scirocco")
        );
        assert_eq!(from_path("A/B/1979.mp3").title, "1979");
        assert_eq!(from_path("lone.mp3").artist, "");
    }
}
