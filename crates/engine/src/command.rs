//! Everything MusicCraft can do, as data.

use crate::queue::Repeat;
use musiccraft_library::Item;
use musiccraft_library::query::Sort;
use musiccraft_library::smart::Rules;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// New tag values; fields left out stay as they are.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct TagEdit {
    pub title: Option<String>,
    pub artist: Option<String>,
    pub album_artist: Option<String>,
    pub album: Option<String>,
    pub genre: Option<String>,
    pub composer: Option<String>,
    pub year: Option<u32>,
    pub track: Option<u32>,
    pub disc: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "cmd", rename_all = "snake_case")]
pub enum Command {
    // Playing
    Play {
        items: Vec<Item>,
        #[serde(default)]
        start: usize,
    },
    PlayPause,
    Stop,
    Next,
    Previous,
    Seek {
        ms: u64,
    },
    Volume {
        percent: u8,
    },
    Shuffle {
        on: bool,
    },
    /// Plays songs louder or softer, from the loudness their tags give, so that they sound equally
    /// loud; at once, for the song playing too.
    EvenVolume {
        on: bool,
    },
    Repeat {
        mode: Repeat,
    },
    QueueNext {
        items: Vec<Item>,
    },
    QueueAdd {
        items: Vec<Item>,
    },
    QueueRemove {
        index: usize,
    },
    QueueShift {
        index: usize,
        later: bool,
    },
    QueueGo {
        index: usize,
    },
    QueueClear,
    // Songs
    Love {
        ids: Vec<String>,
        on: bool,
    },
    Rate {
        ids: Vec<String>,
        stars: u8,
    },
    Edit {
        ids: Vec<String>,
        tags: TagEdit,
    },
    /// Writes a song's lyrics into its file; empty text takes them away.
    LyricsSet {
        id: String,
        text: String,
    },
    Remove {
        ids: Vec<String>,
    },
    RestoreRemoved,
    Import {
        paths: Vec<PathBuf>,
    },
    Scan,
    /// Keeps songs of a service in the library. They stay when the service changes.
    LibraryAdd {
        items: Vec<Item>,
    },
    /// Puts a song without a file in the library.
    SongAdd {
        title: String,
        artist: String,
        #[serde(default)]
        album: String,
    },
    /// Puts the songs of a list (CSV) in the library, with its playlists.
    SongsImport {
        path: PathBuf,
    },
    /// Reads the library another program exported (XML), to see what is in it.
    ExportedRead {
        path: PathBuf,
    },
    /// Takes in the library another program exported: songs, history and playlists. Without `copy`
    /// the songs are added without their files.
    ExportedImport {
        path: PathBuf,
        copy: bool,
    },
    ImportStop,
    /// The service that plays songs without a file; empty for none.
    ActiveService {
        service: String,
    },
    /// Says by hand what a song is on a service: the service's id, or a page of it.
    LinkSet {
        id: String,
        service: String,
        to: String,
        #[serde(default)]
        art: String,
    },
    /// Forgets what a song is on a service, to look for it again.
    LinkClear {
        ids: Vec<String>,
        service: String,
    },
    /// Looks for every song without a file on the service in use.
    FindAll,
    FindStop,
    // Playlists
    PlaylistNew {
        name: String,
        #[serde(default)]
        items: Vec<Item>,
    },
    SmartNew {
        name: String,
        rules: Rules,
    },
    SmartSet {
        id: String,
        rules: Rules,
    },
    PlaylistRename {
        id: String,
        name: String,
    },
    PlaylistDelete {
        id: String,
    },
    PlaylistAdd {
        id: String,
        items: Vec<Item>,
    },
    PlaylistSet {
        id: String,
        items: Vec<Item>,
    },
    PlaylistImport {
        path: PathBuf,
    },
    PlaylistExport {
        id: String,
        path: PathBuf,
    },
    // Radio
    StationAdd {
        name: String,
        url: String,
        #[serde(default)]
        detail: String,
    },
    StationRemove {
        id: String,
    },
    // Accounts
    ServerAdd {
        name: String,
        url: String,
        user: String,
        password: String,
    },
    ServerRemove {
        name: String,
    },
    SpotifySignIn {
        client_id: String,
    },
    SpotifySignOut,
    SpotifyDevice {
        id: String,
    },
    SoloistStart {
        path: String,
        /// Empty uses the key kept in the credential store.
        key: String,
    },
    SoloistStop,
    Refresh,
    // Library
    LibraryOpen {
        path: PathBuf,
    },
    // Questions
    Songs {
        #[serde(default)]
        search: String,
        #[serde(default)]
        sort: Sort,
        #[serde(default)]
        descending: bool,
        #[serde(default)]
        limit: usize,
    },
    Albums,
    Playlists,
    PlaylistSongs {
        id: String,
    },
    /// The lyrics inside a song's file.
    Lyrics {
        id: String,
    },
    Stations,
    Services,
    Now,
    Info,
}
