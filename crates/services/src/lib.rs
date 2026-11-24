//! Streaming services, each through its public, documented interface.
//!
//! - [`subsonic`]: personal music servers that speak the Subsonic / OpenSubsonic API.
//! - [`radio`]: internet radio, with the Radio Browser community directory.
//! - [`spotify`]: the Spotify Web API with the person's own Client ID. MusicCraft does not decode
//!   Spotify's audio: it plays on a Spotify app, or on Spotify's own Soloist player ([`soloist`])
//!   when the person has installed it.
//!
//! Passwords, tokens and keys stay in the system's credential store ([`secrets`]), never in the
//! shared library folder.
#![forbid(unsafe_code)]
#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::unimplemented,
    clippy::todo,
    clippy::unreachable
)]

pub mod http;
pub mod radio;
pub mod secrets;
pub mod soloist;
pub mod spotify;
pub mod subsonic;

#[derive(Debug, Clone, thiserror::Error)]
pub enum ServiceError {
    #[error("the service cannot be reached: {0}")]
    Network(String),
    #[error("the service refused the request ({status}): {message}")]
    Refused { status: u16, message: String },
    #[error("the service asks to wait {0} seconds")]
    Wait(u64),
    #[error("the sign-in has expired: sign in again")]
    SignIn,
    #[error("the answer could not be understood: {0}")]
    Answer(String),
    #[error("the credential store of this computer is not available: {0}")]
    Secrets(String),
    #[error("{0}")]
    Other(String),
}

/// A song, album or playlist a service lists.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Entry {
    pub id: String,
    pub title: String,
    pub artist: String,
    pub album: String,
    pub duration_ms: u64,
    /// Artwork address or server artwork id.
    pub art: String,
    pub year: u32,
    pub track: u32,
    /// The recording's international code, when the service gives it.
    pub isrc: String,
    /// Number of songs, for albums and playlists.
    pub count: u32,
}
