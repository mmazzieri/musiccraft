//! What the library holds, as the person sees it after the merge.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// What is read from a file's tags and properties. Written as one unit: an edit in MusicCraft
/// writes the tags and this record together.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Info {
    /// Path below `Music/`, with `/` between the parts. Empty for a song without a file, which
    /// plays from the streaming service in use.
    pub path: String,
    pub title: String,
    pub artist: String,
    pub album_artist: String,
    pub album: String,
    pub genre: String,
    pub composer: String,
    pub year: u32,
    pub track: u32,
    pub disc: u32,
    pub duration_ms: u64,
    pub bitrate: u32,
    pub size: u64,
    /// The recording's international code, when a tag or a service gives it.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub isrc: String,
    /// Guessed from the file's path, because the file is still only in the cloud.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub guessed: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Track {
    pub id: String,
    #[serde(flatten)]
    pub info: Info,
    /// Milliseconds since 1970 when the song joined the library.
    pub added: u64,
    pub loved: bool,
    /// 0 to 5 stars.
    pub rating: u8,
    pub plays: u64,
    pub skips: u64,
    pub last_played: u64,
    /// What the song is on each service, by [`Source::key`].
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub links: BTreeMap<String, Link>,
    /// Where the file is, seen from this computer.
    #[serde(skip)]
    pub presence: Presence,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Presence {
    /// On this computer.
    #[default]
    Here,
    /// Listed by the sync service but not downloaded yet; playing it downloads it.
    Cloud,
    /// In the library without a file: it plays from the streaming service in use.
    Stream,
    /// Known from another computer, the file has not arrived (or was deleted).
    Missing,
}

impl Presence {
    /// Whether only a streaming service can play the song here.
    pub fn needs_service(self) -> bool {
        matches!(self, Presence::Stream | Presence::Missing)
    }
}

impl Track {
    /// The artist an album is listed under.
    pub fn album_artist(&self) -> &str {
        if self.info.album_artist.is_empty() { &self.info.artist } else { &self.info.album_artist }
    }

    /// Groups the songs of one album.
    pub fn album_key(&self) -> String {
        format!("{}\u{1f}{}", fold(self.album_artist()), fold(&self.info.album))
    }

    pub fn meta(&self) -> Meta {
        Meta {
            title: self.info.title.clone(),
            artist: self.info.artist.clone(),
            album: self.info.album.clone(),
            duration_ms: self.info.duration_ms,
            isrc: self.info.isrc.clone(),
            year: self.info.year,
            track: self.info.track,
            art: String::new(),
        }
    }

    /// The file name extension, in capitals.
    pub fn kind(&self) -> String {
        self.info.path.rsplit_once('.').map(|(_, e)| e.to_ascii_uppercase()).unwrap_or_default()
    }
}

/// Lower case without accents, for search and grouping.
pub fn fold(s: &str) -> String {
    use unicode_normalization::UnicodeNormalization;
    s.nfd().filter(|c| !unicode_normalization::char::is_combining_mark(*c)).flat_map(char::to_lowercase).collect()
}

/// Where an item plays from.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "source", rename_all = "lowercase")]
pub enum Source {
    Local,
    /// A personal music server; `account` is the name given to it on every computer.
    Server {
        account: String,
    },
    Spotify,
    Radio,
}

/// A song, station or stream in a playlist or in the play queue.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Item {
    #[serde(flatten)]
    pub source: Source,
    pub id: String,
    /// What to show for items the library does not hold.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub meta: Option<Meta>,
}

impl Source {
    /// The name of a streaming service in the library's links and in the settings; nothing for
    /// files and radio.
    pub fn key(&self) -> Option<String> {
        match self {
            Source::Server { account } => Some(format!("server:{account}")),
            Source::Spotify => Some("spotify".to_owned()),
            Source::Local | Source::Radio => None,
        }
    }

    pub fn from_key(key: &str) -> Option<Source> {
        match key.strip_prefix("server:") {
            Some(account) if !account.is_empty() => Some(Source::Server { account: account.to_owned() }),
            None if key == "spotify" => Some(Source::Spotify),
            _ => None,
        }
    }
}

/// What a song is on one service.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Link {
    /// The service's own id; empty when it was looked for and not found.
    pub id: String,
    /// The service's artwork id, for personal servers only.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub art: String,
    /// Chosen by the person, so a search never replaces it.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub chosen: bool,
    /// When it was found or looked for, in milliseconds since 1970.
    pub at: u64,
}

impl Item {
    pub fn local(id: &str) -> Item {
        Item { source: Source::Local, id: id.to_owned(), meta: None }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Meta {
    pub title: String,
    pub artist: String,
    pub album: String,
    pub duration_ms: u64,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub isrc: String,
    #[serde(skip_serializing_if = "is_zero")]
    pub year: u32,
    #[serde(skip_serializing_if = "is_zero")]
    pub track: u32,
    /// Artwork address or server artwork id.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub art: String,
}

// serde's skip_serializing_if passes a reference.
fn is_zero(n: &u32) -> bool {
    *n == 0
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Playlist {
    pub id: String,
    pub name: String,
    pub items: Vec<Item>,
    /// A smart playlist: its songs follow from the rules.
    pub smart: Option<crate::smart::Rules>,
    pub created: u64,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Station {
    pub id: String,
    pub name: String,
    pub url: String,
    pub detail: String,
}
