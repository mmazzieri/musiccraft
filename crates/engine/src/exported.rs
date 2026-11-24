//! The library file that iTunes and Apple Music write for other programs (*File › Library › Export
//! Library…*): an XML property list with the songs, their history and the playlists.
//!
//! Sources: Apple's support pages on exporting the library, the public property-list DTD
//! (<https://www.apple.com/DTDs/PropertyList-1.0.dtd>) and the public documentation of the iTunes
//! Library framework for what the values mean. Apple publishes no list of the keys; those read here
//! are the plain words that stand in the exported text. The program's own database (`.itl`,
//! `.musicdb`) is not read.

use craft_plist::{DateValue, Limits, Value, XmlOptions};
use musiccraft_library::History;
use musiccraft_library::model::Info;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

/// The largest file read. A library of 100,000 songs is about 150 MB.
pub const MAX_FILE: u64 = 512 * 1024 * 1024;
const MAX_NODES: usize = 8_000_000;
/// What is taken from the file is bounded too, so that the catalogue stays within what it can read.
const MAX_SONGS: usize = 200_000;
const MAX_LISTS: usize = 5_000;
const MAX_ITEMS: usize = 1_000_000;
const MAX_TEXT: usize = 48 * 1024 * 1024;
const MAX_NAME: usize = 400;
const TOO_LARGE: &str = "the exported library is too large";

/// A song of the exported library.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Song {
    /// Its number in the file; playlists name songs by it.
    pub number: u64,
    pub info: Info,
    pub history: History,
    /// Where its file was, if it had one that MusicCraft may open.
    pub file: Option<PathBuf>,
}

/// A playlist of the exported library.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct List {
    /// Tells it from others at the next import.
    pub origin: String,
    pub name: String,
    pub songs: Vec<u64>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Exported {
    pub songs: Vec<Song>,
    pub lists: Vec<List>,
    /// Films, podcasts, audiobooks and streams, which are left out.
    pub others: usize,
}

fn text(v: &Value, key: &str) -> String {
    let t = v.get(key).and_then(Value::as_str).unwrap_or("").trim();
    t.chars().filter(|c| !c.is_control()).take(MAX_NAME).collect()
}

fn number(v: &Value, key: &str) -> u64 {
    v.get(key).and_then(Value::as_int).and_then(|n| u64::try_from(n).ok()).unwrap_or(0)
}

fn small(v: &Value, key: &str) -> u32 {
    u32::try_from(number(v, key)).unwrap_or(0)
}

fn flag(v: &Value, key: &str) -> bool {
    matches!(v.get(key), Some(Value::Bool(true)))
}

/// A date in milliseconds since 1970; 0 when there is none.
fn date(v: &Value, key: &str) -> u64 {
    let Some(Value::Date(DateValue::Text(t))) = v.get(key) else { return 0 };
    chrono::DateTime::parse_from_rfc3339(t).ok().and_then(|d| u64::try_from(d.timestamp_millis()).ok()).unwrap_or(0)
}

/// Reads the file `path`.
pub fn read(path: &Path) -> Result<Exported, String> {
    let size = std::fs::metadata(path).map_err(|e| e.to_string())?.len();
    if size > MAX_FILE {
        return Err("the file is too large".to_owned());
    }
    parse(&std::fs::read(path).map_err(|e| e.to_string())?, path)
}

/// Reads the exported library in `bytes`; `path` is where it was found, for songs whose files moved
/// together with it.
pub fn parse(bytes: &[u8], path: &Path) -> Result<Exported, String> {
    const WRONG: &str = "this is not an exported library";
    let limit = usize::try_from(MAX_FILE).unwrap_or(usize::MAX);
    let limits = Limits {
        max_input_bytes: limit,
        max_objects: MAX_NODES,
        max_nodes: MAX_NODES,
        max_depth: 16,
        max_expanded_bytes: limit,
    };
    let root = craft_plist::parse_xml_with_options(bytes, limits, XmlOptions::default())
        .map_err(|e| format!("{WRONG}: {e}"))?;
    let Some(Value::Dict(tracks)) = root.get("Tracks") else { return Err(WRONG.to_owned()) };
    let long = |v: &Value, key: &str| -> String {
        // Addresses may be longer than names.
        v.get(key).and_then(Value::as_str).filter(|s| s.len() <= 8_192).unwrap_or("").to_owned()
    };
    let moved = Moved::new(&long(&root, "Music Folder"), path);
    let mut bytes_of_text = 0usize;
    let mut out = Exported::default();
    let mut kept = HashSet::new();
    for (_, t) in tracks {
        let info = Info {
            title: text(t, "Name"),
            artist: text(t, "Artist"),
            album_artist: text(t, "Album Artist"),
            album: text(t, "Album"),
            genre: text(t, "Genre"),
            composer: text(t, "Composer"),
            year: small(t, "Year"),
            track: small(t, "Track Number"),
            disc: small(t, "Disc Number"),
            duration_ms: number(t, "Total Time"),
            ..Info::default()
        };
        let other = ["Podcast", "Has Video", "Movie", "TV Show", "Music Video", "Audiobook", "iTunesU", "Voice Memo"];
        let genre = info.genre.to_lowercase();
        if info.title.is_empty()
            || other.iter().any(|k| flag(t, k))
            || text(t, "Track Type") == "URL"
            || matches!(genre.as_str(), "audiobook" | "audiobooks" | "podcast")
        {
            out.others += 1;
            continue;
        }
        let rating = if flag(t, "Rating Computed") { 0 } else { number(t, "Rating").min(100) };
        let history = History {
            added: date(t, "Date Added"),
            // Twenty for each star; half a star counts as a whole one.
            rating: u8::try_from(rating.div_ceil(20)).unwrap_or(0),
            loved: flag(t, "Loved") || flag(t, "Favorited"),
            plays: number(t, "Play Count"),
            skips: number(t, "Skip Count"),
            last_played: date(t, "Play Date UTC"),
        };
        let song = number(t, "Track ID");
        bytes_of_text += [&info.title, &info.artist, &info.album_artist, &info.album, &info.genre, &info.composer]
            .iter()
            .map(|s| s.len())
            .sum::<usize>();
        if out.songs.len() >= MAX_SONGS || bytes_of_text > MAX_TEXT {
            return Err(TOO_LARGE.to_owned());
        }
        kept.insert(song);
        out.songs.push(Song { number: song, info, history, file: moved.find(&long(t, "Location")) });
    }
    let empty = Vec::new();
    let lists = match root.get("Playlists") {
        Some(Value::Array(a)) => a,
        _ => &empty,
    };
    // A playlist in folders is named after them, since MusicCraft has no folders.
    let folders: HashMap<String, (String, String)> = lists
        .iter()
        .filter(|l| flag(l, "Folder"))
        .map(|l| (text(l, "Playlist Persistent ID"), (text(l, "Name"), text(l, "Parent Persistent ID"))))
        .collect();
    let mut all_items = 0usize;
    for l in lists {
        let built_in = flag(l, "Master") || flag(l, "Folder") || l.get("Distinguished Kind").is_some();
        let hidden = matches!(l.get("Visible"), Some(Value::Bool(false)));
        let items = l.get("Playlist Items").and_then(Value::as_array).unwrap_or_default();
        let songs: Vec<u64> = items.iter().map(|i| number(i, "Track ID")).filter(|n| kept.contains(n)).collect();
        if built_in || hidden || songs.is_empty() {
            continue;
        }
        all_items += songs.len();
        if out.lists.len() >= MAX_LISTS || all_items > MAX_ITEMS {
            return Err(TOO_LARGE.to_owned());
        }
        let mut name = text(l, "Name");
        let mut parent = text(l, "Parent Persistent ID");
        // Folders that contain each other would never end.
        let mut seen = HashSet::new();
        while let Some((folder, up)) = folders.get(&parent).filter(|_| seen.len() < 16) {
            if !seen.insert(parent.clone()) {
                break;
            }
            name = format!("{folder} / {name}").chars().take(MAX_NAME).collect();
            parent.clone_from(up);
        }
        let id = text(l, "Playlist Persistent ID");
        let origin = if id.is_empty() { name.clone() } else { id };
        out.lists.push(List { origin, name, songs });
    }
    if out.songs.is_empty() { Err("the exported library has no songs".to_owned()) } else { Ok(out) }
}

/// Finds files named in the library, also when the library was carried to another place or computer
/// together with them.
struct Moved {
    /// The music folder as the library names it.
    old: String,
    /// Where that folder may be now.
    places: Vec<PathBuf>,
}

impl Moved {
    fn new(folder: &str, library: &Path) -> Moved {
        let old = folder.trim_end_matches('/').to_owned();
        let dir = library.parent().unwrap_or(Path::new("")).to_owned();
        let last = old.rsplit('/').next().and_then(decode).filter(|n| !n.is_empty() && !n.contains(['/', '\\', ':']));
        let mut places = vec![dir.clone()];
        places.extend(last.map(|n| dir.join(n)));
        Moved { old, places }
    }

    fn find(&self, location: &str) -> Option<PathBuf> {
        if !location.starts_with("file:") {
            return None;
        }
        // Files on other computers are refused: a library from someone else must not make this
        // computer sign in to theirs.
        if let Ok(p) = craft_docpath::resolve_absolute(location, None)
            && is_local_file(&p)
        {
            return Some(p);
        }
        let rest = location.strip_prefix(&self.old).filter(|_| !self.old.is_empty())?;
        let parts: Option<Vec<String>> = rest.split('/').filter(|p| !p.is_empty()).map(decode).collect();
        let parts = parts?;
        if parts.iter().any(|p| p == "." || p == ".." || p.contains(['/', '\\', ':', '\0'])) {
            return None;
        }
        self.places.iter().map(|d| parts.iter().fold(d.clone(), |d, p| d.join(p))).find(|p| is_local_file(p))
    }
}

/// Whether a file is there and reading it stays on this computer. Links on the way are followed only
/// when they lead to a local place, and each is looked at before anything behind it is touched.
pub fn is_local_file(path: &Path) -> bool {
    stays_local(path, 8) && path.is_file()
}

fn stays_local(path: &Path, links_left: usize) -> bool {
    let mut at = PathBuf::new();
    for part in path.components() {
        let parent = at.clone();
        at.push(part);
        let Ok(meta) = std::fs::symlink_metadata(&at) else {
            // A drive prefix alone has no metadata; anything else that cannot be looked at is refused.
            if matches!(part, std::path::Component::Prefix(_)) {
                continue;
            }
            return false;
        };
        if !meta.is_symlink() {
            continue;
        }
        let Ok(target) = std::fs::read_link(&at) else { return false };
        let local = target.to_str().is_some_and(craft_docpath::is_local);
        let target = if target.is_absolute() { target } else { parent.join(target) };
        if !local || links_left == 0 || !stays_local(&target, links_left - 1) {
            return false;
        }
    }
    true
}

/// Undoes `%20` and the like.
fn decode(s: &str) -> Option<String> {
    let mut out = Vec::with_capacity(s.len());
    let mut bytes = s.bytes();
    while let Some(b) = bytes.next() {
        if b == b'%' {
            let hex = [bytes.next()?, bytes.next()?];
            out.push(u8::from_str_radix(std::str::from_utf8(&hex).ok()?, 16).ok()?);
        } else {
            out.push(b);
        }
    }
    String::from_utf8(out).ok()
}

#[cfg(test)]
mod tests {
    use super::{Moved, parse};
    use std::path::Path;

    fn plist(tracks: &str, lists: &str) -> Vec<u8> {
        format!(
            "<?xml version=\"1.0\"?><plist version=\"1.0\"><dict><key>Music Folder</key>\
             <string>file://localhost/Z:/Altro/iTunes%20Media/</string><key>Tracks</key><dict>{tracks}</dict>\
             <key>Playlists</key><array>{lists}</array></dict></plist>"
        )
        .into_bytes()
    }

    fn song(id: u32, more: &str) -> String {
        format!(
            "<key>{id}</key><dict><key>Track ID</key><integer>{id}</integer><key>Name</key><string>S{id}</string>{more}</dict>"
        )
    }

    fn list(name: &str, id: &str, more: &str, songs: &[u32]) -> String {
        let items: String =
            songs.iter().map(|s| format!("<dict><key>Track ID</key><integer>{s}</integer></dict>")).collect();
        format!(
            "<dict><key>Name</key><string>{name}</string><key>Playlist Persistent ID</key><string>{id}</string>{more}\
             <key>Playlist Items</key><array>{items}</array></dict>"
        )
    }

    /// Stars the program worked out itself are not the person's; films and podcasts are not songs; a
    /// playlist keeps its order and repeats, and loses only what is not a song.
    #[test]
    fn songs_history_and_playlists_are_read() {
        let tracks = [
            song(1, "<key>Rating</key><integer>70</integer><key>Play Count</key><integer>12</integer><key>Date Added</key><date>2009-05-01T10:00:00Z</date><key>Loved</key><true/>"),
            song(2, "<key>Rating</key><integer>80</integer><key>Rating Computed</key><true/>"),
            song(3, "<key>Podcast</key><true/>"),
            song(4, "<key>Track Type</key><string>Remote</string>"),
        ]
        .concat();
        let lists = [
            list("Tutto", "A", "<key>Master</key><true/>", &[1, 2, 3, 4]),
            list("Musica", "B", "<key>Distinguished Kind</key><integer>4</integer>", &[1]),
            list("Viaggi", "F", "<key>Folder</key><true/>", &[1]),
            list("Mare", "C", "<key>Parent Persistent ID</key><string>F</string>", &[4, 1, 3, 4]),
        ]
        .concat();
        let e = parse(&plist(&tracks, &lists), Path::new("x/Library.xml")).unwrap();
        assert_eq!((e.songs.len(), e.others), (3, 1));
        let h = &e.songs[0].history;
        assert!(h.loved && h.plays == 12 && h.rating == 4 && h.added > 1_200_000_000_000);
        assert_eq!(e.songs[1].history.rating, 0);
        assert_eq!(e.lists.len(), 1);
        assert_eq!((e.lists[0].name.as_str(), e.lists[0].songs.as_slice()), ("Viaggi / Mare", &[4, 1, 4][..]));
    }

    /// Hostile and foreign files are refused, and folders that contain each other end.
    #[test]
    fn hostile_files_do_not_hang_or_crash() {
        let x = Path::new("x.xml");
        for bad in [
            &b""[..],
            b"<plist><dict/></plist>",
            b"hello",
            b"<plist><dict><key>Tracks</key><array/></dict></plist>",
            b"\xff\xfe<",
        ] {
            assert!(parse(bad, x).is_err());
        }
        assert!(parse("<array>".repeat(100_000).as_bytes(), x).is_err());
        let up = |p: &str| format!("<key>Folder</key><true/><key>Parent Persistent ID</key><string>{p}</string>");
        let lists = [
            list("a", "A", &up("B"), &[]),
            list("b", "B", &up("A"), &[]),
            list("c", "C", "<key>Parent Persistent ID</key><string>A</string>", &[1]),
        ]
        .concat();
        assert_eq!(parse(&plist(&song(1, ""), &lists), x).unwrap().lists.len(), 1);
    }

    /// Names are cut and cleaned, and a file that would fill the catalogue is refused.
    #[test]
    fn what_is_taken_is_bounded() {
        let x = Path::new("x.xml");
        let folder = format!("<key>Folder</key><true/><key>Name</key><string>{}</string>", "f".repeat(5_000));
        let lists = [
            list(&"f".repeat(5_000), "A", &folder, &[]),
            list(&"c".repeat(5_000), "C", "<key>Parent Persistent ID</key><string>A</string>", &[1]),
        ]
        .concat();
        let track = song(1, &format!("<key>Artist</key><string>{}&#10;x</string>", "a".repeat(5_000)));
        let e = parse(&plist(&track, &lists), x).unwrap();
        assert!(e.lists[0].name.len() < 1_000 && e.songs[0].info.artist.len() < 1_000);
        let many: Vec<u32> = vec![1; 11_000];
        let lists: String = (0..100).map(|i| list("x", &format!("L{i}"), "", &many)).collect();
        assert!(parse(&plist(&song(1, ""), &lists), x).is_err());
    }

    /// Files that moved together with the library are found beside it; a location cannot lead to
    /// another computer or out of the folder.
    #[test]
    fn files_are_found_where_they_may_be() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("iTunes Media/Music/Ada \u{c8}/01 S%.mp3");
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(&file, b"x").unwrap();
        std::fs::write(dir.path().join("secret.mp3"), b"x").unwrap();
        let m = Moved::new("file://localhost/Z:/Altro/iTunes%20Media/", &dir.path().join("Library.xml"));
        let base = "file://localhost/Z:/Altro/iTunes%20Media";
        assert_eq!(m.find(&format!("{base}/Music/Ada%20%C3%88/01%20S%25.mp3")).unwrap(), file);
        for bad in ["/../secret.mp3", "/%2e%2e/secret.mp3", "/x%5c..%5c..%5csecret.mp3", "/%zz", "/%"] {
            assert!(m.find(&format!("{base}{bad}")).is_none(), "{bad}");
        }
        for bad in
            ["file://server/share/x.mp3", "file:////server/share/x.mp3", "http://x/y.mp3", "\\\\server\\x.mp3", ""]
        {
            assert!(m.find(bad).is_none(), "{bad}");
        }
        // A link beside the library cannot lead to another computer.
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink("//server/share", dir.path().join("iTunes Media/away")).unwrap();
            std::os::unix::fs::symlink(dir.path().join("iTunes Media/Music"), dir.path().join("iTunes Media/near"))
                .unwrap();
            assert!(m.find(&format!("{base}/away/x.mp3")).is_none());
            assert!(m.find(&format!("{base}/near/Ada%20%C3%88/01%20S%25.mp3")).is_some());
        }
        // A path of this computer, as the library writes it.
        let plain = file
            .to_str()
            .unwrap()
            .replace('\\', "/")
            .replace('%', "%25")
            .replace(' ', "%20")
            .replace('\u{c8}', "%C3%88");
        let url = format!("file://localhost/{}", plain.trim_start_matches('/'));
        assert_eq!(m.find(&url).unwrap(), file);
    }
}
