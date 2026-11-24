//! MusicCraft's library for the phone apps.
//!
//! The apps call one function, [`call`], with a request in JSON and get an answer in JSON:
//! `{"ok": …}` or `{"error": "…"}`. The library is the desktop's own catalogue
//! ([`musiccraft_library::Library::detached`]), so the phones read and write the shared folder exactly as
//! the computers do. The phone does the file work: it reads the catalogue files and passes their
//! text in, and writes the files that [`Request::Unsaved`] hands back. The requests are listed in
//! [`Request`]; `docs/mobile.md` says how an app uses them.
//!
//! Only the modules that make [`call`] reachable from Java (`android`) and from C and Swift (`c`)
//! use `unsafe`.
#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::unimplemented,
    clippy::todo,
    clippy::unreachable
)]

#[cfg(target_os = "android")]
mod android;
#[cfg(not(target_os = "android"))]
pub mod c;
mod session;

pub use session::{Request, Session};

use serde_json::{Value, json};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::{Mutex, PoisonError};

/// The open library. One per app process.
static SESSION: Mutex<Option<Session>> = Mutex::new(None);

/// Runs one request. Never panics: a request that cannot be read, or a fault inside, comes back
/// as `{"error": …}`.
pub fn call(input: &str) -> String {
    let answer = catch_unwind(AssertUnwindSafe(|| {
        let request: Request = match serde_json::from_str(input) {
            Ok(r) => r,
            Err(e) => return Err(format!("the request could not be read: {e}")),
        };
        let mut slot = SESSION.lock().unwrap_or_else(PoisonError::into_inner);
        session::run(&mut slot, request)
    }))
    .unwrap_or_else(|_| Err("MusicCraft's library stopped on an internal error".to_owned()));
    let value = match answer {
        Ok(v) => json!({ "ok": v }),
        Err(e) => json!({ "error": e }),
    };
    serde_json::to_string(&value).unwrap_or_else(|_| "{\"error\":\"the answer could not be written\"}".to_owned())
}

/// The answer to a request that has nothing to say.
fn done() -> Value {
    Value::Null
}

#[cfg(test)]
mod tests;
