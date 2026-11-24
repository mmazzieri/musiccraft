//! Playlist files: extended M3U (`.m3u`, `.m3u8`, UTF-8) read and written, PLS read.

/// One line of a playlist file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    /// A path or an address.
    pub location: String,
    pub title: String,
}

/// Reads an M3U or PLS file. Lines it does not understand are passed over.
pub fn read(bytes: &[u8]) -> Vec<Entry> {
    let text = match std::str::from_utf8(bytes) {
        Ok(s) => s.trim_start_matches('\u{feff}').to_owned(),
        // Old `.m3u` files are in Latin-1, whose bytes are the first 256 characters.
        Err(_) => bytes.iter().map(|b| char::from(*b)).collect(),
    };
    if text.trim_start().to_ascii_lowercase().starts_with("[playlist]") {
        return read_pls(&text);
    }
    let mut out = Vec::new();
    let mut title = String::new();
    for line in text.lines().map(str::trim).filter(|l| !l.is_empty()) {
        if let Some(rest) = line.strip_prefix("#EXTINF:") {
            title = rest.split_once(',').map_or("", |(_, t)| t).trim().to_owned();
        } else if !line.starts_with('#') {
            out.push(Entry { location: line.to_owned(), title: std::mem::take(&mut title) });
        }
    }
    out
}

fn read_pls(text: &str) -> Vec<Entry> {
    let mut out: std::collections::BTreeMap<u32, Entry> = std::collections::BTreeMap::new();
    for line in text.lines() {
        let Some((key, value)) = line.trim().split_once('=') else { continue };
        let key = key.to_ascii_lowercase();
        let (is_file, n) = match (key.strip_prefix("file"), key.strip_prefix("title")) {
            (Some(n), _) => (true, n),
            (_, Some(n)) => (false, n),
            _ => continue,
        };
        let Ok(n) = n.parse::<u32>() else { continue };
        let e = out.entry(n).or_insert_with(|| Entry { location: String::new(), title: String::new() });
        if is_file {
            value.trim().clone_into(&mut e.location);
        } else {
            value.trim().clone_into(&mut e.title);
        }
    }
    out.into_values().filter(|e| !e.location.is_empty()).collect()
}

/// Writes an extended M3U file: seconds, title and location of each entry.
pub fn write<'a>(entries: impl Iterator<Item = (u64, &'a str, &'a str)>) -> String {
    let one_line = |s: &str| s.replace(['\r', '\n'], " ");
    let mut s = "#EXTM3U\n".to_owned();
    for (seconds, title, location) in entries {
        s.push_str(&format!("#EXTINF:{seconds},{}\n{}\n", one_line(title), one_line(location)));
    }
    s
}

#[cfg(test)]
mod tests {
    use super::{read, write};

    /// What is written is read back, and the titles stay with their songs.
    #[test]
    fn m3u_survives_a_round_trip() {
        let text = write([(10, "Uno", "A/1.mp3"), (0, "Due\nrighe", "http://x/y")].into_iter());
        let back = read(text.as_bytes());
        assert_eq!(back[0].location, "A/1.mp3");
        assert_eq!((back[1].title.as_str(), back[1].location.as_str()), ("Due righe", "http://x/y"));
    }

    #[test]
    fn pls_pairs_titles_and_files() {
        let e = read(b"[playlist]\nTitle2=B\nFile2=http://b\nFile1=http://a\nNumberOfEntries=2");
        assert_eq!(e[1].title, "B");
        assert_eq!(e[0].location, "http://a");
    }
}
