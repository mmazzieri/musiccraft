//! Sorting, searching and grouping songs.

use crate::model::{Track, fold};
use serde::{Deserialize, Serialize};
use std::cmp::Ordering;
use std::collections::BTreeMap;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Sort {
    Title,
    /// Artist, then album, disc and track.
    #[default]
    Artist,
    /// Album, then disc and track.
    Album,
    Genre,
    Year,
    Time,
    Rating,
    Loved,
    Plays,
    Added,
    LastPlayed,
    Kind,
    Bitrate,
}

/// The text a name is sorted by: without case, accents and a leading article.
pub fn sort_key(name: &str) -> String {
    let f = fold(name);
    for article in ["the ", "a ", "an "] {
        if let Some(rest) = f.strip_prefix(article) {
            return rest.to_owned();
        }
    }
    f
}

fn position(t: &Track) -> (u32, u32) {
    (t.info.disc.max(1), t.info.track)
}

/// Sorts songs; equal ones keep a stable order by artist, album and position.
pub fn sort(list: &mut [&Track], by: Sort, descending: bool) {
    let full = |t: &Track| {
        (sort_key(t.album_artist()), t.info.year, sort_key(&t.info.album), position(t), sort_key(&t.info.title))
    };
    let mut keyed: Vec<(String, _, &Track)> = list
        .iter()
        .map(|t| {
            let text = match by {
                Sort::Title => sort_key(&t.info.title),
                Sort::Artist => sort_key(&t.info.artist),
                Sort::Album => sort_key(&t.info.album),
                Sort::Genre => sort_key(&t.info.genre),
                Sort::Kind => t.kind(),
                _ => String::new(),
            };
            (text, full(t), *t)
        })
        .collect();
    keyed.sort_by(|a, b| {
        let first = match by {
            Sort::Title | Sort::Artist | Sort::Album | Sort::Genre | Sort::Kind => a.0.cmp(&b.0),
            Sort::Year => a.2.info.year.cmp(&b.2.info.year),
            Sort::Time => a.2.info.duration_ms.cmp(&b.2.info.duration_ms),
            Sort::Rating => a.2.rating.cmp(&b.2.rating),
            Sort::Loved => a.2.loved.cmp(&b.2.loved),
            Sort::Plays => a.2.plays.cmp(&b.2.plays),
            Sort::Added => a.2.added.cmp(&b.2.added),
            Sort::LastPlayed => a.2.last_played.cmp(&b.2.last_played),
            Sort::Bitrate => a.2.info.bitrate.cmp(&b.2.info.bitrate),
        };
        let first = if descending { first.reverse() } else { first };
        if first != Ordering::Equal {
            return first;
        }
        let rest = if by == Sort::Album {
            position(a.2).cmp(&position(b.2)).then_with(|| a.1.cmp(&b.1))
        } else {
            a.1.cmp(&b.1)
        };
        rest.then_with(|| a.2.id.cmp(&b.2.id))
    });
    for (slot, (_, _, t)) in list.iter_mut().zip(keyed) {
        *slot = t;
    }
}

/// A search text, split into words that must all appear.
pub struct Search(Vec<String>);

impl Search {
    pub fn new(text: &str) -> Search {
        Search(fold(text).split_whitespace().map(str::to_owned).collect())
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn matches(&self, t: &Track) -> bool {
        if self.0.is_empty() {
            return true;
        }
        let i = &t.info;
        let all =
            fold(&format!("{}\n{}\n{}\n{}\n{}\n{}", i.title, i.artist, i.album, i.album_artist, i.genre, i.composer));
        self.0.iter().all(|w| all.contains(w))
    }
}

/// An album, with its songs in order.
#[derive(Debug, Clone)]
pub struct Album {
    pub key: String,
    pub title: String,
    pub artist: String,
    pub year: u32,
    pub genre: String,
    pub added: u64,
    pub tracks: Vec<String>,
    pub duration_ms: u64,
}

/// The albums of `tracks`, by artist, year and title.
pub fn albums<'a>(tracks: impl Iterator<Item = &'a Track>) -> Vec<Album> {
    let mut by_key: BTreeMap<String, Vec<&Track>> = BTreeMap::new();
    for t in tracks {
        by_key.entry(t.album_key()).or_default().push(t);
    }
    let mut out: Vec<Album> = by_key
        .into_iter()
        .filter_map(|(key, mut list)| {
            sort(&mut list, Sort::Album, false);
            let first = list.first()?;
            Some(Album {
                key,
                title: first.info.album.clone(),
                artist: first.album_artist().to_owned(),
                year: list.iter().map(|t| t.info.year).max().unwrap_or(0),
                genre: first.info.genre.clone(),
                added: list.iter().map(|t| t.added).max().unwrap_or(0),
                duration_ms: list.iter().map(|t| t.info.duration_ms).sum(),
                tracks: list.iter().map(|t| t.id.clone()).collect(),
            })
        })
        .collect();
    out.sort_by_cached_key(|a| (sort_key(&a.artist), a.year, sort_key(&a.title)));
    out
}

/// The distinct values of a field with how many songs have each, in order. Spellings that
/// differ only in case or accents are one value.
pub fn facet<'a>(
    tracks: impl Iterator<Item = &'a Track>,
    value: impl Fn(&'a Track) -> &'a str,
) -> Vec<(String, usize)> {
    let mut seen: BTreeMap<String, (String, usize)> = BTreeMap::new();
    for t in tracks {
        let v = value(t);
        seen.entry(sort_key(v)).or_insert_with(|| (v.to_owned(), 0)).1 += 1;
    }
    seen.into_values().collect()
}

#[cfg(test)]
mod tests {
    use super::{Search, Sort, albums, sort};
    use crate::model::Track;

    fn t(id: &str, artist: &str, album: &str, disc: u32, track: u32) -> Track {
        let mut t = Track { id: id.into(), ..Track::default() };
        t.info.artist = artist.into();
        t.info.album = album.into();
        t.info.disc = disc;
        t.info.track = track;
        t
    }

    /// Songs of an album stay in disc and track order, "The" does not count, and reversing the
    /// artists does not reverse the songs inside an album.
    #[test]
    fn sorting_keeps_albums_together() {
        let list = [
            t("4", "Zed", "A", 1, 1),
            t("2", "The Beta", "X", 1, 2),
            t("3", "Beta", "X", 2, 1),
            t("1", "alpha", "Q", 0, 1),
        ];
        let ids = |by, desc| -> String {
            let mut v: Vec<&Track> = list.iter().collect();
            sort(&mut v, by, desc);
            v.iter().map(|t| t.id.as_str()).collect()
        };
        assert_eq!(ids(Sort::Artist, false), "1234");
        assert_eq!(ids(Sort::Artist, true), "4231");
    }

    /// Every word must be found, in any field.
    #[test]
    fn search_needs_every_word() {
        let x = t("1", "Lucio Città", "Notte", 1, 1);
        assert!(Search::new("notte citta").matches(&x));
        assert!(!Search::new("notte roma").matches(&x));
    }

    /// Two albums with one title by different artists stay apart.
    #[test]
    fn albums_group_by_artist_and_title() {
        let list = [t("1", "A", "Hits", 1, 2), t("2", "A", "HITS", 1, 1), t("3", "B", "Hits", 1, 1)];
        let a = albums(list.iter());
        assert_eq!(a.len(), 2);
        assert_eq!(a[0].tracks, ["2", "1"]);
    }
}
