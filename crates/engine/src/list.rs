//! Song lists as tables: the CSV files that services and playlist movers export.

use musiccraft_library::model::Info;

const MAX_SONGS: usize = 200_000;
const MAX_FIELDS: usize = 64;

/// One line: a song, and the playlist it belongs to, if any.
#[derive(Debug, Clone, PartialEq)]
pub struct Line {
    pub song: Info,
    pub playlist: String,
}

fn column(header: &[String], names: &[&str]) -> Option<usize> {
    let plain = |s: &str| s.trim().trim_matches('"').to_lowercase().replace(['_', '-'], " ");
    // The first name that is there, so "track name" goes before "name".
    names.iter().find_map(|n| header.iter().position(|h| plain(h) == *n))
}

/// Reads a list with a line of headings. Needs at least a title and an artist.
pub fn read(bytes: &[u8]) -> Result<Vec<Line>, String> {
    let text = craft_csv::decode_text(bytes);
    let delimiter = char::from(craft_csv::sniff_delimiter(&text, '"'));
    let records = craft_csv::records(&text, delimiter, '"', MAX_SONGS, MAX_FIELDS);
    let header = records.first().ok_or("the list is empty")?;
    let title = column(header, &["track name", "title", "song", "song name", "track", "titolo", "brano", "name"]);
    let artist =
        column(header, &["artist name(s)", "artist name", "artist names", "artist", "artists", "artista", "artisti"]);
    let (Some(title), Some(artist)) = (title, artist) else {
        return Err("the first line must name a title and an artist column".to_owned());
    };
    let album = column(header, &["album name", "album", "album title"]);
    let isrc = column(header, &["isrc"]);
    let playlist = column(header, &["playlist name", "playlist"]);
    let millis = column(header, &["duration (ms)", "track duration (ms)", "duration ms"]);
    let seconds = column(header, &["duration (s)", "duration (seconds)", "seconds"]);
    let mut out = Vec::new();
    for r in records.iter().skip(1) {
        let get = |i: Option<usize>| i.and_then(|i| r.get(i)).map(|s| s.trim().to_owned()).unwrap_or_default();
        let number = |i: Option<usize>| get(i).parse::<u64>().unwrap_or(0);
        let song = Info {
            title: get(Some(title)),
            // Some lists separate artists with a semicolon.
            artist: get(Some(artist)).replace(';', ", "),
            album: get(album),
            isrc: get(isrc).chars().filter(char::is_ascii_alphanumeric).take(12).collect(),
            duration_ms: match number(millis) {
                0 => number(seconds).saturating_mul(1000),
                n => n,
            },
            ..Info::default()
        };
        if !song.title.is_empty() {
            out.push(Line { song, playlist: get(playlist) });
        }
    }
    if out.is_empty() { Err("the list has no songs".to_owned()) } else { Ok(out) }
}

#[cfg(test)]
mod tests {
    use super::read;

    /// Columns are found by their headings, in any order and with either separator; commas inside a
    /// quoted title do not move the columns.
    #[test]
    fn columns_are_found_by_name() {
        let a = read(b"Artist name;Track name;Playlist name;ISRC\nAda;\"Uno, due\";Mia;IT-B11\nAda;;Mia;\n").unwrap();
        assert_eq!(a.len(), 1);
        assert_eq!((a[0].song.title.as_str(), a[0].playlist.as_str()), ("Uno, due", "Mia"));
        assert_eq!(a[0].song.isrc, "ITB11");
        let b = read(b"Name,Track Name,Artist Name(s),Track Duration (ms)\nx,Alba,\"Ada, Bea\",61000\n").unwrap();
        assert_eq!((b[0].song.title.as_str(), b[0].song.duration_ms), ("Alba", 61_000));
        assert!(read(b"a,b\n1,2\n").is_err());
        assert!(read(b"").is_err());
    }
}
