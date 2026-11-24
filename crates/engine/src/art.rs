//! Album pictures, read and reduced in the background and kept in memory.

use crate::lock;
use musiccraft_services::http;
use musiccraft_services::subsonic::Server;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Condvar, Mutex, PoisonError};

/// Side of a kept picture, in pixels.
pub const SIDE: u32 = 320;
const KEEP: usize = 600;
const WORKERS: usize = 2;

/// Where a picture comes from.
#[derive(Clone)]
pub enum From {
    File(PathBuf),
    Server(Server, String),
    Web(String),
}

/// A picture as RGBA.
#[derive(Debug)]
pub struct Picture {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

enum Slot {
    Waiting,
    Done(Option<Arc<Picture>>),
}

#[derive(Default)]
struct Inner {
    slots: HashMap<String, (Slot, u64)>,
    /// Newest last; the newest is read first, because it is on screen.
    jobs: Vec<(String, From)>,
    clock: u64,
    started: bool,
    generation: u64,
}

#[derive(Clone, Default)]
pub struct Art(Arc<(Mutex<Inner>, Condvar)>);

impl Art {
    /// The picture for `key`, if it has been read; otherwise asks for it. `from` is called only
    /// the first time.
    pub fn get(&self, key: &str, from: impl FnOnce() -> Option<From>) -> Option<Arc<Picture>> {
        let mut inner = lock(&self.0.0);
        inner.clock += 1;
        let clock = inner.clock;
        if let Some((slot, used)) = inner.slots.get_mut(key) {
            *used = clock;
            return match slot {
                Slot::Done(p) => p.clone(),
                Slot::Waiting => None,
            };
        }
        let Some(from) = from() else {
            inner.slots.insert(key.to_owned(), (Slot::Done(None), clock));
            return None;
        };
        inner.slots.insert(key.to_owned(), (Slot::Waiting, clock));
        inner.jobs.push((key.to_owned(), from));
        if !inner.started {
            inner.started = true;
            for _ in 0..WORKERS {
                let art = self.clone();
                // Without a thread there are no pictures, and nothing else changes.
                crate::spawn("artwork", move || art.work()).ok();
            }
        }
        drop(inner);
        self.0.1.notify_one();
        None
    }

    /// Forgets one picture, after its song was edited.
    pub fn forget(&self, key: &str) {
        lock(&self.0.0).slots.remove(key);
    }

    /// Grows when a picture arrives.
    pub fn generation(&self) -> u64 {
        lock(&self.0.0).generation
    }

    pub fn is_busy(&self) -> bool {
        lock(&self.0.0).slots.values().any(|(s, _)| matches!(s, Slot::Waiting))
    }

    fn work(&self) {
        loop {
            let (key, from) = {
                let mut inner = lock(&self.0.0);
                loop {
                    match inner.jobs.pop() {
                        Some(j) => break j,
                        None => inner = self.0.1.wait(inner).unwrap_or_else(PoisonError::into_inner),
                    }
                }
            };
            let picture = load(&from).map(Arc::new);
            let mut inner = lock(&self.0.0);
            let clock = inner.clock;
            inner.slots.insert(key, (Slot::Done(picture), clock));
            inner.generation += 1;
            if inner.slots.len() > KEEP {
                let mut ages: Vec<u64> = inner.slots.values().map(|(_, u)| *u).collect();
                ages.sort_unstable();
                let limit = ages.get(KEEP / 4).copied().unwrap_or(0);
                inner.slots.retain(|_, (s, u)| *u > limit || matches!(s, Slot::Waiting));
            }
        }
    }
}

fn load(from: &From) -> Option<Picture> {
    let bytes = match from {
        From::File(p) => {
            let meta = std::fs::metadata(p).ok()?;
            if musiccraft_library::cloud::online_only(&meta) {
                return None;
            }
            musiccraft_library::tags::picture(p)?
        }
        From::Server(s, id) => s.cover(id).ok()?,
        From::Web(url) => {
            if !url.starts_with("https://") {
                return None;
            }
            http::bytes(http::get(url).ok()?).ok()?
        }
    };
    decode(&bytes)
}

/// Decodes a PNG or JPEG and reduces it. Sizes are limited before any pixel is allocated.
pub fn decode(bytes: &[u8]) -> Option<Picture> {
    let mut reader = image::ImageReader::new(std::io::Cursor::new(bytes)).with_guessed_format().ok()?;
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(12_000);
    limits.max_image_height = Some(12_000);
    limits.max_alloc = Some(512 * 1024 * 1024);
    reader.limits(limits);
    let img = reader.decode().ok()?;
    let img = if img.width() > SIDE || img.height() > SIDE { img.thumbnail(SIDE, SIDE) } else { img };
    let rgba = img.to_rgba8();
    Some(Picture { width: rgba.width(), height: rgba.height(), rgba: rgba.into_raw() })
}
