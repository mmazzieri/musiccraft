//! The shared catalogue: one set of files per computer, merged on reading.
//!
//! Every fact is a cell: a field of a record with the time it was written. A computer stores only
//! the cells it wrote itself, in its own files, and the merge keeps the newest cell of each field
//! (ties go to the greater device id, so every computer agrees). Play counts are kept per device
//! and added up, so two computers playing at once lose nothing. Reading is idempotent: a conflict
//! copy made by a sync service holds the same kind of cells and merges like any other file.

use crate::model::{Info, Item, Link, Playlist, Presence, Station, Track};
use crate::names::song_key;
use crate::{now_ms, scan::ScanResult, smart::Rules};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

pub const FORMAT: &str = "musiccraft-library";
pub const FORMAT_VERSION: u32 = 1;
/// A catalogue file larger than this is refused.
const MAX_FILE: u64 = 256 * 1024 * 1024;
/// A clock this far ahead of ours is not followed when stamping new cells.
const MAX_AHEAD_MS: u64 = 10 * 60 * 1000;

#[derive(Debug, thiserror::Error)]
pub enum LibraryError {
    #[error("{path}: {source}")]
    Io { path: PathBuf, source: std::io::Error },
    #[error("there is no such song in the library")]
    NoTrack,
    #[error("there is no such playlist in the library")]
    NoPlaylist,
    #[error("a smart playlist's songs follow from its rules")]
    SmartItems,
    #[error("the song's file is not on this computer")]
    NotHere,
    #[error("the song has no file")]
    NoFile,
    #[error("the tags could not be written: {0}")]
    Tags(String),
    #[error("{0}")]
    Other(String),
}

fn io(path: &Path) -> impl FnOnce(std::io::Error) -> LibraryError + '_ {
    move |source| LibraryError::Io { path: path.to_owned(), source }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
enum Section {
    Tracks,
    Activity,
    Playlists,
}

const SECTIONS: [Section; 3] = [Section::Tracks, Section::Activity, Section::Playlists];

impl Section {
    fn name(self) -> &'static str {
        match self {
            Section::Tracks => "tracks",
            Section::Activity => "activity",
            Section::Playlists => "playlists",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Cell {
    v: Value,
    t: u64,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
struct Counter {
    plays: u64,
    skips: u64,
    last: u64,
}

type Records = BTreeMap<String, BTreeMap<String, Cell>>;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct FileData {
    format: String,
    version: u32,
    section: Section,
    device: String,
    #[serde(default)]
    device_name: String,
    #[serde(default)]
    records: Records,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    counters: BTreeMap<String, Counter>,
}

#[derive(Debug, Clone)]
struct Merged {
    v: Value,
    t: u64,
    device: String,
}

/// What another program knew about a song.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct History {
    /// Milliseconds since 1970; 0 when it is not known. So is `last_played`.
    pub added: u64,
    pub rating: u8,
    pub loved: bool,
    pub plays: u64,
    pub skips: u64,
    pub last_played: u64,
}

/// An open library.
pub struct Library {
    root: PathBuf,
    local: PathBuf,
    device: String,
    device_name: String,
    own: Vec<FileData>,
    dirty: [bool; 3],
    merged: BTreeMap<String, BTreeMap<String, Merged>>,
    /// Counters by track, then device.
    counts: BTreeMap<String, BTreeMap<String, Counter>>,
    tracks: BTreeMap<String, Track>,
    playlists: Vec<Playlist>,
    stations: Vec<Station>,
    /// Name, size and time of every catalogue file at the last read.
    seen: BTreeMap<PathBuf, (u64, Option<SystemTime>)>,
    presence: HashMap<String, Presence>,
    aliases: HashMap<String, PathBuf>,
    scanned: bool,
    warnings: Vec<String>,
    clock: u64,
    /// Grows with every change to what the library shows.
    revision: u64,
}

impl Library {
    /// Opens the library in `root`, creating its folders. `local` is this computer's own data
    /// folder, outside the shared one; it keeps a copy of this device's files.
    pub fn open(root: &Path, local: &Path, device: &str, device_name: &str) -> Result<Library, LibraryError> {
        for dir in [root.join("Music"), root.join("Library"), local.to_owned()] {
            std::fs::create_dir_all(&dir).map_err(io(&dir))?;
        }
        let mut lib = Library::blank(root, local, device, device_name);
        lib.load_own();
        lib.reload();
        Ok(lib)
    }

    /// A library without a folder: its catalogue files go in and out as bytes, through
    /// [`Library::load_bytes`], [`Library::unsaved`] and [`Library::mark_saved`]. The phone apps
    /// use it (`crates/ffi`); their own code reads and writes the shared folder. Nothing here
    /// touches a file: the methods that would ([`Library::save`], [`Library::refresh`],
    /// [`Library::edit`], [`Library::file`]) are not for it.
    pub fn detached(device: &str, device_name: &str) -> Library {
        Library::blank(Path::new(""), Path::new(""), device, device_name)
    }

    fn blank(root: &Path, local: &Path, device: &str, device_name: &str) -> Library {
        Library {
            root: root.to_owned(),
            local: local.to_owned(),
            device: device.to_owned(),
            device_name: device_name.to_owned(),
            own: SECTIONS.map(|s| empty(s, device, device_name)).to_vec(),
            dirty: [false; 3],
            merged: BTreeMap::new(),
            counts: BTreeMap::new(),
            tracks: BTreeMap::new(),
            playlists: Vec::new(),
            stations: Vec::new(),
            seen: BTreeMap::new(),
            presence: HashMap::new(),
            aliases: HashMap::new(),
            scanned: false,
            warnings: Vec::new(),
            clock: 0,
            revision: 0,
        }
    }

    /// The names of this computer's three files in `Library/`.
    pub fn own_names(&self) -> Vec<String> {
        SECTIONS.iter().map(|s| format!("{}.{}.json", self.device, s.name())).collect()
    }

    /// Reads a catalogue from bytes, as [`Library::open`] reads it from a folder: `files` are the
    /// `*.json` files of `Library/`, `backups` this computer's own copy of its files. This
    /// computer's file is taken from the shared folder when it can be read, else from the copy.
    /// Gives back the names of this computer's files that are damaged: they must be set aside
    /// (renamed) before they are written again.
    pub fn load_bytes(&mut self, files: &[(String, Vec<u8>)], backups: &[(String, Vec<u8>)]) -> Vec<String> {
        self.warnings.clear();
        let mut set_aside = Vec::new();
        let names = self.own_names();
        for (i, (section, name)) in SECTIONS.into_iter().zip(&names).enumerate() {
            let backup = || backups.iter().find(|(n, _)| n == name).and_then(|(_, b)| parse(b).ok());
            let (data, dirty) = match files.iter().find(|(n, _)| n == name).map(|(_, b)| parse(b)) {
                Some(Ok(data)) => (Some(data), false),
                Some(Err(e)) => {
                    // Damaged, or half synced: set aside by the caller instead of written over.
                    self.warnings.push(format!("{name}: {e}"));
                    set_aside.push(name.clone());
                    (backup(), false)
                }
                None => (backup(), true),
            };
            let slot = self.own.get_mut(i);
            match (data.filter(|d| d.section == section && d.device == self.device), slot) {
                (Some(data), Some(slot)) => {
                    *slot = data;
                    slot.device_name.clone_from(&self.device_name);
                    if let Some(d) = self.dirty.get_mut(i) {
                        *d = dirty;
                    }
                }
                (None, Some(slot)) => *slot = empty(section, &self.device, &self.device_name),
                _ => {}
            }
        }
        self.merged.clear();
        self.counts.clear();
        for data in self.own.clone() {
            self.absorb(data);
        }
        for (name, bytes) in files.iter().filter(|(n, _)| n.ends_with(".json") && !names.contains(n)) {
            match parse(bytes) {
                Ok(data) => self.absorb(data),
                Err(e) => self.warnings.push(format!("{name}: {e}")),
            }
        }
        self.rebuild();
        set_aside
    }

    /// This computer's files that changed since they were written, by name, with their bytes.
    pub fn unsaved(&self) -> Result<Vec<(String, Vec<u8>)>, LibraryError> {
        let mut out = Vec::new();
        for (i, name) in self.own_names().into_iter().enumerate() {
            if self.dirty.get(i).copied().unwrap_or(false)
                && let Some(data) = self.own.get(i)
            {
                out.push((name, serde_json::to_vec(data).map_err(|e| LibraryError::Other(e.to_string()))?));
            }
        }
        Ok(out)
    }

    /// Notes that one of this computer's files, named as [`Library::unsaved`] names it, was written.
    pub fn mark_saved(&mut self, name: &str) {
        if let Some(i) = self.own_names().iter().position(|n| n == name)
            && let Some(d) = self.dirty.get_mut(i)
        {
            *d = false;
        }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn music_dir(&self) -> PathBuf {
        self.root.join("Music")
    }

    fn own_path(&self, section: Section) -> PathBuf {
        self.root.join("Library").join(format!("{}.{}.json", self.device, section.name()))
    }

    fn backup_path(&self, section: Section) -> PathBuf {
        self.local.join(format!("{}.{}.json", self.device, section.name()))
    }

    fn load_own(&mut self) {
        for (i, section) in SECTIONS.into_iter().enumerate() {
            let path = self.own_path(section);
            let read = if path.exists() { Some(read_file(&path)) } else { None };
            let kept = self.backup_path(section);
            let backup = || read_file(&kept).ok();
            let data = match read {
                Some(Ok(data)) => Some(data),
                Some(Err(e)) => {
                    // Damaged, or half synced: set it aside instead of writing over it.
                    let aside = path.with_extension(format!("damaged-{}", now_ms()));
                    let moved = std::fs::rename(&path, &aside).is_ok();
                    self.warnings.push(format!("{}: {e}", path.display()));
                    if moved { backup() } else { None }
                }
                None => backup(),
            };
            if let Some(data) = data.filter(|d| d.section == section && d.device == self.device)
                && let missing = !path.exists()
                && let Some(slot) = self.own.get_mut(i)
            {
                *slot = data;
                slot.device_name.clone_from(&self.device_name);
                if let Some(d) = self.dirty.get_mut(i) {
                    *d = missing;
                }
            }
        }
    }

    /// Reads the other computers' files again if any changed. Returns whether anything did.
    pub fn refresh(&mut self) -> bool {
        if self.list_files() == self.seen {
            return false;
        }
        self.reload();
        true
    }

    fn list_files(&self) -> BTreeMap<PathBuf, (u64, Option<SystemTime>)> {
        let mut out = BTreeMap::new();
        let own: HashSet<PathBuf> = SECTIONS.iter().map(|s| self.own_path(*s)).collect();
        let Ok(dir) = std::fs::read_dir(self.root.join("Library")) else { return out };
        for entry in dir.flatten() {
            let path = entry.path();
            if path.extension().is_some_and(|e| e == "json")
                && !own.contains(&path)
                && let Ok(meta) = entry.metadata()
            {
                out.insert(path, (meta.len(), meta.modified().ok()));
            }
        }
        out
    }

    fn reload(&mut self) {
        let files = self.list_files();
        self.merged.clear();
        self.counts.clear();
        self.warnings.retain(|w| w.contains("damaged"));
        for data in self.own.clone() {
            self.absorb(data);
        }
        for path in files.keys() {
            match read_file(path) {
                Ok(data) => self.absorb(data),
                Err(e) => self.warnings.push(format!("{}: {e}", path.display())),
            }
        }
        self.seen = files;
        self.rebuild();
    }

    fn absorb(&mut self, data: FileData) {
        for (key, fields) in data.records {
            for (field, cell) in fields {
                self.put(&key, &field, Merged { v: cell.v, t: cell.t, device: data.device.clone() });
            }
        }
        for (id, c) in data.counters {
            let slot = self.counts.entry(id).or_default().entry(data.device.clone()).or_default();
            slot.plays = slot.plays.max(c.plays);
            slot.skips = slot.skips.max(c.skips);
            slot.last = slot.last.max(c.last);
        }
    }

    fn put(&mut self, key: &str, field: &str, new: Merged) {
        if new.t <= now_ms().saturating_add(MAX_AHEAD_MS) {
            self.clock = self.clock.max(new.t);
        }
        let record = self.merged.entry(key.to_owned()).or_default();
        match record.get(field) {
            Some(old) if (old.t, &old.device) >= (new.t, &new.device) => {}
            _ => {
                record.insert(field.to_owned(), new);
            }
        }
    }

    /// Writes one cell of ours.
    fn set(&mut self, section: Section, key: &str, field: &str, v: Value) {
        let newest = self.merged.get(key).and_then(|r| r.get(field)).map_or(0, |m| m.t);
        // Newer than what it replaces, even when another computer's clock is ahead.
        let t = now_ms().max(self.clock.saturating_add(1)).max(newest.saturating_add(1));
        self.clock = t;
        let i = section as usize;
        if let Some(own) = self.own.get_mut(i) {
            own.records.entry(key.to_owned()).or_default().insert(field.to_owned(), Cell { v: v.clone(), t });
        }
        if let Some(d) = self.dirty.get_mut(i) {
            *d = true;
        }
        let device = self.device.clone();
        self.put(key, field, Merged { v, t, device });
    }

    fn field<T: serde::de::DeserializeOwned>(&self, key: &str, field: &str) -> Option<T> {
        let v = self.merged.get(key)?.get(field)?;
        serde_json::from_value(v.v.clone()).ok()
    }

    fn build_track(&self, id: &str) -> Option<Track> {
        let key = format!("track/{id}");
        if self.field::<bool>(&key, "removed").unwrap_or(false) {
            return None;
        }
        let info: Info = self.field(&key, "info")?;
        if info.path.is_empty() {
            if info.title.is_empty() {
                return None;
            }
        } else {
            resolve(&self.root, &info.path)?;
        }
        let mut t = Track {
            id: id.to_owned(),
            added: self.field(&key, "added").unwrap_or(0),
            loved: self.field(&key, "loved").unwrap_or(false),
            rating: self.field::<u8>(&key, "rating").unwrap_or(0).min(5),
            links: self.links(&key),
            presence: match self.presence.get(&info.path) {
                _ if info.path.is_empty() => Presence::Stream,
                Some(p) => *p,
                None if self.scanned => Presence::Missing,
                None => Presence::Here,
            },
            info,
            ..Track::default()
        };
        // Counts from another program are one field, so importing them again adds nothing.
        if let Some(c) = self.field::<Counter>(&key, "imported") {
            t.plays = c.plays;
            t.skips = c.skips;
            t.last_played = c.last;
        }
        if let Some(by_device) = self.counts.get(id) {
            for c in by_device.values() {
                t.plays = t.plays.saturating_add(c.plays);
                t.skips = t.skips.saturating_add(c.skips);
                t.last_played = t.last_played.max(c.last);
            }
        }
        Some(t)
    }

    fn links(&self, key: &str) -> BTreeMap<String, Link> {
        let Some(record) = self.merged.get(key) else { return BTreeMap::new() };
        record
            .iter()
            .filter_map(|(field, cell)| {
                let service = field.strip_prefix("link:")?;
                Some((service.to_owned(), serde_json::from_value(cell.v.clone()).ok()?))
            })
            .collect()
    }

    /// Says what a song is on a service.
    pub fn set_link(&mut self, id: &str, service: &str, link: &Link) -> Result<(), LibraryError> {
        let v = serde_json::to_value(link).unwrap_or(Value::Null);
        self.write_link(id, service, v)
    }

    /// Forgets what a song is on a service.
    pub fn clear_link(&mut self, id: &str, service: &str) -> Result<(), LibraryError> {
        self.write_link(id, service, Value::Null)
    }

    fn write_link(&mut self, id: &str, service: &str, v: Value) -> Result<(), LibraryError> {
        if !self.tracks.contains_key(id) {
            return Err(LibraryError::NoTrack);
        }
        self.set(Section::Tracks, &format!("track/{id}"), &format!("link:{service}"), v);
        self.touch(id);
        Ok(())
    }

    /// Forgets everything that came from a service, when its account goes.
    pub fn clear_service(&mut self, service: &str) {
        let ids: Vec<String> =
            self.tracks.values().filter(|t| t.links.contains_key(service)).map(|t| t.id.clone()).collect();
        for id in ids {
            self.set(Section::Tracks, &format!("track/{id}"), &format!("link:{service}"), Value::Null);
        }
        self.rebuild();
    }

    /// Puts songs without a file in the library, and says what each is on a service. A song that is
    /// already there, with or without a file, is not added twice. Gives back the songs' ids, in
    /// order, and how many are new.
    pub fn add_songs(&mut self, songs: Vec<(Info, Option<(String, Link)>)>) -> (Vec<String>, usize) {
        let mut by_key: HashMap<String, String> = HashMap::new();
        let mut by_isrc: HashMap<String, String> = HashMap::new();
        for t in self.tracks.values() {
            by_key.insert(song_key(&t.info), t.id.clone());
            if !t.info.isrc.is_empty() {
                by_isrc.insert(t.info.isrc.to_ascii_uppercase(), t.id.clone());
            }
        }
        let now = now_ms();
        let (mut ids, mut added) = (Vec::new(), 0);
        for (mut info, link) in songs {
            info.path.clear();
            let key = song_key(&info);
            let isrc = info.isrc.to_ascii_uppercase();
            let known = by_isrc.get(&isrc).filter(|_| !isrc.is_empty()).or_else(|| by_key.get(&key)).cloned();
            let id = known.unwrap_or_else(|| {
                // From the names, so two computers that add it at once give it the same id.
                let id: String = Sha256::digest(format!("song\0{key}").as_bytes())
                    .iter()
                    .take(16)
                    .map(|b| format!("{b:02x}"))
                    .collect();
                let record = format!("track/{id}");
                added += 1;
                if self.field::<bool>(&record, "removed").unwrap_or(false) {
                    self.set(Section::Activity, &record, "removed", Value::from(false));
                }
                // Back after a removal, with the file it may have had since.
                if self.field::<Info>(&record, "info").is_none() {
                    let v = serde_json::to_value(&info).unwrap_or(Value::Null);
                    self.set(Section::Tracks, &record, "info", v);
                    self.set(Section::Tracks, &record, "added", Value::from(now));
                }
                by_key.insert(key, id.clone());
                if !isrc.is_empty() {
                    by_isrc.insert(isrc, id.clone());
                }
                id
            });
            if let Some((service, link)) = link {
                let field = format!("link:{service}");
                let record = format!("track/{id}");
                let old: Option<Link> = self.field(&record, &field);
                if old.is_none_or(|o| !o.chosen && o.id != link.id) {
                    self.set(Section::Tracks, &record, &field, serde_json::to_value(link).unwrap_or(Value::Null));
                }
            }
            ids.push(id);
        }
        self.rebuild();
        (ids, added)
    }

    fn rebuild(&mut self) {
        let ids: Vec<String> = self.merged.keys().filter_map(|k| k.strip_prefix("track/")).map(str::to_owned).collect();
        self.tracks = ids.into_iter().filter_map(|id| Some((id.clone(), self.build_track(&id)?))).collect();
        self.rebuild_lists();
    }

    fn rebuild_lists(&mut self) {
        let mut playlists = Vec::new();
        let mut stations = Vec::new();
        for key in self.merged.keys() {
            if self.field::<bool>(key, "removed").unwrap_or(false) {
                continue;
            }
            if let Some(id) = key.strip_prefix("playlist/") {
                let Some(name) = self.field::<String>(key, "name") else { continue };
                playlists.push(Playlist {
                    id: id.to_owned(),
                    name,
                    items: self.field(key, "items").unwrap_or_default(),
                    smart: self.field::<Option<Rules>>(key, "smart").flatten(),
                    created: self.field(key, "created").unwrap_or(0),
                });
            } else if let Some(id) = key.strip_prefix("station/")
                && let Some(mut s) = self.field::<Station>(key, "station")
            {
                s.id = id.to_owned();
                stations.push(s);
            }
        }
        playlists.sort_by_cached_key(|p| (crate::model::fold(&p.name), p.id.clone()));
        stations.sort_by_cached_key(|s| (crate::model::fold(&s.name), s.id.clone()));
        self.playlists = playlists;
        self.stations = stations;
        self.revision += 1;
    }

    fn touch(&mut self, id: &str) {
        match self.build_track(id) {
            Some(t) => {
                self.tracks.insert(id.to_owned(), t);
            }
            None => {
                self.tracks.remove(id);
            }
        }
        self.revision += 1;
    }

    /// Writes this computer's changed files. The shared copy is replaced whole, so another
    /// program never sees half of it.
    pub fn save(&mut self) -> Result<(), LibraryError> {
        for (i, section) in SECTIONS.into_iter().enumerate() {
            if !self.dirty.get(i).copied().unwrap_or(false) {
                continue;
            }
            let Some(data) = self.own.get(i) else { continue };
            let bytes = serde_json::to_vec(data).map_err(|e| LibraryError::Other(e.to_string()))?;
            // A file this large could not be read again: the one that is there, and its copy, stay.
            if u64::try_from(bytes.len()).is_ok_and(|n| n > MAX_FILE) {
                return Err(LibraryError::Other("the catalogue is too large to be saved".to_owned()));
            }
            let path = self.own_path(section);
            craft_atomic::atomic_write(&path, &bytes).map_err(io(&path))?;
            let backup = self.backup_path(section);
            if let Err(e) = craft_atomic::atomic_write(&backup, &bytes) {
                self.warnings.push(format!("{}: {e}", backup.display()));
            }
            if let Some(d) = self.dirty.get_mut(i) {
                *d = false;
            }
        }
        Ok(())
    }

    pub fn is_dirty(&self) -> bool {
        self.dirty.contains(&true)
    }

    pub fn revision(&self) -> u64 {
        self.revision
    }

    /// Files that could not be read, for the person to see.
    pub fn warnings(&self) -> &[String] {
        &self.warnings
    }

    /// The names of the computers that share this library, this one included.
    pub fn devices(&self) -> Vec<String> {
        let mut names: BTreeMap<String, String> = BTreeMap::new();
        names.insert(self.device.clone(), self.device_name.clone());
        for path in self.seen.keys() {
            if let Ok(d) = read_file(path) {
                names.entry(d.device).or_insert(d.device_name);
            }
        }
        names.into_values().collect()
    }

    /// How many computers have written to this library, this one included.
    pub fn devices_known(&self) -> usize {
        let mut ids: HashSet<&str> = HashSet::from([self.device.as_str()]);
        for path in self.seen.keys() {
            if let Some(id) = path.file_name().and_then(|n| n.to_str()).and_then(|n| n.split('.').next()) {
                ids.insert(id);
            }
        }
        ids.len()
    }

    // ---- songs ----

    pub fn tracks(&self) -> impl Iterator<Item = &Track> {
        self.tracks.values()
    }

    pub fn len(&self) -> usize {
        self.tracks.len()
    }

    pub fn is_empty(&self) -> bool {
        self.tracks.is_empty()
    }

    pub fn track(&self, id: &str) -> Option<&Track> {
        self.tracks.get(id)
    }

    /// The song's file. The path comes from a shared file, so it is checked to stay in `Music/`.
    pub fn file(&self, id: &str) -> Option<PathBuf> {
        let rel = &self.tracks.get(id)?.info.path;
        match self.aliases.get(rel) {
            Some(p) => Some(p.clone()),
            None => resolve(&self.root, rel),
        }
    }

    /// Every known path with its record, for a scan. Removed songs are included, so that a scan
    /// does not bring them back.
    pub fn known(&self) -> HashMap<String, Info> {
        self.merged
            .iter()
            .filter(|(k, _)| k.starts_with("track/"))
            .filter_map(|(k, _)| self.field::<Info>(k, "info"))
            .filter(|i| !i.path.is_empty())
            .map(|i| (i.path.clone(), i))
            .collect()
    }

    /// Takes in what a scan found. Returns how many songs were added.
    pub fn apply_scan(&mut self, scan: ScanResult) -> usize {
        self.scanned = true;
        self.presence = scan.presence;
        self.aliases = scan.aliases;
        let mut by_path: HashMap<String, String> = HashMap::new();
        for key in self.merged.keys() {
            if let Some(id) = key.strip_prefix("track/")
                && let Some(info) = self.field::<Info>(key, "info")
                && !info.path.is_empty()
            {
                by_path.insert(info.path, id.to_owned());
            }
        }
        // Songs whose file is gone: a new file with the same content is the same song, moved.
        let mut gone: HashMap<(u64, String, String), Vec<String>> = HashMap::new();
        // Songs that had no file: a file of the same song becomes theirs.
        let mut waiting: HashMap<String, (String, u64)> = HashMap::new();
        for t in self.tracks.values().filter(|t| t.info.path.is_empty()) {
            waiting.insert(song_key(&t.info), (t.id.clone(), t.info.duration_ms));
        }
        for t in self.tracks.values().filter(|t| !t.info.path.is_empty() && !self.presence.contains_key(&t.info.path)) {
            gone.entry((t.info.size, t.info.title.clone(), t.info.album.clone())).or_default().push(t.id.clone());
        }
        let now = now_ms();
        let mut added = 0;
        for info in scan.read {
            let value = serde_json::to_value(&info).unwrap_or(Value::Null);
            if let Some(id) = by_path.get(&info.path) {
                let key = format!("track/{id}");
                let old: Option<Info> = self.field(&key, "info");
                if old.as_ref() != Some(&info) {
                    self.set(Section::Tracks, &key, "info", value);
                }
                continue;
            }
            let key = song_key(&info);
            let close = |ms: u64| ms == 0 || info.duration_ms == 0 || ms.abs_diff(info.duration_ms) <= 5_000;
            let arrived = waiting.get(&key).filter(|(_, ms)| close(*ms)).map(|(id, _)| id.clone());
            if arrived.is_some() {
                waiting.remove(&key);
            }
            let moved = if arrived.is_some() {
                arrived
            } else if info.guessed {
                None
            } else {
                gone.get_mut(&(info.size, info.title.clone(), info.album.clone())).and_then(Vec::pop)
            };
            let id = match moved {
                Some(id) => id,
                None => {
                    added += 1;
                    self.free_id(&info)
                }
            };
            let key = format!("track/{id}");
            self.set(Section::Tracks, &key, "info", value);
            if self.field::<u64>(&key, "added").is_none() {
                self.set(Section::Tracks, &key, "added", Value::from(now));
            }
            by_path.insert(info.path, id);
        }
        self.rebuild();
        added
    }

    /// The id of a new file. It follows from the path, so two computers that find the same file
    /// at the same time give it the same id.
    fn free_id(&self, info: &Info) -> String {
        for salt in 0..64u32 {
            let mut h = Sha256::new();
            h.update(info.path.to_lowercase().as_bytes());
            if salt > 0 {
                h.update(format!("\0{}\0{salt}", info.size).as_bytes());
            }
            let id: String = h.finalize().iter().take(16).map(|b| format!("{b:02x}")).collect();
            match self.field::<Info>(&format!("track/{id}"), "info") {
                Some(other) if other.path != info.path => {}
                _ => return id,
            }
        }
        uuid::Uuid::new_v4().simple().to_string()
    }

    /// Changes a song's tags, in the file and in the library.
    pub fn edit(&mut self, id: &str, change: impl FnOnce(&mut Info)) -> Result<(), LibraryError> {
        let track = self.tracks.get(id).ok_or(LibraryError::NoTrack)?;
        if track.info.path.is_empty() {
            let mut info = track.info.clone();
            change(&mut info);
            info.path.clear();
            let value = serde_json::to_value(&info).unwrap_or(Value::Null);
            self.set(Section::Tracks, &format!("track/{id}"), "info", value);
            self.touch(id);
            return Ok(());
        }
        let file = self.file(id).ok_or(LibraryError::NoTrack)?;
        let meta = std::fs::metadata(&file).map_err(|_| LibraryError::NotHere)?;
        if crate::cloud::online_only(&meta) {
            return Err(LibraryError::NotHere);
        }
        let mut info = track.info.clone();
        let path = info.path.clone();
        change(&mut info);
        info.path = path;
        crate::tags::write(&file, &info).map_err(LibraryError::Tags)?;
        info.size = std::fs::metadata(&file).map(|m| m.len()).unwrap_or(info.size);
        info.guessed = false;
        let value = serde_json::to_value(&info).unwrap_or(Value::Null);
        self.set(Section::Tracks, &format!("track/{id}"), "info", value);
        self.touch(id);
        Ok(())
    }

    /// The file of a song, when it is on this computer and not only in the cloud.
    fn file_here(&self, id: &str) -> Result<PathBuf, LibraryError> {
        let track = self.tracks.get(id).ok_or(LibraryError::NoTrack)?;
        if track.info.path.is_empty() {
            return Err(LibraryError::NoFile);
        }
        let file = self.file(id).ok_or(LibraryError::NotHere)?;
        let meta = std::fs::metadata(&file).map_err(|_| LibraryError::NotHere)?;
        if crate::cloud::online_only(&meta) {
            return Err(LibraryError::NotHere);
        }
        Ok(file)
    }

    /// The lyrics inside a song's file; empty when it has none. They are not kept in the catalogue.
    pub fn lyrics(&self, id: &str) -> Result<String, LibraryError> {
        crate::tags::lyrics(&self.file_here(id)?).map_err(LibraryError::Other)
    }

    /// Writes a song's lyrics into its file, where other computers read them.
    pub fn set_lyrics(&mut self, id: &str, text: &str) -> Result<(), LibraryError> {
        let file = self.file_here(id)?;
        crate::tags::write_lyrics(&file, text).map_err(LibraryError::Tags)?;
        let Some(track) = self.tracks.get(id) else { return Err(LibraryError::NoTrack) };
        let mut info = track.info.clone();
        info.size = std::fs::metadata(&file).map(|m| m.len()).unwrap_or(info.size);
        let value = serde_json::to_value(&info).unwrap_or(Value::Null);
        self.set(Section::Tracks, &format!("track/{id}"), "info", value);
        self.touch(id);
        Ok(())
    }

    fn activity(&mut self, id: &str, field: &str, v: Value) -> Result<(), LibraryError> {
        if !self.tracks.contains_key(id) {
            return Err(LibraryError::NoTrack);
        }
        self.set(Section::Activity, &format!("track/{id}"), field, v);
        self.touch(id);
        Ok(())
    }

    pub fn set_loved(&mut self, id: &str, loved: bool) -> Result<(), LibraryError> {
        self.activity(id, "loved", Value::from(loved))
    }

    pub fn set_rating(&mut self, id: &str, stars: u8) -> Result<(), LibraryError> {
        self.activity(id, "rating", Value::from(stars.min(5)))
    }

    /// Takes a song off the list. Its file stays where it is.
    pub fn remove_track(&mut self, id: &str) -> Result<(), LibraryError> {
        self.activity(id, "removed", Value::from(true))
    }

    /// Brings back every song taken off the list. Returns how many.
    pub fn restore_removed(&mut self) -> usize {
        let keys: Vec<String> = self
            .merged
            .keys()
            .filter(|k| k.starts_with("track/") && self.field::<bool>(k, "removed").unwrap_or(false))
            .cloned()
            .collect();
        for key in &keys {
            self.set(Section::Activity, key, "removed", Value::from(false));
        }
        self.rebuild();
        keys.len()
    }

    fn count(&mut self, id: &str, change: impl FnOnce(&mut Counter)) -> Result<(), LibraryError> {
        if !self.tracks.contains_key(id) {
            return Err(LibraryError::NoTrack);
        }
        let i = Section::Activity as usize;
        let Some(own) = self.own.get_mut(i) else { return Ok(()) };
        let c = own.counters.entry(id.to_owned()).or_default();
        change(c);
        let c = *c;
        self.counts.entry(id.to_owned()).or_default().insert(self.device.clone(), c);
        if let Some(d) = self.dirty.get_mut(i) {
            *d = true;
        }
        self.touch(id);
        Ok(())
    }

    pub fn record_play(&mut self, id: &str) -> Result<(), LibraryError> {
        let now = now_ms();
        self.count(id, |c| {
            c.plays = c.plays.saturating_add(1);
            c.last = now;
        })
    }

    pub fn record_skip(&mut self, id: &str) -> Result<(), LibraryError> {
        self.count(id, |c| c.skips = c.skips.saturating_add(1))
    }

    /// The song whose file has this path below `Music/`.
    pub fn paths(&self) -> HashMap<String, String> {
        self.tracks.values().filter(|t| !t.info.path.is_empty()).map(|t| (t.info.path.clone(), t.id.clone())).collect()
    }

    /// Takes in what another program knew about songs. The person's own stars and love in this
    /// library are kept; a date is taken when it is earlier; counts replace those imported before.
    pub fn add_history(&mut self, songs: &[(String, History)]) {
        for (id, h) in songs {
            let Some(added) = self.tracks.get(id).map(|t| t.added) else { continue };
            let key = format!("track/{id}");
            if h.added > 0 && (added == 0 || h.added < added) {
                self.set(Section::Tracks, &key, "added", Value::from(h.added));
            }
            if h.rating > 0 && self.field::<u8>(&key, "rating").is_none() {
                self.set(Section::Activity, &key, "rating", Value::from(h.rating.min(5)));
            }
            if h.loved && self.field::<bool>(&key, "loved").is_none() {
                self.set(Section::Activity, &key, "loved", Value::from(true));
            }
            let counts = Counter { plays: h.plays, skips: h.skips, last: h.last_played };
            let old: Counter = self.field(&key, "imported").unwrap_or_default();
            if counts != old {
                let v = serde_json::to_value(counts).unwrap_or(Value::Null);
                self.set(Section::Activity, &key, "imported", v);
            }
        }
        self.rebuild();
    }

    /// Makes the playlists of another program, each under an id that follows from `origin`, so
    /// importing again makes none twice. One that is there, perhaps edited since, stays as it is; one
    /// that was deleted comes back. Gives back how many were made.
    pub fn add_playlists(&mut self, lists: Vec<(String, String, Vec<Item>)>) -> usize {
        let mut made = 0;
        for (origin, name, items) in lists {
            let hash = Sha256::digest(format!("playlist\0{origin}").as_bytes());
            let id: String = hash.iter().take(16).map(|b| format!("{b:02x}")).collect();
            if self.playlist(&id).is_some() {
                continue;
            }
            made += 1;
            let key = format!("playlist/{id}");
            self.set(Section::Playlists, &key, "name", Value::from(name));
            self.set(Section::Playlists, &key, "created", Value::from(now_ms()));
            self.set(Section::Playlists, &key, "items", serde_json::to_value(items).unwrap_or(Value::Null));
            if self.field::<bool>(&key, "removed").unwrap_or(false) {
                self.set(Section::Playlists, &key, "removed", Value::from(false));
            }
        }
        self.rebuild_lists();
        made
    }

    // ---- playlists and stations ----

    pub fn playlists(&self) -> &[Playlist] {
        &self.playlists
    }

    pub fn playlist(&self, id: &str) -> Option<&Playlist> {
        self.playlists.iter().find(|p| p.id == id)
    }

    pub fn create_playlist(&mut self, name: &str, items: Vec<Item>, smart: Option<Rules>) -> String {
        let id = uuid::Uuid::new_v4().simple().to_string();
        let key = format!("playlist/{id}");
        self.set(Section::Playlists, &key, "name", Value::from(name));
        self.set(Section::Playlists, &key, "created", Value::from(now_ms()));
        if let Some(rules) = smart {
            self.set(Section::Playlists, &key, "smart", serde_json::to_value(rules).unwrap_or(Value::Null));
        } else {
            self.set(Section::Playlists, &key, "items", serde_json::to_value(items).unwrap_or(Value::Null));
        }
        self.rebuild_lists();
        id
    }

    fn set_playlist(&mut self, id: &str, field: &str, v: Value) -> Result<(), LibraryError> {
        if self.playlist(id).is_none() {
            return Err(LibraryError::NoPlaylist);
        }
        self.set(Section::Playlists, &format!("playlist/{id}"), field, v);
        self.rebuild_lists();
        Ok(())
    }

    pub fn rename_playlist(&mut self, id: &str, name: &str) -> Result<(), LibraryError> {
        self.set_playlist(id, "name", Value::from(name))
    }

    pub fn delete_playlist(&mut self, id: &str) -> Result<(), LibraryError> {
        self.set_playlist(id, "removed", Value::from(true))
    }

    pub fn set_rules(&mut self, id: &str, rules: &Rules) -> Result<(), LibraryError> {
        self.set_playlist(id, "smart", serde_json::to_value(rules).unwrap_or(Value::Null))
    }

    pub fn set_items(&mut self, id: &str, items: Vec<Item>) -> Result<(), LibraryError> {
        if self.playlist(id).is_some_and(|p| p.smart.is_some()) {
            return Err(LibraryError::SmartItems);
        }
        self.set_playlist(id, "items", serde_json::to_value(items).unwrap_or(Value::Null))
    }

    pub fn add_to_playlist(&mut self, id: &str, more: Vec<Item>) -> Result<(), LibraryError> {
        let mut items = self.playlist(id).ok_or(LibraryError::NoPlaylist)?.items.clone();
        items.extend(more);
        self.set_items(id, items)
    }

    pub fn stations(&self) -> &[Station] {
        &self.stations
    }

    pub fn add_station(&mut self, name: &str, url: &str, detail: &str) -> String {
        let id = uuid::Uuid::new_v4().simple().to_string();
        let s = Station { id: id.clone(), name: name.to_owned(), url: url.to_owned(), detail: detail.to_owned() };
        let v = serde_json::to_value(s).unwrap_or(Value::Null);
        self.set(Section::Playlists, &format!("station/{id}"), "station", v);
        self.rebuild_lists();
        id
    }

    pub fn remove_station(&mut self, id: &str) {
        self.set(Section::Playlists, &format!("station/{id}"), "removed", Value::from(true));
        self.rebuild_lists();
    }
}

fn empty(section: Section, device: &str, device_name: &str) -> FileData {
    FileData {
        format: FORMAT.to_owned(),
        version: FORMAT_VERSION,
        section,
        device: device.to_owned(),
        device_name: device_name.to_owned(),
        records: BTreeMap::new(),
        counters: BTreeMap::new(),
    }
}

fn read_file(path: &Path) -> Result<FileData, String> {
    let meta = std::fs::metadata(path).map_err(|e| e.to_string())?;
    if meta.len() > MAX_FILE {
        return Err("the file is too large".to_owned());
    }
    parse(&std::fs::read(path).map_err(|e| e.to_string())?)
}

/// A catalogue file's bytes, read. Refuses anything this version does not know.
fn parse(bytes: &[u8]) -> Result<FileData, String> {
    if u64::try_from(bytes.len()).unwrap_or(u64::MAX) > MAX_FILE {
        return Err("the file is too large".to_owned());
    }
    let data: FileData = serde_json::from_slice(bytes).map_err(|e| e.to_string())?;
    if data.format != FORMAT {
        return Err("not a MusicCraft library file".to_owned());
    }
    if data.version > FORMAT_VERSION {
        return Err("written by a newer MusicCraft".to_owned());
    }
    Ok(data)
}

/// The file for a path below `Music/`, or nothing when the path would leave that folder.
pub fn resolve(root: &Path, rel: &str) -> Option<PathBuf> {
    let mut out = root.join("Music");
    let mut any = false;
    for part in rel.split('/') {
        if part.is_empty() || part == "." || part == ".." || part.contains(['\\', ':', '\0']) {
            return None;
        }
        out.push(part);
        any = true;
    }
    any.then_some(out)
}

#[cfg(test)]
mod tests {
    use super::{Library, resolve};
    use crate::model::{Info, Item, Link, Presence};
    use crate::scan::ScanResult;
    use std::path::Path;

    fn open(root: &Path, local: &Path, device: &str) -> Library {
        Library::open(root, local, device, device).unwrap()
    }

    fn found(paths: &[(&str, u64)]) -> ScanResult {
        let mut s = ScanResult::default();
        for (p, size) in paths {
            s.presence.insert((*p).to_owned(), Presence::Here);
            s.read.push(Info { path: (*p).to_owned(), title: "Same".into(), size: *size, ..Info::default() });
        }
        s
    }

    /// Two computers sharing a folder see each other's songs, plays and playlists, whichever
    /// saves first; plays made on both are added, not overwritten.
    #[test]
    fn two_computers_converge() {
        let (shared, la, lb) =
            (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
        let mut a = open(shared.path(), la.path(), "aaa");
        let mut b = open(shared.path(), lb.path(), "bbb");
        a.apply_scan(found(&[("X/Y/1.mp3", 10)]));
        b.apply_scan(found(&[("X/Y/1.mp3", 10)]));
        let id = a.tracks().next().unwrap().id.clone();
        assert_eq!(b.tracks().next().unwrap().id, id, "the same file has the same id on both");
        a.record_play(&id).unwrap();
        b.record_play(&id).unwrap();
        b.record_play(&id).unwrap();
        b.set_loved(&id, true).unwrap();
        let list = a.create_playlist("Mix", vec![Item::local(&id)], None);
        a.save().unwrap();
        b.save().unwrap();
        assert!(a.refresh() && b.refresh());
        b.rename_playlist(&list, "Mix 2").unwrap();
        b.save().unwrap();
        assert!(a.refresh());
        for lib in [&a, &b] {
            let t = lib.track(&id).unwrap();
            assert!(t.loved && t.plays == 3);
            assert_eq!(lib.playlists()[0].name, "Mix 2");
        }
        assert!(!a.refresh(), "nothing changed since");
    }

    /// A value written after seeing another computer's value wins over it, even when that
    /// computer's clock runs ahead.
    #[test]
    fn a_later_edit_wins_over_a_fast_clock() {
        let (shared, la, lb) =
            (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
        let mut b = open(shared.path(), lb.path(), "bbb");
        b.apply_scan(found(&[("1.mp3", 10)]));
        let id = b.tracks().next().unwrap().id.clone();
        b.set_rating(&id, 2).unwrap();
        b.save().unwrap();
        let file = shared.path().join("Library/bbb.activity.json");
        let mut text: serde_json::Value = serde_json::from_slice(&std::fs::read(&file).unwrap()).unwrap();
        text["records"][format!("track/{id}")]["rating"]["t"] = (crate::now_ms() + 3_600_000).into();
        std::fs::write(&file, serde_json::to_vec(&text).unwrap()).unwrap();
        let mut a = open(shared.path(), la.path(), "aaa");
        assert_eq!(a.track(&id).unwrap().rating, 2);
        a.set_rating(&id, 5).unwrap();
        a.save().unwrap();
        assert_eq!(open(shared.path(), lb.path(), "ccc").track(&id).unwrap().rating, 5);
    }

    /// A sync service's conflict copy of a file changes nothing, and neither does a damaged file
    /// of another computer; a damaged file of our own is restored from this computer's copy.
    #[test]
    fn conflict_copies_and_damage_lose_nothing() {
        let (shared, la) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
        let mut a = open(shared.path(), la.path(), "aaa");
        a.apply_scan(found(&[("1.mp3", 10)]));
        let id = a.tracks().next().unwrap().id.clone();
        a.record_play(&id).unwrap();
        a.save().unwrap();
        let dir = shared.path().join("Library");
        std::fs::copy(dir.join("aaa.activity.json"), dir.join("aaa.activity-PC2.json")).unwrap();
        std::fs::write(dir.join("zzz.tracks.json"), b"{\"format\":").unwrap();
        a.refresh();
        assert_eq!(a.track(&id).unwrap().plays, 1);
        assert_eq!(a.warnings().len(), 1);
        std::fs::write(dir.join("aaa.tracks.json"), b"garbage").unwrap();
        assert_eq!(open(shared.path(), la.path(), "aaa").len(), 1);
    }

    /// A file moved outside MusicCraft keeps its plays; a song taken off the list stays off at
    /// the next scan; a missing file does not remove its song.
    #[test]
    fn scans_follow_moves_and_respect_removals() {
        let (shared, la) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
        let mut a = open(shared.path(), la.path(), "aaa");
        a.apply_scan(found(&[("old/1.mp3", 10)]));
        let id = a.tracks().next().unwrap().id.clone();
        a.record_play(&id).unwrap();
        assert_eq!(a.apply_scan(found(&[("new/1.mp3", 10)])), 0);
        let t = a.track(&id).unwrap();
        assert_eq!((t.info.path.as_str(), t.plays), ("new/1.mp3", 1));
        a.apply_scan(found(&[]));
        assert_eq!(a.track(&id).unwrap().presence, Presence::Missing);
        a.remove_track(&id).unwrap();
        a.apply_scan(found(&[("new/1.mp3", 10)]));
        assert!(a.is_empty());
        assert_eq!(a.restore_removed(), 1);
        assert_eq!(a.len(), 1);
    }

    fn song(title: &str) -> Info {
        Info { title: title.into(), artist: "Ada".into(), album: "X".into(), duration_ms: 200_000, ..Info::default() }
    }

    fn link(id: &str) -> Link {
        Link { id: id.into(), at: 1, ..Link::default() }
    }

    /// A song without a file keeps its id, plays and playlists through a change of service and when
    /// its file arrives; two computers that add it agree on the id.
    #[test]
    fn a_song_outlives_its_services_and_takes_its_file() {
        let (shared, la, lb) =
            (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
        let mut a = open(shared.path(), la.path(), "aaa");
        let mut b = open(shared.path(), lb.path(), "bbb");
        let (ids, n) = a.add_songs(vec![(song("Uno"), Some(("spotify".into(), link("s1"))))]);
        let (other, _) = b.add_songs(vec![(song("UNO (feat. Bea)"), Some(("server:casa".into(), link("c1"))))]);
        assert_eq!((n, &ids), (1, &other));
        let id = ids[0].clone();
        a.record_play(&id).unwrap();
        a.save().unwrap();
        b.save().unwrap();
        a.refresh();
        let t = a.track(&id).unwrap();
        assert_eq!((t.presence, t.links.len()), (Presence::Stream, 2));
        a.clear_service("spotify");
        assert_eq!(a.track(&id).unwrap().links.keys().collect::<Vec<_>>(), ["server:casa"]);
        let mut scan = ScanResult::default();
        scan.presence.insert("Ada/X/Uno.mp3".into(), Presence::Here);
        scan.read.push(Info { path: "Ada/X/Uno.mp3".into(), duration_ms: 201_000, size: 9, ..song("Uno") });
        assert_eq!(a.apply_scan(scan), 0);
        let t = a.track(&id).unwrap();
        assert_eq!((t.presence, t.plays, a.len()), (Presence::Here, 1, 1));
    }

    /// Another take with the same title is not taken for the song, and adding a song that is here
    /// as a file adds nothing.
    #[test]
    fn songs_are_not_confused_or_doubled() {
        let (shared, la) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
        let mut a = open(shared.path(), la.path(), "aaa");
        a.add_songs(vec![(song("Uno"), None)]);
        let mut scan = ScanResult::default();
        scan.presence.insert("u.mp3".into(), Presence::Here);
        scan.read.push(Info { path: "u.mp3".into(), duration_ms: 420_000, size: 9, ..song("Uno") });
        assert_eq!(a.apply_scan(scan), 1, "a much longer recording is another song");
        assert_eq!(a.add_songs(vec![(song("uno"), Some(("spotify".into(), link("z"))))]).1, 0);
    }

    /// A match the person chose is not replaced by one that was found.
    #[test]
    fn a_chosen_match_stays() {
        let (shared, la) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
        let mut a = open(shared.path(), la.path(), "aaa");
        let chosen = Link { chosen: true, ..link("mine") };
        let (ids, _) = a.add_songs(vec![(song("Uno"), None)]);
        a.set_link(&ids[0], "spotify", &chosen).unwrap();
        a.add_songs(vec![(song("Uno"), Some(("spotify".into(), link("found"))))]);
        assert_eq!(a.track(&ids[0]).unwrap().links["spotify"], chosen);
    }

    /// A shared file cannot point MusicCraft at anything outside the music folder.
    #[test]
    fn paths_from_shared_files_stay_inside() {
        let root = Path::new("lib");
        for bad in ["../x.mp3", "a/../../x", "/etc/passwd", "C:/x", "a\\..\\b", "", "a//b", "a/\0"] {
            assert!(resolve(root, bad).is_none(), "{bad:?}");
        }
        assert!(resolve(root, "a/b.mp3").unwrap().starts_with("lib"));
    }
}
