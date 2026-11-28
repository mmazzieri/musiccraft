//! Tags and pictures of music files, through `lofty`.

use crate::model::Info;
use lofty::config::WriteOptions;
use lofty::file::{AudioFile, TaggedFileExt};
use lofty::picture::PictureType;
use lofty::tag::{Accessor, ItemKey, Tag, TagExt, TagType};
use std::path::Path;

/// The largest picture read from a file or a folder.
const MAX_PICTURE: u64 = 24 * 1024 * 1024;
/// The most lyrics read from a file or written into one, in bytes.
pub const MAX_LYRICS: usize = 64 * 1024;

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
    set(&mut info.isrc, tag.get_string(ItemKey::Isrc).map(str::to_owned));
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

/// The unsynchronised lyrics inside the file, empty when it has none. Longer lyrics are cut at
/// [`MAX_LYRICS`].
pub fn lyrics(file: &Path) -> Result<String, String> {
    let tagged = lofty::read_from_path(file).map_err(|e| e.to_string())?;
    let tags = tagged.primary_tag().into_iter().chain(tagged.tags());
    let found = tags
        .flat_map(|t| [ItemKey::UnsyncLyrics, ItemKey::Lyrics].map(|k| t.get_string(k)))
        .flatten()
        .map(|v| v.trim_matches(['\0', ' ', '\r', '\n', '\t']))
        .find(|v| !v.is_empty())
        .unwrap_or("");
    let mut end = found.len().min(MAX_LYRICS);
    while !found.is_char_boundary(end) {
        end -= 1;
    }
    Ok(found.get(..end).unwrap_or("").replace("\r\n", "\n").replace('\r', "\n"))
}

/// Writes the lyrics into the file's own tags; empty text takes them away.
pub fn write_lyrics(file: &Path, text: &str) -> Result<(), String> {
    if text.len() > MAX_LYRICS {
        return Err(format!("lyrics are kept up to {} KB", MAX_LYRICS / 1024));
    }
    let text: String = text.chars().filter(|c| *c != '\0').collect();
    let text = text.trim_end();
    let mut tagged = lofty::read_from_path(file).map_err(|e| e.to_string())?;
    let kind = tagged.primary_tag_type();
    let keys = [ItemKey::Lyrics, ItemKey::UnsyncLyrics];
    // Lyrics in the file's other tags are taken away too, or they would be read again.
    let others: Vec<TagType> = tagged
        .tags()
        .iter()
        .filter(|t| t.tag_type() != kind && keys.iter().any(|k| t.get(*k).is_some()))
        .map(Tag::tag_type)
        .collect();
    for other in others {
        if let Some(t) = tagged.tag_mut(other) {
            for k in keys {
                t.remove_key(k);
            }
            t.save_to_path(file, WriteOptions::default()).map_err(|e| e.to_string())?;
        }
    }
    if tagged.primary_tag().is_none() {
        // A new tag of the main kind is read first: it starts with what the file's other tag says.
        let mut fresh = Tag::new(kind);
        if let Some(other) = tagged.first_tag() {
            for item in other.items() {
                fresh.insert(item.clone());
            }
            for picture in other.pictures() {
                fresh.push_picture(picture.clone());
            }
        }
        tagged.insert_tag(fresh);
    }
    let tag = tagged.primary_tag_mut().ok_or("this kind of file has no tags")?;
    for k in keys {
        tag.remove_key(k);
    }
    // Each kind of tag has its own name for them; ID3v2 keeps unsynchronised ones apart.
    if !text.is_empty()
        && !tag.insert_text(ItemKey::Lyrics, text.to_owned())
        && !tag.insert_text(ItemKey::UnsyncLyrics, text.to_owned())
    {
        return Err("this kind of file cannot hold lyrics".to_owned());
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
    use super::{from_path, lyrics, read, write_lyrics};
    use lofty::config::WriteOptions;
    use lofty::tag::{Accessor, Tag, TagExt, TagType};

    /// A WAV file of a few silent samples.
    fn wav(path: &std::path::Path) {
        let data = [128u8; 64];
        let mut b = Vec::new();
        b.extend_from_slice(b"RIFF");
        b.extend_from_slice(&(36 + data.len() as u32).to_le_bytes());
        b.extend_from_slice(b"WAVEfmt ");
        b.extend_from_slice(&16u32.to_le_bytes());
        b.extend_from_slice(&[1, 0, 1, 0]);
        b.extend_from_slice(&8000u32.to_le_bytes());
        b.extend_from_slice(&8000u32.to_le_bytes());
        b.extend_from_slice(&[1, 0, 8, 0]);
        b.extend_from_slice(b"data");
        b.extend_from_slice(&(data.len() as u32).to_le_bytes());
        b.extend_from_slice(&data);
        std::fs::write(path, b).unwrap();
    }

    /// Lyrics written into a file whose only tag is of another kind keep what that tag says.
    #[test]
    fn lyrics_keep_what_another_tag_says() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("song.wav");
        wav(&file);
        let mut info = Tag::new(TagType::RiffInfo);
        info.set_title("Titolo vero".to_owned());
        info.save_to_path(&file, WriteOptions::default()).unwrap();
        assert_eq!(lyrics(&file).unwrap(), "");
        write_lyrics(&file, "Nuovo testo").unwrap();
        assert_eq!(read(&file, "song.wav").unwrap().title, "Titolo vero");
        assert_eq!(lyrics(&file).unwrap(), "Nuovo testo");
        write_lyrics(&file, "").unwrap();
        assert_eq!(lyrics(&file).unwrap(), "");
        assert_eq!(read(&file, "song.wav").unwrap().title, "Titolo vero");
    }

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
