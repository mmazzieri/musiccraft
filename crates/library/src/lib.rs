//! The MusicCraft library: a folder the person chooses, with their music files and a
//! catalogue that several computers can share through any folder-sync service.
//!
//! ```text
//! MusicCraft/
//!   Music/<Album artist>/<Album>/01 Title.mp3     the music files
//!   Library/<device>.tracks.json                  one set of files per computer
//!   Library/<device>.activity.json
//!   Library/<device>.playlists.json
//! ```
//!
//! Each computer writes only its own files, so a sync service never sees two writers of one
//! file. What the person sees is the merge of all of them: for each field the newest write
//! wins, and play counts add up (see [`store`]).
#![forbid(unsafe_code)]
#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::unimplemented,
    clippy::todo,
    clippy::unreachable
)]

pub mod cloud;
pub mod import;
pub mod m3u;
pub mod model;
pub mod query;
pub mod scan;
pub mod smart;
pub mod store;
pub mod tags;

pub use model::{Item, Meta, Playlist, Source, Station, Track};
pub use store::{Library, LibraryError};

use std::path::PathBuf;

/// File name extensions MusicCraft can play.
pub const AUDIO_EXTENSIONS: &[&str] = &["mp3", "m4a", "aac", "flac", "ogg", "oga", "wav", "aif", "aiff", "caf"];

/// The folder a new library goes in: `MusicCraft` inside the system's music folder, wherever
/// the person or a sync service has put that folder.
pub fn default_folder() -> Option<PathBuf> {
    dirs::audio_dir().or_else(|| dirs::home_dir().map(|h| h.join("Music"))).map(|d| d.join("MusicCraft"))
}

pub fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX))
        .unwrap_or(0)
}

/// Whether the name ends in an extension MusicCraft can play.
pub fn is_audio(name: &str) -> bool {
    name.rsplit_once('.').is_some_and(|(_, e)| AUDIO_EXTENSIONS.iter().any(|a| a.eq_ignore_ascii_case(e)))
}
