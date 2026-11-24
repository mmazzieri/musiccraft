//! Reads the library file that iTunes and the Music app export ("Library.xml", an XML property
//! list): songs with their history, playlists and radio stations. Learned from files exported by
//! those programs and from Apple's published property-list format; no other program's code.
//!
//! Smart playlists are kept as the songs they held when the file was written: their rules are
//! stored in a form Apple does not publish.

use crate::History;
use crate::model::Info;
use craft_plist::{DateValue, Limits, Value};
use std::collections::HashSet;
use std::path::{Path, PathBuf};

/// A library file larger than this is refused.
pub const MAX_FILE: u64 = 512 * 1024 * 1024;
const MAX_SONGS: usize = 500_000;
const MAX_PLAYLISTS: usize = 20_000;

/// Seconds from 1970 to 2001, the start of a binary property list's dates.
const APPLE_EPOCH: i64 = 978_307_200;

/// One song of the file.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Song {
    /// The song's number in the file, which its playlists use.
    pub key: i64,
    /// Its tags as the other program knew them; `path` is empty.
    pub info: Info,
    pub history: History,
    /// Where the file was, as the other program wrote it; empty for a song it only streamed.
    pub location: String,
}

/// One playlist of the file.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct List {
    pub name: String,
    /// Song numbers, in order.
    pub songs: Vec<i64>,
    /// Chosen by rules in the other program; here it keeps the songs it held then.
    pub smart: bool,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Export {
    /// The other program's media folder, as an address.
    pub music_folder: String,
    pub songs: Vec<Song>,
    pub playlists: Vec<List>,
    /// Internet radio: name and address.
    pub stations: Vec<(String, String)>,
    /// Videos, podcasts and audiobooks, which are not music and are left out.
    pub not_music: usize,
}

/// Reads an exported library file.
pub fn read(bytes: &[u8]) -> Result<Export, String> {
    let limits = Limits {
        max_input_bytes: usize::try_from(MAX_FILE).unwrap_or(usize::MAX),
        max_objects: 50_000_000,
        max_nodes: 50_000_000,
        max_depth: 16,
        max_expanded_bytes: usize::try_from(MAX_FILE).unwrap_or(usize::MAX),
    };
    let root = if bytes.starts_with(b"bplist") {
        craft_plist::parse_binary_with_limits(bytes, limits)
    } else {
        craft_plist::parse_xml_with_limits(bytes, limits)
    }
    .map_err(|e| e.to_string())?;
    let Some(Value::Dict(tracks)) = root.get("Tracks") else {
        return Err("this is not a music library file: it has no list of songs".to_owned());
    };
    let mut out = Export { music_folder: text(&root, "Music Folder"), ..Export::default() };
    for (_, t) in tracks.iter().take(MAX_SONGS) {
        let Some(key) = int(t, "Track ID") else { continue };
        if flag(t, "Podcast")
            || flag(t, "Movie")
            || flag(t, "TV Show")
            || flag(t, "Has Video")
            || flag(t, "Music Video")
            || flag(t, "iTunesU")
            || flag(t, "Audiobook")
        {
            out.not_music += 1;
            continue;
        }
        let location = text(t, "Location");
        let web = location.starts_with("http://") || location.starts_with("https://");
        if web || text(t, "Track Type") == "URL" {
            if web {
                out.stations.push((text(t, "Name"), location));
            }
            continue;
        }
        if [".m4b", ".aa", ".aax"].iter().any(|e| location.to_ascii_lowercase().ends_with(e)) {
            out.not_music += 1;
            continue;
        }
        let number = |k: &str| int(t, k).and_then(|n| u32::try_from(n).ok()).unwrap_or(0);
        let long = |k: &str| int(t, k).and_then(|n| u64::try_from(n).ok()).unwrap_or(0);
        let info = Info {
            title: text(t, "Name"),
            artist: text(t, "Artist"),
            album_artist: text(t, "Album Artist"),
            album: text(t, "Album"),
            genre: text(t, "Genre"),
            composer: text(t, "Composer"),
            year: number("Year"),
            track: number("Track Number"),
            disc: number("Disc Number"),
            duration_ms: long("Total Time"),
            bitrate: number("Bit Rate"),
            size: long("Size"),
            ..Info::default()
        };
        if info.title.trim().is_empty() && location.is_empty() {
            continue;
        }
        // A rating the program worked out from the album's is not the person's.
        let rating = if flag(t, "Rating Computed") { 0 } else { stars(long("Rating")) };
        let history = History {
            added: date(t, "Date Added"),
            plays: long("Play Count"),
            skips: long("Skip Count"),
            last_played: date(t, "Play Date UTC"),
            rating,
            loved: flag(t, "Loved") || flag(t, "Favorited"),
        };
        out.songs.push(Song { key, info, history, location });
    }
    let kept: HashSet<i64> = out.songs.iter().map(|s| s.key).collect();
    for p in root.get("Playlists").and_then(Value::as_array).unwrap_or_default().iter().take(MAX_PLAYLISTS) {
        // The whole library, the program's own lists (Music, Movies, Purchased…) and folders.
        if flag(p, "Master") || int(p, "Distinguished Kind").is_some() || flag(p, "Folder") {
            continue;
        }
        if p.get("Visible") == Some(&Value::Bool(false)) {
            continue;
        }
        let songs: Vec<i64> = p
            .get("Playlist Items")
            .and_then(Value::as_array)
            .unwrap_or_default()
            .iter()
            .filter_map(|i| int(i, "Track ID"))
            .filter(|k| kept.contains(k))
            .collect();
        let name = text(p, "Name");
        if !name.trim().is_empty() {
            out.playlists.push(List { name, songs, smart: p.get("Smart Info").is_some() });
        }
    }
    Ok(out)
}

fn text(v: &Value, key: &str) -> String {
    v.get(key).and_then(Value::as_str).map(|s| s.trim().to_owned()).unwrap_or_default()
}

fn int(v: &Value, key: &str) -> Option<i64> {
    v.get(key).and_then(Value::as_int).and_then(|i| i64::try_from(i).ok())
}

fn flag(v: &Value, key: &str) -> bool {
    v.get(key) == Some(&Value::Bool(true))
}

/// 0 to 100 in steps of 20, as stars.
fn stars(rating: u64) -> u8 {
    u8::try_from(rating.saturating_add(10) / 20).unwrap_or(5).min(5)
}

/// Milliseconds since 1970, or 0.
fn date(v: &Value, key: &str) -> u64 {
    let seconds = match v.get(key) {
        Some(Value::Date(DateValue::Text(t))) => {
            chrono::DateTime::parse_from_rfc3339(t.trim()).map(|d| d.timestamp_millis()).unwrap_or(0)
        }
        // Precision below a millisecond does not matter; the clamp keeps the cast in range.
        Some(Value::Date(DateValue::Seconds(s))) if s.is_finite() => {
            ((s.clamp(-1e12, 1e12) * 1000.0) as i64).saturating_add(APPLE_EPOCH * 1000)
        }
        _ => 0,
    };
    u64::try_from(seconds).unwrap_or(0)
}

/// The file a song's address names, then the same place below the folder the library file is in:
/// a library file read on another computer, or after its folder moved, still finds its songs
/// when the media folder came along with it.
pub fn candidates(location: &str, music_folder: &str, beside: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let Some(file) = file_path(location) else { return out };
    out.push(PathBuf::from(&file));
    if let Some(folder) = file_path(music_folder) {
        let folder = folder.trim_end_matches('/');
        if let Some(rest) = file.strip_prefix(folder).and_then(|r| r.strip_prefix('/')) {
            let name = folder.rsplit('/').next().unwrap_or("");
            let parts: Vec<&str> = rest.split('/').collect();
            if !name.is_empty() && parts.iter().all(|p| !p.is_empty() && *p != "..") {
                let mut moved = beside.join(name);
                for p in &parts {
                    moved.push(p);
                }
                out.push(moved);
            }
        }
    }
    out
}

/// The path of a `file:` address: `file://localhost/C:/Music/x.mp3` is `C:/Music/x.mp3`,
/// `file:///Users/a/x.m4a` is `/Users/a/x.m4a`.
fn file_path(url: &str) -> Option<String> {
    let rest = url.strip_prefix("file://")?;
    let rest = rest.strip_prefix("localhost").unwrap_or(rest);
    let decoded = percent_decode(rest)?;
    // A Windows drive: "/C:/…".
    let bytes = decoded.as_bytes();
    let drive = bytes.len() > 2 && bytes.first() == Some(&b'/') && bytes.get(2) == Some(&b':');
    let path = if drive { decoded.get(1..)?.to_owned() } else { decoded };
    if path.contains('\0') || path.is_empty() { None } else { Some(path) }
}

fn percent_decode(s: &str) -> Option<String> {
    let mut out = Vec::with_capacity(s.len());
    let mut bytes = s.bytes();
    while let Some(b) = bytes.next() {
        if b == b'%' {
            let hex = [bytes.next()?, bytes.next()?];
            let hex = std::str::from_utf8(&hex).ok()?;
            out.push(u8::from_str_radix(hex, 16).ok()?);
        } else {
            out.push(b);
        }
    }
    String::from_utf8(out).ok()
}

#[cfg(test)]
mod tests;
