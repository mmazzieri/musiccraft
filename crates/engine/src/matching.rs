//! Finds a library song on a streaming service.
//!
//! The recording's code decides when both sides have it. Otherwise title, artist, album and length
//! are weighed, and only a clear match is taken: a song that is not found is better than a wrong
//! one, and the person can always choose by hand.

use musiccraft_library::model::Info;
use musiccraft_library::names::{first_artist, plain_title, words};
use musiccraft_services::spotify::Spotify;
use musiccraft_services::subsonic::Server;
use musiccraft_services::{Entry, ServiceError};
use std::collections::HashSet;
use std::sync::Arc;

/// From this score on, a song is taken for the same.
const ENOUGH: f64 = 0.8;
/// Words that name another recording when only one of the titles has them.
const TAKES: &[&str] =
    &["live", "remix", "mix", "karaoke", "instrumental", "acoustic", "demo", "cover", "tribute", "edit", "version"];

/// A service with its account.
#[derive(Clone)]
pub enum Account {
    Server(Server),
    Spotify(Arc<Spotify>),
}

/// How alike two names are, from 0 to 1.
fn alike(a: &[String], b: &[String]) -> f64 {
    if a.is_empty() || b.is_empty() {
        return 0.0;
    }
    if a == b {
        return 1.0;
    }
    let (x, y): (HashSet<&String>, HashSet<&String>) = (a.iter().collect(), b.iter().collect());
    let common = x.intersection(&y).count() as f64;
    2.0 * common / (x.len() + y.len()) as f64
}

/// How surely `got` is the song `want`, from 0 to 1.
pub fn score(want: &Info, got: &Entry) -> f64 {
    if !want.isrc.is_empty() && want.isrc.eq_ignore_ascii_case(&got.isrc) {
        return 1.0;
    }
    let (wt, gt) = (words(&plain_title(&want.title)), words(&plain_title(&got.title)));
    let title = alike(&wt, &gt);
    let takes = |w: &[String]| -> Vec<String> { w.iter().filter(|x| TAKES.contains(&x.as_str())).cloned().collect() };
    if title < 0.75 || takes(&wt) != takes(&gt) {
        return 0.0;
    }
    let asked = if want.artist.is_empty() { &want.album_artist } else { &want.artist };
    let whole = alike(&words(asked), &words(&got.artist));
    let first = alike(&words(first_artist(asked)), &words(first_artist(&got.artist)));
    let artist = whole.max(first);
    if artist < 0.5 {
        return 0.0;
    }
    let length = match (want.duration_ms, got.duration_ms) {
        (0, _) | (_, 0) => 0.5,
        (a, b) => match a.abs_diff(b) {
            0..=3_000 => 1.0,
            3_001..=8_000 => 0.7,
            8_001..=15_000 => 0.3,
            _ => return 0.0,
        },
    };
    let (wa, ga) = (words(&plain_title(&want.album)), words(&plain_title(&got.album)));
    let album = if wa.is_empty() || ga.is_empty() { 0.5 } else { alike(&wa, &ga) };
    title * 0.4 + artist * 0.3 + length * 0.2 + album * 0.1
}

/// The surest of the songs a service gave, if it is sure enough.
pub fn best(want: &Info, found: &[Entry]) -> Option<Entry> {
    let scored = found.iter().map(|e| (score(want, e), e));
    let (s, e) = scored.max_by(|a, b| a.0.total_cmp(&b.0))?;
    (s >= ENOUGH).then(|| e.clone())
}

/// What a person would type to find the song.
pub fn query(want: &Info) -> String {
    let artist = if want.artist.is_empty() { &want.album_artist } else { &want.artist };
    format!("{} {}", plain_title(&want.title), first_artist(artist)).trim().to_owned()
}

impl Account {
    /// Asks the service for the song, the surest way first.
    pub fn find(&self, want: &Info) -> Result<Option<Entry>, ServiceError> {
        let title = plain_title(&want.title);
        let artist = first_artist(if want.artist.is_empty() { &want.album_artist } else { &want.artist });
        let mut asks = Vec::new();
        match self {
            Account::Server(_) => asks.extend([query(want), title]),
            Account::Spotify(_) => {
                let quoted = |s: &str| s.replace('"', " ");
                if !want.isrc.is_empty() && want.isrc.chars().all(|c| c.is_ascii_alphanumeric()) {
                    asks.push(format!("isrc:{}", want.isrc));
                }
                asks.push(format!("track:\"{}\" artist:\"{}\"", quoted(&title), quoted(artist)));
                asks.push(query(want));
            }
        }
        for ask in asks.iter().filter(|a| !a.trim().is_empty()) {
            let found = match self {
                Account::Server(s) => s.search(ask)?.0,
                Account::Spotify(s) => s.find(ask, 1)?,
            };
            if let Some(e) = best(want, &found) {
                return Ok(Some(e));
            }
        }
        Ok(None)
    }
}

#[cfg(test)]
mod tests {
    use super::{best, score};
    use musiccraft_library::model::Info;
    use musiccraft_services::Entry;

    fn want() -> Info {
        Info {
            title: "Faro spento".into(),
            artist: "Lanterna Blu".into(),
            album: "Porti d'inverno".into(),
            duration_ms: 214_000,
            ..Info::default()
        }
    }

    fn got(id: &str, title: &str, artist: &str, album: &str, seconds: u64) -> Entry {
        Entry {
            id: id.into(),
            title: title.into(),
            artist: artist.into(),
            album: album.into(),
            duration_ms: seconds * 1000,
            ..Entry::default()
        }
    }

    /// The song is found under the names stores give it, also on a compilation, and the one on its
    /// own album is preferred.
    #[test]
    fn the_same_song_is_found() {
        let list = [
            got("hits", "Faro spento", "Lanterna Blu", "Il meglio di", 214),
            got("own", "Faro Spento - 2021 Remaster", "Lanterna Blu, Ada Verdi", "Porti D\u{2019}Inverno", 215),
        ];
        assert_eq!(best(&want(), &list).unwrap().id, "own");
        assert_eq!(best(&want(), &list[..1]).unwrap().id, "hits");
    }

    /// Another take, another artist, another length or a title that only resembles it is not taken.
    #[test]
    fn another_song_is_not_taken() {
        let album = "Porti d'inverno";
        for e in [
            got("live", "Faro spento (Live)", "Lanterna Blu", album, 214),
            got("karaoke", "Faro spento (Karaoke Version)", "Lanterna Blu", album, 214),
            got("other", "Faro spento", "Nonna Elettrica", album, 214),
            got("long", "Faro spento", "Lanterna Blu", album, 420),
            got("near", "Faro", "Lanterna Blu", album, 214),
        ] {
            assert!(best(&want(), std::slice::from_ref(&e)).is_none(), "{}: {}", e.id, score(&want(), &e));
        }
    }

    /// The recording's code settles it, whatever the names look like.
    #[test]
    fn the_code_decides() {
        let w = Info { isrc: "ITB112100001".into(), ..want() };
        let e = Entry { isrc: "itb112100001".into(), ..got("x", "\u{706f}\u{53f0}", "?", "", 0) };
        assert!(best(&w, &[e]).is_some());
    }
}
