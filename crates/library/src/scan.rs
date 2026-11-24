//! Looks for music files below `Music/`. Reads the tags only of files that are new or changed
//! since the last scan on this computer, and never of files that are still only in the cloud.

use crate::model::{Info, Presence};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::SystemTime;
use unicode_normalization::UnicodeNormalization;

const MAX_DEPTH: usize = 24;

/// Size and time of each file at its last reading, kept on this computer only.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Cache(HashMap<String, (u64, u64)>);

#[derive(Debug, Default)]
pub struct ScanResult {
    /// Every file found.
    pub presence: HashMap<String, Presence>,
    /// New or changed records.
    pub read: Vec<Info>,
    /// Files whose name on disk is not in the composed Unicode form the library uses.
    pub aliases: HashMap<String, PathBuf>,
    pub cache: Cache,
    /// Files that could not be read, with the reason.
    pub failed: Vec<(String, String)>,
}

/// Lets another thread watch a scan and stop it.
#[derive(Debug, Default)]
pub struct Progress {
    pub found: AtomicUsize,
    pub read: AtomicUsize,
    pub stop: AtomicBool,
}

struct Found {
    rel: String,
    path: PathBuf,
    size: u64,
    time: u64,
    cloud: bool,
}

/// Scans `music`. `known` holds the library's records by path. Returns nothing when the folder
/// cannot be listed, so an unplugged or unmounted folder never looks like an empty one.
pub fn run(music: &Path, known: &HashMap<String, Info>, cache: &Cache, progress: &Progress) -> Option<ScanResult> {
    let mut files = Vec::new();
    let mut out = ScanResult::default();
    std::fs::read_dir(music).ok()?;
    walk(music, "", 0, &mut files, &mut out, progress);
    for f in files {
        if progress.stop.load(Ordering::Relaxed) {
            return None;
        }
        out.presence.insert(f.rel.clone(), if f.cloud { Presence::Cloud } else { Presence::Here });
        let old = known.get(&f.rel);
        if f.cloud {
            if old.is_none() {
                let mut info = crate::tags::from_path(&f.rel);
                info.size = f.size;
                out.read.push(info);
            }
        } else if old.is_some_and(|o| !o.guessed) && cache.0.get(&f.rel) == Some(&(f.size, f.time)) {
            out.cache.0.insert(f.rel.clone(), (f.size, f.time));
        } else {
            match crate::tags::read(&f.path, &f.rel) {
                Ok(info) => {
                    out.cache.0.insert(f.rel.clone(), (f.size, f.time));
                    out.read.push(info);
                }
                Err(e) => {
                    out.presence.remove(&f.rel);
                    out.failed.push((f.rel.clone(), e));
                }
            }
            progress.read.fetch_add(1, Ordering::Relaxed);
        }
        let composed = crate::store::resolve(music.parent().unwrap_or(music), &f.rel);
        if composed.as_ref() != Some(&f.path) {
            out.aliases.insert(f.rel, f.path);
        }
    }
    Some(out)
}

fn walk(dir: &Path, rel: &str, depth: usize, files: &mut Vec<Found>, out: &mut ScanResult, progress: &Progress) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        out.failed.push((rel.to_owned(), "the folder cannot be read".to_owned()));
        return;
    };
    for entry in entries.flatten() {
        if progress.stop.load(Ordering::Relaxed) {
            return;
        }
        let Ok(name) = entry.file_name().into_string() else { continue };
        if name.starts_with('.') {
            continue;
        }
        let Ok(meta) = entry.metadata() else { continue };
        if meta.is_symlink() {
            continue;
        }
        let composed: String = name.nfc().collect();
        let child = if rel.is_empty() { composed } else { format!("{rel}/{composed}") };
        if meta.is_dir() {
            if depth < MAX_DEPTH {
                walk(&entry.path(), &child, depth + 1, files, out, progress);
            }
        } else if crate::is_audio(&name) {
            let time = meta
                .modified()
                .ok()
                .and_then(|t| t.duration_since(SystemTime::UNIX_EPOCH).ok())
                .map_or(0, |d| d.as_secs());
            progress.found.fetch_add(1, Ordering::Relaxed);
            files.push(Found {
                rel: child,
                path: entry.path(),
                size: meta.len(),
                time,
                cloud: crate::cloud::online_only(&meta),
            });
        }
    }
}
