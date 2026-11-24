//! Smart playlists: rules that choose songs.

use crate::model::{Track, fold};
use crate::query::Sort;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Field {
    Title,
    Artist,
    Album,
    Genre,
    Composer,
    Year,
    Rating,
    Loved,
    Plays,
    /// Days since the song joined the library.
    AddedDays,
    /// Days since the song was last played; a song never played has none.
    PlayedDays,
    Minutes,
}

pub const FIELDS: [Field; 12] = [
    Field::Title,
    Field::Artist,
    Field::Album,
    Field::Genre,
    Field::Composer,
    Field::Year,
    Field::Rating,
    Field::Loved,
    Field::Plays,
    Field::AddedDays,
    Field::PlayedDays,
    Field::Minutes,
];

impl Field {
    pub fn is_text(self) -> bool {
        matches!(self, Field::Title | Field::Artist | Field::Album | Field::Genre | Field::Composer)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Op {
    Is,
    IsNot,
    /// Text fields only.
    Contains,
    /// Numbers only.
    Over,
    /// Numbers only.
    Under,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Rule {
    pub field: Field,
    pub op: Op,
    pub value: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Rules {
    /// All the rules, or any of them.
    pub all: bool,
    pub rules: Vec<Rule>,
    /// At most this many songs; 0 is no limit.
    #[serde(default)]
    pub limit: usize,
    #[serde(default)]
    pub sort: Sort,
    #[serde(default)]
    pub descending: bool,
}

impl Default for Rules {
    fn default() -> Self {
        Rules { all: true, rules: Vec::new(), limit: 0, sort: Sort::Artist, descending: false }
    }
}

fn days_since(ms: u64, now: u64) -> Option<f64> {
    if ms == 0 {
        return None;
    }
    // Precision lost above 2^53 ms, far beyond any date.
    Some(now.saturating_sub(ms) as f64 / 86_400_000.0)
}

impl Rule {
    fn matches(&self, t: &Track, now: u64) -> bool {
        if self.field.is_text() {
            let text = match self.field {
                Field::Title => &t.info.title,
                Field::Artist => &t.info.artist,
                Field::Album => &t.info.album,
                Field::Genre => &t.info.genre,
                _ => &t.info.composer,
            };
            let (text, value) = (fold(text), fold(self.value.trim()));
            return match self.op {
                Op::Is => text == value,
                Op::IsNot => text != value,
                _ => text.contains(&value),
            };
        }
        let number = match self.field {
            Field::Year => Some(f64::from(t.info.year)),
            Field::Rating => Some(f64::from(t.rating)),
            Field::Loved => Some(if t.loved { 1.0 } else { 0.0 }),
            Field::Plays => Some(t.plays as f64),
            Field::AddedDays => days_since(t.added, now),
            Field::PlayedDays => days_since(t.last_played, now),
            _ => Some(t.info.duration_ms as f64 / 60_000.0),
        };
        let Ok(value) = self.value.trim().replace(',', ".").parse::<f64>() else { return false };
        match (number, self.op) {
            (Some(n), Op::Is | Op::Contains) => (n - value).abs() < 0.5,
            (Some(n), Op::IsNot) => (n - value).abs() >= 0.5,
            (Some(n), Op::Over) => n > value,
            (Some(n), Op::Under) => n < value,
            (None, op) => op == Op::Over || op == Op::IsNot,
        }
    }
}

impl Rules {
    /// The songs the rules choose, in order.
    pub fn select<'a>(&self, tracks: impl Iterator<Item = &'a Track>, now: u64) -> Vec<&'a Track> {
        let mut out: Vec<&Track> = tracks
            .filter(|t| {
                if self.all {
                    self.rules.iter().all(|r| r.matches(t, now))
                } else {
                    self.rules.iter().any(|r| r.matches(t, now))
                }
            })
            .collect();
        crate::query::sort(&mut out, self.sort, self.descending);
        if self.limit > 0 {
            out.truncate(self.limit);
        }
        out
    }
}

/// The smart lists every library has.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BuiltIn {
    RecentlyAdded,
    RecentlyPlayed,
    MostPlayed,
    Loved,
}

pub const BUILT_IN: [BuiltIn; 4] =
    [BuiltIn::RecentlyAdded, BuiltIn::RecentlyPlayed, BuiltIn::MostPlayed, BuiltIn::Loved];

impl BuiltIn {
    pub fn rules(self) -> Rules {
        let one = |field, op, value: &str| vec![Rule { field, op, value: value.to_owned() }];
        let (rules, sort, limit) = match self {
            BuiltIn::RecentlyAdded => (one(Field::AddedDays, Op::Under, "90"), Sort::Added, 500),
            BuiltIn::RecentlyPlayed => (one(Field::PlayedDays, Op::Under, "30"), Sort::LastPlayed, 200),
            BuiltIn::MostPlayed => (one(Field::Plays, Op::Over, "0"), Sort::Plays, 50),
            BuiltIn::Loved => (one(Field::Loved, Op::Is, "1"), Sort::Artist, 0),
        };
        Rules { all: true, rules, limit, sort, descending: self != BuiltIn::Loved }
    }
}

#[cfg(test)]
mod tests {
    use super::{Field, Op, Rule, Rules};
    use crate::model::Track;
    use crate::query::Sort;

    fn track(id: &str, artist: &str, plays: u64, last_played: u64) -> Track {
        let mut t = Track { id: id.into(), plays, last_played, ..Track::default() };
        t.info.artist = artist.into();
        t
    }

    /// "All" needs every rule, "any" one; text ignores case and accents; a song never played
    /// counts as played longer ago than any number of days, and never as played recently.
    #[test]
    fn rules_choose_songs() {
        let day = 86_400_000;
        let now = 100 * day;
        let tracks =
            [track("a", "Björk", 5, now - day), track("b", "Bjork", 0, 0), track("c", "Zucchero", 9, now - 50 * day)];
        let rule = |field, op, value: &str| Rule { field, op, value: value.into() };
        let ids = |all, rules: Vec<Rule>| -> Vec<String> {
            let r = Rules { all, rules, sort: Sort::Plays, descending: true, limit: 0 };
            r.select(tracks.iter(), now).into_iter().map(|t| t.id.clone()).collect()
        };
        assert_eq!(ids(true, vec![rule(Field::Artist, Op::Is, "BJORK"), rule(Field::Plays, Op::Over, "0")]), ["a"]);
        assert_eq!(
            ids(false, vec![rule(Field::Artist, Op::Contains, "zucc"), rule(Field::Plays, Op::Under, "1")]),
            ["c", "b"]
        );
        assert_eq!(ids(true, vec![rule(Field::PlayedDays, Op::Under, "30")]), ["a"]);
        assert_eq!(ids(true, vec![rule(Field::PlayedDays, Op::Over, "30")]), ["c", "b"]);
    }
}
