//! The MusicCraft window. The layout follows the conventions music-library programs share: a
//! header with the controls and what is playing, a sidebar, and the songs. Every shape, icon
//! and colour is drawn here.
#![forbid(unsafe_code)]
#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::unimplemented,
    clippy::todo,
    clippy::unreachable
)]

mod albums;
mod app;
mod chooser;
mod dialogs;
mod fonts;
mod header;
mod i18n;
mod icons;
mod menus;
mod services;
mod sidebar;
mod table;
mod theme;
mod views;

pub use app::{App, Options, View};
