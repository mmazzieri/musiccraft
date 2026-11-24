//! MusicCraft without its window: settings, the play queue, background work and the commands.
//!
//! Every change goes through [`Command`], so the window, the command line and a script can do
//! the same things.
#![forbid(unsafe_code)]
#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::unimplemented,
    clippy::todo,
    clippy::unreachable
)]

pub mod art;
pub mod command;
pub mod core;
pub mod demo;
pub mod pages;
pub mod queue;
pub mod remote;
pub mod settings;

pub use command::Command;
pub use core::{Engine, Now, NowStatus};
pub use queue::{Queue, Repeat};
pub use settings::{ServerAccount, Settings, Theme};

use std::sync::{Mutex, MutexGuard, PoisonError};

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Runs `work` on a thread of its own.
fn spawn(name: &str, work: impl FnOnce() + Send + 'static) -> Result<(), String> {
    std::thread::Builder::new().name(name.to_owned()).spawn(work).map(|_| ()).map_err(|e| e.to_string())
}
