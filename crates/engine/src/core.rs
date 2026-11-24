//! The engine: one value that holds everything and is driven by [`Engine::run`] and
//! [`Engine::tick`].

use crate::art::{Art, From as ArtFrom};
use crate::command::{Command, TagEdit};
use crate::pages::{Page, PageState, Pages, Via};
use crate::queue::{Queue, Repeat};
use crate::remote::{self, Remote};
use crate::settings::{ServerAccount, Settings, device_name};
use musiccraft_library::import::Report;
use musiccraft_library::model::Presence;
use musiccraft_library::query::{self, Search};
use musiccraft_library::scan::{self, Cache, Progress, ScanResult};
use musiccraft_library::{Item, Library, Meta, Source, Track, m3u, now_ms};
use musiccraft_player::{Input, Player, Status};
use musiccraft_services::soloist::{self, Soloist};
use musiccraft_services::spotify::{self, Hosts, Spotify};
use musiccraft_services::subsonic::Server;
use musiccraft_services::{Entry, radio, secrets};
use serde_json::{Value, json};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::time::{Duration, Instant};

const SAVE_AFTER: Duration = Duration::from_millis(1500);
const LOOK_EVERY: Duration = Duration::from_secs(5);
const SCAN_EVERY: Duration = Duration::from_secs(900);
/// The next song is made ready this long before the current one ends.
const PRELOAD_MS: u64 = 8_000;
const SIGN_IN_WAIT: Duration = Duration::from_secs(300);
const SOLOIST_KEY: &str = "soloist-key";

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum NowStatus {
    #[default]
    Stopped,
    Loading,
    Playing,
    Paused,
    Failed,
}

/// What is playing.
#[derive(Debug, Clone, Default)]
pub struct Now {
    pub item: Option<Item>,
    pub meta: Meta,
    pub status: NowStatus,
    pub position_ms: u64,
    pub duration_ms: u64,
    pub live: bool,
    pub stream_title: String,
    pub error: String,
}

/// A message for the person. `error` ones stay until dismissed.
#[derive(Debug, Clone, PartialEq)]
pub struct Notice {
    pub text: String,
    /// What the text's `{}` stands for.
    pub detail: String,
    pub error: bool,
}

enum Event {
    Scan(Option<ScanResult>),
    Import(Report),
    Resolved(u64, Result<String, String>),
    SignedIn(String, Result<(Arc<Spotify>, String), String>),
    ServerChecked(ServerAccount, String, Result<(), String>),
}

struct Playing {
    item: Item,
    meta: Meta,
    remote: bool,
    /// The player's number for it; 0 before it is known.
    number: u64,
    counted: bool,
    /// Shown but not opened yet; playing starts it.
    held: bool,
}

pub struct Engine {
    dir: PathBuf,
    pub settings: Settings,
    lib: Option<Library>,
    player: Player,
    pub queue: Queue,
    playing: Option<Playing>,
    preloaded: Option<(u64, Item)>,
    remote: Option<Remote>,
    spotify: Option<Arc<Spotify>>,
    spotify_looked: bool,
    pub spotify_name: String,
    /// The sign-in page, while the browser is expected back.
    pub sign_in_page: Option<String>,
    soloist: Option<Soloist>,
    servers: HashMap<String, Option<Server>>,
    pub pages: Pages,
    pub art: Art,
    tx: Sender<Event>,
    rx: Receiver<Event>,
    scan: Option<Arc<Progress>>,
    scan_again: bool,
    import: Option<Arc<Progress>>,
    import_total: usize,
    checking: usize,
    resolving: u64,
    pub notices: Vec<Notice>,
    dirty_since: Option<Instant>,
    looked: Instant,
    scanned: Instant,
    /// Files the last scan could not read.
    pub unreadable: Vec<(String, String)>,
    /// Test libraries read nothing from the credential store.
    use_secrets: bool,
}

fn data(v: impl serde::Serialize) -> Value {
    serde_json::to_value(v).unwrap_or(Value::Null)
}

fn text(e: impl std::fmt::Display) -> String {
    e.to_string()
}

impl Engine {
    /// `dir` is this computer's data folder.
    pub fn new(dir: &Path) -> Engine {
        let settings = Settings::load(dir);
        Self::with(dir, settings, true)
    }

    /// An engine for a demonstration: nothing of the person's is read or changed.
    pub fn isolated(dir: &Path, library: Option<PathBuf>) -> Engine {
        let settings = Settings { library, ..Settings::default() };
        Self::with(dir, settings, false)
    }

    fn with(dir: &Path, settings: Settings, use_secrets: bool) -> Engine {
        let (tx, rx) = channel();
        let player = Player::new();
        player.set_volume(f32::from(settings.volume.min(100)) / 100.0);
        let mut e = Engine {
            dir: dir.to_owned(),
            queue: Queue::new(settings.shuffle, settings.repeat),
            settings,
            lib: None,
            player,
            playing: None,
            preloaded: None,
            remote: None,
            spotify: None,
            spotify_looked: false,
            spotify_name: String::new(),
            sign_in_page: None,
            soloist: None,
            servers: HashMap::new(),
            pages: Pages::default(),
            art: Art::default(),
            tx,
            rx,
            scan: None,
            scan_again: false,
            import: None,
            import_total: 0,
            checking: 0,
            resolving: 0,
            notices: Vec::new(),
            dirty_since: None,
            looked: Instant::now(),
            scanned: Instant::now(),
            unreadable: Vec::new(),
            use_secrets,
        };
        if let Some(path) = e.settings.library.clone()
            && let Err(err) = e.open_library(&path)
        {
            e.error("The library could not be opened: {}", &err);
        }
        e
    }

    pub fn error(&mut self, text: &str, detail: &str) {
        self.notice(text, detail, true);
    }

    fn notice(&mut self, text: &str, detail: &str, error: bool) {
        let n = Notice { text: text.to_owned(), detail: detail.to_owned(), error };
        if self.notices.last() != Some(&n) {
            self.notices.push(n);
        }
        let extra = self.notices.len().saturating_sub(5);
        self.notices.drain(..extra);
    }

    pub fn library(&self) -> Option<&Library> {
        self.lib.as_ref()
    }

    fn lib_mut(&mut self) -> Result<&mut Library, String> {
        self.lib.as_mut().ok_or_else(|| "no library is open".to_owned())
    }

    fn open_library(&mut self, path: &Path) -> Result<(), String> {
        self.save_now();
        let key: String = path.to_string_lossy().chars().map(|c| if c.is_alphanumeric() { c } else { '_' }).collect();
        let keep: String = key.chars().rev().take(80).collect::<Vec<_>>().into_iter().rev().collect();
        let local = self.dir.join("libraries").join(keep);
        let lib = Library::open(path, &local, &self.settings.device, &device_name()).map_err(text)?;
        self.lib = Some(lib);
        self.settings.library = Some(path.to_owned());
        self.save_settings();
        self.start_scan();
        Ok(())
    }

    fn cache_path(&self) -> PathBuf {
        self.dir.join("scan-cache.json")
    }

    fn save_settings(&mut self) {
        if !self.use_secrets {
            return;
        }
        if let Err(e) = self.settings.save(&self.dir) {
            self.error("The settings could not be saved: {}", &e);
        }
    }

    /// Changes a setting of this computer and saves it.
    pub fn change_settings(&mut self, change: impl FnOnce(&mut Settings)) {
        change(&mut self.settings);
        self.save_settings();
    }

    /// Writes unsaved changes at once; for closing.
    pub fn save_now(&mut self) {
        self.dirty_since = None;
        if let Some(lib) = &mut self.lib
            && let Err(e) = lib.save()
        {
            self.error("The library could not be saved: {}", &e.to_string());
        }
    }

    // ---- background work ----

    fn start_scan(&mut self) {
        let Some(lib) = &self.lib else { return };
        if self.scan.is_some() {
            self.scan_again = true;
            return;
        }
        let progress = Arc::new(Progress::default());
        let (music, known, tx, p, cache) =
            (lib.music_dir(), lib.known(), self.tx.clone(), progress.clone(), self.cache_path());
        let started = crate::spawn("scan", move || {
            let cache: Cache =
                std::fs::read(cache).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default();
            tx.send(Event::Scan(scan::run(&music, &known, &cache, &p))).ok();
        });
        self.scanned = Instant::now();
        match started {
            Ok(()) => self.scan = Some(progress),
            Err(e) => self.error("The music folder could not be read: {}", &e),
        }
    }

    /// Files found and read by the scan under way.
    pub fn scan_progress(&self) -> Option<(usize, usize)> {
        self.scan.as_ref().map(|p| (p.found.load(Ordering::Relaxed), p.read.load(Ordering::Relaxed)))
    }

    /// Files copied, of how many, by the import under way.
    pub fn import_progress(&self) -> Option<(usize, usize)> {
        self.import.as_ref().map(|p| (p.read.load(Ordering::Relaxed), self.import_total))
    }

    /// Whether something runs in the background.
    pub fn is_busy(&self) -> bool {
        self.scan.is_some()
            || self.import.is_some()
            || self.checking > 0
            || self.sign_in_page.is_some()
            || self.resolving > 0
            || self.pages.is_busy()
            || self.art.is_busy()
    }

    fn event(&mut self, e: Event) {
        match e {
            Event::Scan(result) => {
                self.scan = None;
                match (result, &mut self.lib) {
                    (Some(mut r), Some(lib)) => {
                        self.unreadable = std::mem::take(&mut r.failed);
                        let cache = std::mem::take(&mut r.cache);
                        lib.apply_scan(r);
                        if let Ok(bytes) = serde_json::to_vec(&cache) {
                            // Without the cache the next scan only reads more.
                            craft_atomic::atomic_write(&self.cache_path(), &bytes).ok();
                        }
                    }
                    (None, Some(_)) => self.error("The music folder could not be read: {}", ""),
                    _ => {}
                }
                if std::mem::take(&mut self.scan_again) {
                    self.start_scan();
                }
            }
            Event::Import(r) => {
                self.import = None;
                if let Some((path, why)) = r.failed.first() {
                    self.error("Some files could not be added: {}", &format!("{} ({why})", path.display()));
                }
                let detail = format!("{} / {}", r.copied, r.duplicates);
                self.notice("Songs added / already there: {}", &detail, false);
                self.start_scan();
            }
            Event::Resolved(token, url) => {
                if self.resolving != token {
                    return;
                }
                self.resolving = 0;
                match url {
                    Ok(url) => {
                        let n = self.player.load(Input::Url { url, live: true }, Duration::ZERO, false);
                        if let Some(p) = &mut self.playing {
                            p.number = n;
                        }
                    }
                    Err(e) => {
                        self.playing = None;
                        self.error("The station cannot be played: {}", &e);
                    }
                }
            }
            Event::SignedIn(client_id, result) => {
                self.sign_in_page = None;
                match result {
                    Ok((account, name)) => {
                        self.settings.spotify_client_id = client_id;
                        self.save_settings();
                        self.spotify = Some(account);
                        self.spotify_looked = true;
                        self.spotify_name = name;
                        self.remote = None;
                        self.pages.clear();
                        self.notice("Signed in to Spotify.", "", false);
                    }
                    Err(e) => self.error("The Spotify sign-in did not work: {}", &e),
                }
            }
            Event::ServerChecked(account, password, result) => {
                self.checking = self.checking.saturating_sub(1);
                let kept = result.and_then(|()| secrets::set(&account.secret(), &password).map_err(text));
                match kept {
                    Ok(()) => {
                        self.servers.clear();
                        self.settings.servers.retain(|s| s.name != account.name);
                        self.notice("The server was added: {}", &account.name, false);
                        self.settings.servers.push(account);
                        self.save_settings();
                    }
                    Err(e) => self.error("The server could not be added: {}", &e),
                }
            }
        }
    }

    /// Does what is due. Call it a few times a second.
    pub fn tick(&mut self) {
        while let Ok(e) = self.rx.try_recv() {
            self.event(e);
        }
        self.follow_playback();
        if let Some(s) = &mut self.soloist
            && let Some(why) = s.ended()
        {
            self.soloist = None;
            self.error("{}", &why);
        }
        if self.looked.elapsed() > LOOK_EVERY {
            self.looked = Instant::now();
            let changed = self.lib.as_mut().is_some_and(Library::refresh);
            if changed || self.scanned.elapsed() > SCAN_EVERY {
                self.start_scan();
            }
        }
        if self.lib.as_ref().is_some_and(Library::is_dirty) {
            let since = *self.dirty_since.get_or_insert_with(Instant::now);
            if since.elapsed() > SAVE_AFTER {
                self.save_now();
            }
        }
    }

    /// Ticks until the background work is done; for the command line.
    pub fn settle(&mut self, limit: Duration) {
        let until = Instant::now() + limit;
        loop {
            self.tick();
            if !self.is_busy() || Instant::now() > until {
                break;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        self.save_now();
    }

    // ---- services ----

    pub fn server(&mut self, name: &str) -> Option<Server> {
        if !self.use_secrets {
            return None;
        }
        if !self.servers.contains_key(name) {
            let account = self.settings.servers.iter().find(|s| s.name == name)?.clone();
            let server =
                secrets::get(&account.secret()).and_then(|p| Server::new(&account.url, &account.user, &p).ok());
            self.servers.insert(name.to_owned(), server);
        }
        self.servers.get(name).cloned().flatten()
    }

    pub fn spotify(&mut self) -> Option<Arc<Spotify>> {
        if !self.spotify_looked && self.use_secrets {
            self.spotify_looked = true;
            self.spotify = Spotify::stored(&self.settings.spotify_client_id).map(Arc::new);
        }
        self.spotify.clone()
    }

    pub fn soloist_runs(&self) -> bool {
        self.soloist.is_some()
    }

    /// The name this computer has on Spotify while Soloist runs.
    pub fn soloist_name() -> String {
        format!("MusicCraft ({})", device_name())
    }

    /// A list from a service.
    pub fn page(&mut self, page: &Page) -> PageState {
        let via = match page {
            Page::ServerAlbums(a, _)
            | Page::ServerAlbum(a, _)
            | Page::ServerPlaylists(a)
            | Page::ServerPlaylist(a, _)
            | Page::ServerSearch(a, _) => self.server(a).map(Via::Server),
            Page::Radio(_) => Some(Via::Nothing),
            _ => self.spotify().map(Via::Spotify),
        };
        self.pages.get(page, || via)
    }

    /// The name under which a picture is kept, for an item.
    pub fn art_key(&self, item: &Item, meta: &Meta) -> String {
        match &item.source {
            Source::Local => match self.lib.as_ref().and_then(|l| l.track(&item.id)) {
                Some(t) if !t.info.album.is_empty() => format!("album:{}", t.album_key()),
                _ => format!("track:{}", item.id),
            },
            Source::Server { account } => format!("server:{account}:{}", meta.art),
            Source::Spotify => meta.art.clone(),
            Source::Radio => String::new(),
        }
    }

    /// The picture of an item, when it is there.
    pub fn picture(&mut self, item: &Item, meta: &Meta) -> Option<Arc<crate::art::Picture>> {
        let key = self.art_key(item, meta);
        if key.is_empty() {
            return None;
        }
        let server = match &item.source {
            Source::Server { account } if !meta.art.is_empty() => self.server(account),
            _ => None,
        };
        let lib = self.lib.as_ref();
        self.art.get(&key, || match &item.source {
            Source::Local => {
                let lib = lib?;
                let t = lib.track(&item.id)?;
                // Any song of the album that is here will do.
                let here = |t: &Track| t.presence == Presence::Here;
                let from = if here(t) {
                    Some(t)
                } else {
                    let album = t.album_key();
                    lib.tracks().find(|o| here(o) && o.album_key() == album)
                };
                Some(ArtFrom::File(lib.file(&from?.id)?))
            }
            Source::Server { .. } => Some(ArtFrom::Server(server?, meta.art.clone())),
            Source::Spotify if !meta.art.is_empty() => Some(ArtFrom::Web(meta.art.clone())),
            _ => None,
        })
    }

    /// What to show for an item.
    pub fn meta(&self, item: &Item) -> Meta {
        match (&item.source, &self.lib) {
            (Source::Local, Some(lib)) => lib.track(&item.id).map(Track::meta).unwrap_or_default(),
            _ => item.meta.clone().unwrap_or_default(),
        }
    }

    // ---- playing ----

    fn input(&mut self, item: &Item) -> Result<Input, String> {
        match &item.source {
            Source::Local => {
                let lib = self.lib.as_ref().ok_or("no library is open")?;
                let t = lib.track(&item.id).ok_or("the song is no longer in the library")?;
                if t.presence == Presence::Missing {
                    return Err("the file has not arrived on this computer".to_owned());
                }
                Ok(Input::File(lib.file(&item.id).ok_or("the song has no file")?))
            }
            Source::Server { account } => {
                let s = self.server(account).ok_or_else(|| format!("{account}: not signed in"))?;
                Ok(Input::Url { url: s.stream_url(&item.id), live: false })
            }
            Source::Radio => Ok(Input::Url { url: item.id.clone(), live: true }),
            Source::Spotify => Err("plays on a Spotify device".to_owned()),
        }
    }

    /// Plays the queue's current item.
    fn start(&mut self) {
        self.preloaded = None;
        self.resolving = 0;
        let Some(item) = self.queue.current().cloned() else {
            self.stop();
            return;
        };
        let meta = self.meta(&item);
        let remote = item.source == Source::Spotify;
        self.playing =
            Some(Playing { item: item.clone(), meta: meta.clone(), remote, number: 0, counted: false, held: false });
        if remote {
            self.player.stop();
            match self.spotify() {
                Some(account) => {
                    let r = self.remote.get_or_insert_with(|| Remote::new(account));
                    r.play(&item.id, meta.duration_ms, &self.settings.spotify_device);
                }
                None => {
                    self.playing = None;
                    self.error("Sign in to Spotify first, under Streaming.", "");
                }
            }
            return;
        }
        if let Some(r) = &self.remote {
            r.stop();
        }
        if item.source == Source::Radio && radio::is_playlist(&item.id) {
            self.player.stop();
            self.resolve(item.id);
            return;
        }
        match self.input(&item) {
            Ok(input) => {
                let n = self.player.load(input, Duration::ZERO, false);
                if let Some(p) = &mut self.playing {
                    p.number = n;
                }
            }
            Err(e) => {
                self.player.stop();
                self.playing = None;
                self.error("The song cannot be played: {}", &e);
            }
        }
    }

    /// Fills the queue and shows its current song, paused, without opening it.
    pub fn hold(&mut self, items: Vec<Item>, start: usize) {
        self.queue.set(items, start);
        if let Some(item) = self.queue.current().cloned() {
            let meta = self.meta(&item);
            self.playing = Some(Playing { item, meta, remote: false, number: 0, counted: false, held: true });
        }
    }

    /// Reads a station's playlist file to find its stream.
    fn resolve(&mut self, url: String) {
        let token = now_ms().max(1);
        self.resolving = token;
        let tx = self.tx.clone();
        let started = crate::spawn("station", move || {
            let found = radio::fetch(&url).map_err(text).and_then(|bytes| {
                let first = m3u::read(&bytes).into_iter().find(|e| radio::is_web(&e.location));
                first.map(|e| e.location).ok_or_else(|| "its playlist names no stream".to_owned())
            });
            tx.send(Event::Resolved(token, found)).ok();
        });
        if let Err(e) = started {
            self.resolving = 0;
            self.error("The station cannot be played: {}", &e);
        }
    }

    fn stop(&mut self) {
        self.player.stop();
        if let Some(r) = &self.remote {
            r.stop();
        }
        self.playing = None;
        self.preloaded = None;
        self.resolving = 0;
    }

    /// A song played to its end: counts it, here and on its server.
    fn count_play(&mut self) {
        let Some(p) = &mut self.playing else { return };
        if std::mem::replace(&mut p.counted, true) {
            return;
        }
        let item = p.item.clone();
        match &item.source {
            Source::Local => {
                if let Some(lib) = &mut self.lib {
                    // The song may have left the library while it played.
                    lib.record_play(&item.id).ok();
                }
            }
            Source::Server { account } => {
                if let Some(s) = self.server(account) {
                    let id = item.id.clone();
                    // The count on the server is a courtesy; playing goes on without it.
                    crate::spawn("scrobble", move || {
                        s.scrobble(&id).ok();
                    })
                    .ok();
                }
            }
            _ => {}
        }
    }

    /// Goes to the next item; `ended` when the song ended by itself.
    fn advance(&mut self, ended: bool) {
        match self.queue.next_position(ended) {
            Some(n) => {
                self.queue.go(n);
                self.start();
            }
            None => self.stop(),
        }
    }

    fn follow_playback(&mut self) {
        let Some(p) = &self.playing else { return };
        if p.held {
            return;
        }
        if p.remote {
            let Some(r) = &self.remote else { return };
            let s = r.state();
            match s.status {
                remote::Status::Ended => {
                    self.count_play();
                    self.advance(true);
                }
                // Someone took the device over.
                remote::Status::Idle if s.uri == p.item.id => {
                    self.playing = None;
                    self.notice("Something else is playing on the Spotify device.", "", false);
                }
                remote::Status::Failed => {
                    self.playing = None;
                    self.error("Spotify: {}", &s.error);
                }
                _ => {}
            }
            return;
        }
        let s = self.player.state();
        if let Some((number, item)) = &self.preloaded
            && s.current == *number
        {
            let (number, item) = (*number, item.clone());
            self.preloaded = None;
            self.count_play();
            let next = self.queue.next_position(true);
            if let Some(n) = next {
                self.queue.go(n);
            }
            if self.queue.current() == Some(&item) {
                let meta = self.meta(&item);
                self.playing = Some(Playing { item, meta, remote: false, number, counted: false, held: false });
            } else {
                // The queue changed after the next song was made ready.
                self.start();
            }
            return;
        }
        if s.current != p.number {
            return;
        }
        match s.status {
            Status::Ended => {
                self.count_play();
                self.advance(true);
            }
            Status::Failed => {
                self.playing = None;
                self.error("The song cannot be played: {}", &s.error);
            }
            Status::Playing => {
                let left = p.meta.duration_ms.saturating_sub(millis(s.position));
                if self.preloaded.is_none() && p.meta.duration_ms > 0 && left < PRELOAD_MS {
                    self.preload();
                }
            }
            _ => {}
        }
    }

    fn preload(&mut self) {
        let Some(next) = self.queue.next_position(true) else { return };
        // A new round of a shuffled queue gets a new order when it begins.
        if next == 0 && self.queue.shuffle() {
            return;
        }
        let Some(item) = self.queue.items().get(next).cloned() else { return };
        if !matches!(item.source, Source::Local | Source::Server { .. }) {
            return;
        }
        if let Ok(input) = self.input(&item) {
            let n = self.player.preload(input);
            self.preloaded = Some((n, item));
        }
    }

    pub fn now(&self) -> Now {
        let Some(p) = &self.playing else { return Now::default() };
        let mut now = Now {
            item: Some(p.item.clone()),
            meta: p.meta.clone(),
            duration_ms: p.meta.duration_ms,
            live: p.item.source == Source::Radio,
            status: NowStatus::Loading,
            ..Now::default()
        };
        if p.held {
            now.status = NowStatus::Paused;
        } else if p.remote {
            let Some(r) = &self.remote else { return now };
            let s = r.state();
            now.position_ms = s.position_ms();
            now.status = match s.status {
                remote::Status::Idle | remote::Status::Ended => NowStatus::Stopped,
                remote::Status::Loading => NowStatus::Loading,
                remote::Status::Playing => NowStatus::Playing,
                remote::Status::Paused => NowStatus::Paused,
                remote::Status::Failed => NowStatus::Failed,
            };
        } else if p.number > 0 {
            let s = self.player.state();
            if s.current == p.number {
                now.position_ms = millis(s.position);
                now.stream_title = s.stream_title;
                now.status = match s.status {
                    Status::Stopped | Status::Ended => NowStatus::Stopped,
                    Status::Loading => NowStatus::Loading,
                    Status::Playing => NowStatus::Playing,
                    Status::Paused => NowStatus::Paused,
                    Status::Failed => NowStatus::Failed,
                };
            }
        }
        now
    }

    fn seek(&mut self, ms: u64) {
        match &self.playing {
            Some(p) if p.remote => {
                if let Some(r) = &self.remote {
                    r.seek(ms);
                }
            }
            Some(p) if p.item.source != Source::Radio => self.player.seek(Duration::from_millis(ms)),
            _ => {}
        }
    }

    fn play_pause(&mut self) {
        let now = self.now();
        let Some(p) = &self.playing else {
            if self.queue.current().is_some() {
                self.start();
            }
            return;
        };
        if p.held {
            self.start();
            return;
        }
        let pause = matches!(now.status, NowStatus::Playing | NowStatus::Loading);
        match (&self.remote, p.remote) {
            (Some(r), true) if pause => r.pause(),
            (Some(r), true) => r.resume(),
            // A live stream goes on from now, not from where it was left.
            _ if pause && now.live => self.stop(),
            _ if pause => self.player.pause(),
            _ => self.player.play(),
        }
    }

    // ---- commands ----

    /// Carries out a command. The answer is data, so scripts can use it.
    pub fn run(&mut self, command: Command) -> Result<Value, String> {
        let done = Ok(Value::Null);
        match command {
            Command::Play { items, start } => {
                self.queue.set(items, start);
                self.start();
            }
            Command::PlayPause => self.play_pause(),
            Command::Stop => self.stop(),
            Command::Next => {
                let now = self.now();
                if let Some(Item { source: Source::Local, id, .. }) = &now.item {
                    if now.duration_ms > 0 && now.position_ms * 10 >= now.duration_ms * 9 {
                        self.count_play();
                    } else if (2_000..20_000).contains(&now.position_ms)
                        && let Some(lib) = &mut self.lib
                    {
                        // The song may have left the library while it played.
                        lib.record_skip(id).ok();
                    }
                }
                self.advance(false);
            }
            Command::Previous => {
                let now = self.now();
                match self.queue.previous_position() {
                    Some(n) if now.position_ms < 3_000 || now.live => {
                        self.queue.go(n);
                        self.start();
                    }
                    _ => self.seek(0),
                }
            }
            Command::Seek { ms } => self.seek(ms),
            Command::Volume { percent } => {
                let percent = percent.min(100);
                self.player.set_volume(f32::from(percent) / 100.0);
                if let (Some(r), Some(Playing { remote: true, .. })) = (&self.remote, &self.playing) {
                    r.volume(u32::from(percent));
                }
                self.change_settings(|s| s.volume = percent);
            }
            Command::Shuffle { on } => {
                self.queue.set_shuffle(on);
                self.change_settings(|s| s.shuffle = on);
            }
            Command::Repeat { mode } => {
                self.queue.repeat = mode;
                self.change_settings(|s| s.repeat = mode);
            }
            Command::QueueNext { items } => self.queue.insert_next(items),
            Command::QueueAdd { items } => self.queue.append(items),
            Command::QueueRemove { index } => self.queue.remove(index),
            Command::QueueShift { index, later } => self.queue.shift(index, later),
            Command::QueueGo { index } => {
                if !self.queue.go(index) {
                    return Err("there is no such place in the queue".to_owned());
                }
                self.start();
            }
            Command::QueueClear => self.queue.clear_upcoming(),
            Command::Love { ids, on } => {
                let lib = self.lib_mut()?;
                for id in &ids {
                    lib.set_loved(id, on).map_err(text)?;
                }
            }
            Command::Rate { ids, stars } => {
                let lib = self.lib_mut()?;
                for id in &ids {
                    lib.set_rating(id, stars).map_err(text)?;
                }
            }
            Command::Edit { ids, tags } => return self.edit(&ids, &tags),
            Command::Remove { ids } => {
                let lib = self.lib_mut()?;
                for id in &ids {
                    lib.remove_track(id).map_err(text)?;
                }
            }
            Command::RestoreRemoved => return Ok(json!({ "restored": self.lib_mut()?.restore_removed() })),
            Command::Import { paths } => return self.import(&paths),
            Command::Scan => {
                self.lib_mut()?;
                self.start_scan();
            }
            Command::PlaylistNew { name, items } => {
                return Ok(json!({ "id": self.lib_mut()?.create_playlist(&name, items, None) }));
            }
            Command::SmartNew { name, rules } => {
                return Ok(json!({ "id": self.lib_mut()?.create_playlist(&name, Vec::new(), Some(rules)) }));
            }
            Command::SmartSet { id, rules } => self.lib_mut()?.set_rules(&id, &rules).map_err(text)?,
            Command::PlaylistRename { id, name } => self.lib_mut()?.rename_playlist(&id, &name).map_err(text)?,
            Command::PlaylistDelete { id } => self.lib_mut()?.delete_playlist(&id).map_err(text)?,
            Command::PlaylistAdd { id, items } => self.lib_mut()?.add_to_playlist(&id, items).map_err(text)?,
            Command::PlaylistSet { id, items } => self.lib_mut()?.set_items(&id, items).map_err(text)?,
            Command::PlaylistImport { path } => return self.import_playlist(&path),
            Command::PlaylistExport { id, path } => return self.export_playlist(&id, &path),
            Command::StationAdd { name, url, detail } => {
                if !radio::is_web(&url) {
                    return Err("a station's address starts with https:// or http://".to_owned());
                }
                return Ok(json!({ "id": self.lib_mut()?.add_station(&name, &url, &detail) }));
            }
            Command::StationRemove { id } => self.lib_mut()?.remove_station(&id),
            Command::ServerAdd { name, url, user, password } => {
                let server = Server::new(&url, &user, &password).map_err(text)?;
                let name = if name.trim().is_empty() { server.url.clone() } else { name.trim().to_owned() };
                let account = ServerAccount { name, url: server.url.clone(), user: server.user.clone() };
                let tx = self.tx.clone();
                crate::spawn("server", move || {
                    let result = server.ping().map_err(text);
                    tx.send(Event::ServerChecked(account, password, result)).ok();
                })?;
                self.checking += 1;
            }
            Command::ServerRemove { name } => {
                if let Some(a) = self.settings.servers.iter().find(|s| s.name == name) {
                    secrets::remove(&a.secret());
                }
                self.servers.clear();
                self.pages.clear();
                self.change_settings(|s| s.servers.retain(|a| a.name != name));
            }
            Command::SpotifySignIn { client_id } => return self.sign_in(&client_id),
            Command::SpotifySignOut => {
                Spotify::sign_out();
                if self.playing.as_ref().is_some_and(|p| p.remote) {
                    self.stop();
                }
                self.spotify = None;
                self.remote = None;
                self.spotify_name.clear();
                self.pages.clear();
            }
            Command::SpotifyDevice { id } => self.change_settings(|s| s.spotify_device = id),
            Command::SoloistStart { path, key } => self.start_soloist(&path, &key)?,
            Command::SoloistStop => self.soloist = None,
            Command::Refresh => self.pages.clear(),
            Command::LibraryOpen { path } => self.open_library(&path)?,
            Command::Songs { search, sort, descending, limit } => {
                let lib = self.lib_mut()?;
                let s = Search::new(&search);
                let mut list: Vec<&Track> = lib.tracks().filter(|t| s.matches(t)).collect();
                query::sort(&mut list, sort, descending);
                if limit > 0 {
                    list.truncate(limit);
                }
                return Ok(data(list));
            }
            Command::Albums => {
                let albums = query::albums(self.lib_mut()?.tracks());
                let rows: Vec<Value> = albums
                    .iter()
                    .map(|a| json!({ "title": a.title, "artist": a.artist, "year": a.year, "songs": a.tracks }))
                    .collect();
                return Ok(Value::Array(rows));
            }
            Command::Playlists => return Ok(data(self.lib_mut()?.playlists())),
            Command::PlaylistSongs { id } => {
                let items = self.playlist_items(&id).ok_or("there is no such playlist in the library")?;
                let rows: Vec<Value> = items.iter().map(|i| json!({ "item": i, "meta": self.meta(i) })).collect();
                return Ok(Value::Array(rows));
            }
            Command::Stations => return Ok(data(self.lib_mut()?.stations())),
            Command::Now => {
                let n = self.now();
                return Ok(json!({
                    "item": n.item, "meta": n.meta, "status": format!("{:?}", n.status).to_lowercase(),
                    "position_ms": n.position_ms, "stream_title": n.stream_title,
                }));
            }
            Command::Info => {
                let unreadable = self.unreadable.clone();
                let lib = self.lib_mut()?;
                return Ok(json!({
                    "folder": lib.root(), "songs": lib.len(), "playlists": lib.playlists().len(),
                    "computers": lib.devices(), "warnings": lib.warnings(), "unreadable": unreadable,
                }));
            }
        }
        done
    }

    /// The songs of a playlist, smart ones included, in order.
    pub fn playlist_items(&self, id: &str) -> Option<Vec<Item>> {
        let lib = self.lib.as_ref()?;
        let p = lib.playlist(id)?;
        Some(match &p.smart {
            Some(rules) => rules.select(lib.tracks(), now_ms()).iter().map(|t| Item::local(&t.id)).collect(),
            None => p.items.clone(),
        })
    }

    fn edit(&mut self, ids: &[String], tags: &TagEdit) -> Result<Value, String> {
        let mut failed = Vec::new();
        let mut keys = Vec::new();
        let lib = self.lib_mut()?;
        for id in ids {
            if let Some(t) = lib.track(id) {
                keys.push(format!("album:{}", t.album_key()));
            }
            let result = lib.edit(id, |i| {
                let set = |slot: &mut String, v: &Option<String>| {
                    if let Some(v) = v {
                        v.trim().clone_into(slot);
                    }
                };
                set(&mut i.title, &tags.title);
                set(&mut i.artist, &tags.artist);
                set(&mut i.album_artist, &tags.album_artist);
                set(&mut i.album, &tags.album);
                set(&mut i.genre, &tags.genre);
                set(&mut i.composer, &tags.composer);
                i.year = tags.year.unwrap_or(i.year);
                i.track = tags.track.unwrap_or(i.track);
                i.disc = tags.disc.unwrap_or(i.disc);
            });
            if let Err(e) = result {
                let title = lib.track(id).map(|t| t.info.title.clone()).unwrap_or_default();
                failed.push(format!("{title}: {e}"));
            }
            if let Some(t) = lib.track(id) {
                keys.push(format!("album:{}", t.album_key()));
            }
        }
        for k in keys {
            self.art.forget(&k);
        }
        match failed.first() {
            None => Ok(Value::Null),
            Some(first) => Err(format!("{} / {}: {first}", failed.len(), ids.len())),
        }
    }

    fn import(&mut self, paths: &[PathBuf]) -> Result<Value, String> {
        if self.import.is_some() {
            return Err("files are already being added".to_owned());
        }
        let music = self.lib_mut()?.music_dir();
        let files = musiccraft_library::import::collect(paths);
        let n = files.len();
        if n == 0 {
            return Err("no music files were found there".to_owned());
        }
        let progress = Arc::new(Progress::default());
        let (tx, p) = (self.tx.clone(), progress.clone());
        crate::spawn("import", move || {
            let r = musiccraft_library::import::copy_in(&music, &files, |done| p.read.store(done, Ordering::Relaxed));
            tx.send(Event::Import(r)).ok();
        })?;
        self.import = Some(progress);
        self.import_total = n;
        Ok(json!({ "files": n }))
    }

    /// Reads a playlist file; its entries are matched with the library by path, then by title.
    fn import_playlist(&mut self, path: &Path) -> Result<Value, String> {
        let bytes = std::fs::read(path).map_err(text)?;
        let entries = m3u::read(&bytes);
        let name = path.file_stem().and_then(|s| s.to_str()).unwrap_or("Playlist").to_owned();
        let lib = self.lib_mut()?;
        let by_path: HashMap<String, String> =
            lib.tracks().map(|t| (t.info.path.to_lowercase(), t.id.clone())).collect();
        let mut missing = 0usize;
        let mut items = Vec::new();
        for e in &entries {
            if radio::is_web(&e.location) {
                let meta = Meta { title: e.title.clone(), ..Meta::default() };
                items.push(Item { source: Source::Radio, id: e.location.clone(), meta: Some(meta) });
                continue;
            }
            let wanted = e.location.replace('\\', "/").to_lowercase();
            // The longest tail of the path that is a path in the library.
            let parts: Vec<&str> = wanted.split('/').collect();
            let found = (0..parts.len()).find_map(|i| by_path.get(&parts.get(i..)?.join("/")));
            match found {
                Some(id) => items.push(Item::local(id)),
                None => missing += 1,
            }
        }
        if items.is_empty() {
            return Err("none of its songs are in the library".to_owned());
        }
        let found = items.len();
        let id = lib.create_playlist(&name, items, None);
        Ok(json!({ "id": id, "songs": found, "missing": missing }))
    }

    fn export_playlist(&mut self, id: &str, path: &Path) -> Result<Value, String> {
        let items = self.playlist_items(id).ok_or("there is no such playlist in the library")?;
        let lib = self.lib.as_ref().ok_or("no library is open")?;
        let mut rows = Vec::new();
        let mut left_out = 0usize;
        for i in &items {
            let meta = self.meta(i);
            let location = match i.source {
                Source::Local => lib.file(&i.id).map(|p| p.to_string_lossy().into_owned()),
                Source::Radio => Some(i.id.clone()),
                // A service's song has no address another program could play.
                _ => None,
            };
            match location {
                Some(l) => {
                    let title =
                        if meta.artist.is_empty() { meta.title } else { format!("{} - {}", meta.artist, meta.title) };
                    rows.push((meta.duration_ms / 1000, title, l));
                }
                None => left_out += 1,
            }
        }
        let body = m3u::write(rows.iter().map(|(s, t, l)| (*s, t.as_str(), l.as_str())));
        craft_atomic::atomic_write(path, body.as_bytes()).map_err(text)?;
        Ok(json!({ "songs": rows.len(), "left_out": left_out }))
    }

    fn sign_in(&mut self, client_id: &str) -> Result<Value, String> {
        if self.sign_in_page.is_some() {
            return Ok(json!({ "page": self.sign_in_page }));
        }
        let s = spotify::sign_in(client_id, Hosts::default()).map_err(text)?;
        let page = s.page.clone();
        let (tx, id, store) = (self.tx.clone(), client_id.trim().to_owned(), self.use_secrets);
        crate::spawn("sign-in", move || {
            let result = s.finish(SIGN_IN_WAIT, store).map_err(text).and_then(|a| {
                let name = a.me().map_err(text)?;
                Ok((Arc::new(a), name))
            });
            tx.send(Event::SignedIn(id, result)).ok();
        })?;
        self.sign_in_page = Some(page.clone());
        Ok(json!({ "page": page }))
    }

    fn start_soloist(&mut self, path: &str, key: &str) -> Result<(), String> {
        let program = soloist::find(path).ok_or("Spotify Soloist was not found there")?;
        let key = if key.trim().is_empty() {
            secrets::get(SOLOIST_KEY).ok_or("Spotify Soloist needs your own key")?
        } else {
            secrets::set(SOLOIST_KEY, key.trim()).map_err(text)?;
            key.trim().to_owned()
        };
        self.soloist = Some(Soloist::start(&program, &Self::soloist_name(), &key).map_err(text)?);
        self.change_settings(|s| path.trim().clone_into(&mut s.soloist_path));
        self.pages.clear();
        Ok(())
    }
}

impl Drop for Engine {
    fn drop(&mut self) {
        self.save_now();
    }
}

/// A service's song as an item.
pub fn entry_item(source: Source, e: &Entry) -> Item {
    Item {
        source,
        id: e.id.clone(),
        meta: Some(Meta {
            title: e.title.clone(),
            artist: e.artist.clone(),
            album: e.album.clone(),
            duration_ms: e.duration_ms,
            art: e.art.clone(),
        }),
    }
}

fn millis(d: Duration) -> u64 {
    u64::try_from(d.as_millis()).unwrap_or(u64::MAX)
}

/// The next way of repeating.
pub fn next_repeat(r: Repeat) -> Repeat {
    match r {
        Repeat::Off => Repeat::All,
        Repeat::All => Repeat::One,
        Repeat::One => Repeat::Off,
    }
}

#[cfg(test)]
mod tests {
    use super::Engine;
    use crate::command::{Command, TagEdit};
    use musiccraft_library::Item;
    use musiccraft_library::query::Sort;
    use std::time::Duration;

    fn songs(e: &mut Engine, search: &str) -> Vec<serde_json::Value> {
        let c = Command::Songs { search: search.into(), sort: Sort::Title, descending: false, limit: 0 };
        e.run(c).unwrap().as_array().unwrap().clone()
    }

    /// From files elsewhere on disk to songs in the shared folder, edited, put in a playlist,
    /// exported and read back; adding the same files again adds nothing.
    #[test]
    fn import_edit_and_playlists_end_to_end() {
        let (outside, root, dir) =
            (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
        crate::demo::write_album(outside.path(), 0, 3, 1).unwrap();
        let mut e = Engine::isolated(dir.path(), Some(root.path().to_owned()));
        for _ in 0..2 {
            e.run(Command::Import { paths: vec![outside.path().to_owned()] }).unwrap();
            e.settle(Duration::from_secs(30));
        }
        let all = songs(&mut e, "");
        assert_eq!(all.len(), 3);
        let id = all[0]["id"].as_str().unwrap().to_owned();
        assert!(all[0]["duration_ms"].as_u64().unwrap() > 500);
        let tags = TagEdit { title: Some("Titolo nuovo".into()), year: Some(1999), ..TagEdit::default() };
        e.run(Command::Edit { ids: vec![id.clone()], tags }).unwrap();
        let list = e.run(Command::PlaylistNew { name: "Mia".into(), items: vec![Item::local(&id)] }).unwrap();
        let file = outside.path().join("Mia.m3u8");
        let list = list["id"].as_str().unwrap().to_owned();
        e.run(Command::PlaylistExport { id: list, path: file.clone() }).unwrap();
        assert_eq!(e.run(Command::PlaylistImport { path: file }).unwrap()["songs"], 1);
        e.settle(Duration::from_secs(30));
        // Another computer reads the edit from the file itself.
        let other = tempfile::tempdir().unwrap();
        let mut b = Engine::isolated(other.path(), Some(root.path().to_owned()));
        b.settle(Duration::from_secs(30));
        assert_eq!(songs(&mut b, "titolo nuovo")[0]["year"], 1999);
        assert_eq!(b.library().unwrap().playlists().len(), 2);
        let item = Item::local(&id);
        let meta = b.meta(&item);
        for _ in 0..500 {
            if b.picture(&item, &meta).is_some() {
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(b.picture(&item, &meta).is_some(), "the picture inside the file is shown");
    }

    /// Commands survive the trip through JSON that the command line takes them on.
    #[test]
    fn commands_are_data() {
        let text = r#"{"cmd":"play","items":[{"source":"local","id":"x"},{"source":"server","account":"casa","id":"9"}],"start":1}"#;
        let c: Command = serde_json::from_str(text).unwrap();
        assert!(matches!(&c, Command::Play { items, start: 1 } if items.len() == 2));
    }
}
