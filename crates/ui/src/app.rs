//! The window: what it remembers, and one frame of it.

use crate::chooser::{self, Want};
use crate::dialogs::{self, Dialog};
use crate::fonts::Fonts;
use crate::i18n::Lang;
use crate::table::{Row, State as TableState};
use crate::theme::{self, tokens};
use egui::{Key, Modifiers, TextureHandle};
use musiccraft_engine::{Command, Engine, NowStatus, Theme};
use musiccraft_library::smart::BuiltIn;
use musiccraft_library::{Item, Meta, Track, default_folder};
use musiccraft_services::Entry;
use musiccraft_services::subsonic::AlbumList;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

/// How the window is started.
#[derive(Debug, Clone, Default)]
pub struct Options {
    /// Show the demonstration library; nothing of the person's is read or changed.
    pub demo: bool,
    /// Save a picture of the window here, then end.
    pub screenshot: Option<PathBuf>,
    /// View to start with, for pictures: `albums`, `album`, `artists`, `songs`, `genres`, `radio`,
    /// `services`, `welcome`.
    pub view: String,
    /// `light` or `dark`, instead of the setting.
    pub theme: String,
    /// Language code, instead of the setting.
    pub language: String,
    /// Ask for paths in the window instead of showing the system's file chooser.
    pub type_paths: bool,
}

/// What the main area shows.
#[derive(Debug, Clone, PartialEq)]
pub enum View {
    RecentlyAdded,
    Artists,
    Albums,
    Songs,
    Genres,
    BuiltIn(BuiltIn),
    Playlist(String),
    Stations,
    RadioDirectory,
    Services,
    Server(String),
    ServerAlbum(String, Entry),
    ServerPlaylist(String, Entry),
    SpotifySaved,
    SpotifyPlaylist(Entry),
}

/// The tabs over the main area.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    Library,
    Radio,
    Streaming,
}

impl View {
    pub fn tab(&self) -> Tab {
        match self {
            View::Stations | View::RadioDirectory => Tab::Radio,
            View::Services
            | View::Server(_)
            | View::ServerAlbum(..)
            | View::ServerPlaylist(..)
            | View::SpotifySaved
            | View::SpotifyPlaylist(_) => Tab::Streaming,
            _ => Tab::Library,
        }
    }
}

/// What a frame asks for; done after it is drawn.
pub enum Action {
    Run(Command),
    Show(View),
    Open(Dialog),
    AddFiles,
    AddFolder,
    ChooseLibrary,
    ImportPlaylist,
    ImportSongs,
    ImportLibrary,
    ExportPlaylist(String, String),
    Reveal(String),
    Browse(String),
    SignIn(String),
    FocusSearch,
    Compact(bool),
    Dismiss,
    Quit,
}

pub struct App {
    pub engine: Engine,
    pub lang: Rc<Lang>,
    pub view: View,
    pub search: String,
    pub tables: HashMap<String, TableState>,
    rows: HashMap<String, (String, Rc<Vec<Row>>)>,
    /// The album shown open in a grid.
    pub open_album: Option<String>,
    /// The album just opened is brought into view.
    pub reveal_album: bool,
    pub artist: Option<String>,
    pub genre: Option<String>,
    pub radio_query: String,
    pub server_list: AlbumList,
    pub service_search: String,
    pub dialog: Option<Dialog>,
    pub show_queue: bool,
    pub compact: bool,
    pub actions: Vec<Action>,
    /// Where the seek slider is held.
    pub seeking: Option<u64>,
    pub focus_search: bool,
    textures: HashMap<String, TextureHandle>,
    fonts: Fonts,
    options: Options,
    frames: u32,
    shot: Arc<Mutex<Option<Result<(), String>>>>,
    shot_asked: bool,
    pub demo: bool,
    announced: String,
    /// A warning a dialog shows beside the focused field, spoken in this frame.
    pub(crate) warning: Option<String>,
}

/// Where the demonstration library is kept.
fn demo_dir() -> PathBuf {
    std::env::temp_dir().join(concat!("musiccraft-demo-", env!("CARGO_PKG_VERSION")))
}

impl App {
    pub fn new(ctx: &egui::Context, options: Options) -> App {
        theme::install(ctx);
        let real = musiccraft_engine::settings::data_dir();
        let mut engine = match (&real, options.demo) {
            (Some(dir), false) => Engine::new(dir),
            _ => Engine::isolated(&demo_dir(), None),
        };
        let mut demo = options.demo || real.is_none();
        // A picture of a computer without a library shows the demonstration one.
        if options.screenshot.is_some() && engine.library().is_none() && options.view != "welcome" {
            demo = true;
        }
        if demo && options.view != "welcome" {
            engine = Engine::isolated(&demo_dir(), None);
            "demonstration".clone_into(&mut engine.settings.device);
            let dir = demo_dir().join("library");
            match musiccraft_engine::demo::write_library(&dir) {
                Ok(()) => {
                    if let Err(e) = engine.run(Command::LibraryOpen { path: dir }) {
                        engine.error("The library could not be opened: {}", &e);
                    }
                    musiccraft_engine::demo::dress(&mut engine, options.screenshot.is_some());
                }
                Err(e) => engine.error("The demonstration library could not be written: {}", &e),
            }
        }
        let choice = if options.language.is_empty() { &engine.settings.language } else { &options.language };
        let lang = Rc::new(Lang::new(choice));
        let theme = match options.theme.as_str() {
            "light" => Theme::Light,
            "dark" => Theme::Dark,
            _ => engine.settings.theme,
        };
        set_theme(ctx, theme);
        let view = match options.view.as_str() {
            "albums" | "album" => View::Albums,
            "artists" => View::Artists,
            "songs" => View::Songs,
            "genres" => View::Genres,
            "radio" => View::RadioDirectory,
            "services" => View::Services,
            _ => View::RecentlyAdded,
        };
        // A picture of an open album, beside the queue.
        let open = options.view == "album";
        let open_album = engine
            .library()
            .filter(|_| open)
            .and_then(|l| {
                musiccraft_library::query::albums(l.tracks()).into_iter().find(|a| a.title == "Ground Station")
            })
            .map(|a| a.key);
        App {
            engine,
            lang,
            view,
            search: String::new(),
            tables: HashMap::new(),
            rows: HashMap::new(),
            open_album,
            reveal_album: open,
            artist: None,
            genre: None,
            radio_query: String::new(),
            server_list: AlbumList::Newest,
            service_search: String::new(),
            dialog: None,
            show_queue: open,
            compact: false,
            actions: Vec::new(),
            seeking: None,
            focus_search: false,
            textures: HashMap::new(),
            fonts: Fonts::default(),
            options,
            frames: 0,
            shot: Arc::default(),
            shot_asked: false,
            demo,
            announced: String::new(),
            warning: None,
        }
    }

    pub fn act(&mut self, a: Action) {
        self.actions.push(a);
    }

    pub fn cmd(&mut self, c: Command) {
        self.actions.push(Action::Run(c));
    }

    /// The texture of an item's picture, when it has arrived.
    pub fn texture(&mut self, ctx: &egui::Context, item: &Item, meta: &Meta) -> Option<egui::TextureId> {
        let key = self.engine.art_key(item, meta);
        // Asking each time keeps the picture among the recently used.
        let picture = self.engine.picture(item, meta)?;
        if let Some(t) = self.textures.get(&key) {
            return Some(t.id());
        }
        if self.textures.len() > 400 {
            self.textures.clear();
        }
        let size = [picture.width as usize, picture.height as usize];
        let image = egui::ColorImage::from_rgba_unmultiplied(size, &picture.rgba);
        let t = ctx.load_texture(&key, image, egui::TextureOptions::LINEAR);
        let id = t.id();
        self.textures.insert(key, t);
        Some(id)
    }

    /// Rows for a table, made again only when `stamp` changes.
    pub fn rows(&mut self, key: &str, stamp: String, make: impl FnOnce(&Engine) -> Vec<Row>) -> Rc<Vec<Row>> {
        if let Some((s, rows)) = self.rows.get(key)
            && *s == stamp
        {
            return rows.clone();
        }
        let changed_length = self.rows.get(key).map(|(_, r)| r.len());
        let rows = Rc::new(make(&self.engine));
        if changed_length != Some(rows.len())
            && let Some(t) = self.tables.get_mut(key)
        {
            t.reset();
        }
        if self.rows.len() > 40 {
            self.rows.clear();
        }
        self.rows.insert(key.to_owned(), (stamp, rows.clone()));
        rows
    }

    pub fn revision(&self) -> u64 {
        self.engine.library().map_or(0, musiccraft_library::Library::revision)
    }

    /// Plays rows of a table from one of them.
    pub fn play_rows(&mut self, rows: &[Row], at: usize) {
        let items: Vec<Item> = rows.iter().filter_map(|r| r.item.clone()).collect();
        let start = rows.iter().take(at).filter(|r| r.item.is_some()).count();
        self.cmd(Command::Play { items, start });
    }

    fn shortcuts(&mut self, ctx: &egui::Context) {
        if self.dialog.is_some() {
            return;
        }
        let typing = ctx.text_edit_focused();
        let free = ctx.memory(|m| m.focused()).is_none();
        let (cmd, shift) = (Modifiers::COMMAND, Modifiers::COMMAND | Modifiers::SHIFT);
        let mut hits: Vec<Action> = Vec::new();
        ctx.input_mut(|i| {
            let volume = self.engine.settings.volume;
            let mut on = |m: Modifiers, k: Key, a: Action| {
                if i.consume_key(m, k) {
                    hits.push(a);
                }
            };
            on(shift, Key::N, Action::Open(Dialog::smart_new()));
            on(cmd, Key::N, Action::Open(Dialog::new_playlist(Vec::new())));
            on(cmd, Key::O, Action::AddFiles);
            on(cmd, Key::F, Action::FocusSearch);
            on(cmd, Key::Comma, Action::Open(Dialog::settings()));
            on(Modifiers::NONE, Key::F5, Action::Run(Command::Scan));
            if !typing {
                on(cmd, Key::ArrowRight, Action::Run(Command::Next));
                on(cmd, Key::ArrowLeft, Action::Run(Command::Previous));
                on(cmd, Key::ArrowUp, Action::Run(Command::Volume { percent: volume.saturating_add(5).min(100) }));
                on(cmd, Key::ArrowDown, Action::Run(Command::Volume { percent: volume.saturating_sub(5) }));
            }
        });
        // Space belongs to a focused button; rows have no use for it.
        let on_row = self.tables.values().any(|t| t.has_focus(ctx));
        if !typing && (free || on_row) && ctx.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Space)) {
            hits.push(Action::Run(Command::PlayPause));
        }
        self.actions.extend(hits);
    }

    fn apply(&mut self, ctx: &egui::Context) {
        for a in std::mem::take(&mut self.actions) {
            match a {
                Action::Run(c) => {
                    if let Err(e) = self.engine.run(c) {
                        self.engine.error("{}", &e);
                    }
                }
                Action::Show(v) => {
                    self.view = v;
                    self.open_album = None;
                }
                Action::Open(d) => self.dialog = Some(d),
                Action::AddFiles => self.choose(Want::Add),
                Action::AddFolder => self.choose(Want::AddFolder),
                Action::ChooseLibrary => self.choose(Want::Library),
                Action::ImportPlaylist => self.choose(Want::Playlist),
                Action::ImportLibrary => self.choose(Want::LibraryFile),
                Action::ImportSongs => self.choose(Want::SongList),
                Action::ExportPlaylist(id, name) => self.choose(Want::SavePlaylist { id, name }),
                Action::Reveal(id) => {
                    if let Some(dir) =
                        self.engine.library().and_then(|l| l.file(&id)).and_then(|f| f.parent().map(Path::to_owned))
                    {
                        open_folder(&dir);
                    }
                }
                Action::Browse(url) => ctx.open_url(egui::OpenUrl::new_tab(url)),
                Action::SignIn(client_id) => match self.engine.run(Command::SpotifySignIn { client_id }) {
                    Ok(v) => {
                        if let Some(page) = v.get("page").and_then(serde_json::Value::as_str) {
                            ctx.open_url(egui::OpenUrl::new_tab(page));
                        }
                    }
                    Err(e) => self.engine.error("The Spotify sign-in did not work: {}", &e),
                },
                Action::FocusSearch => self.focus_search = true,
                Action::Compact(on) => {
                    self.compact = on;
                    let size = if on { egui::vec2(560.0, 118.0) } else { egui::vec2(1180.0, 760.0) };
                    let min = if on { egui::vec2(480.0, 100.0) } else { egui::vec2(760.0, 480.0) };
                    ctx.send_viewport_cmd(egui::ViewportCommand::MinInnerSize(min));
                    ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(size));
                }
                Action::Dismiss => {
                    self.engine.notices.pop();
                }
                Action::Quit => ctx.send_viewport_cmd(egui::ViewportCommand::Close),
            }
        }
    }

    /// Shows the system's file chooser, or, where there is none, a dialog that asks for the path.
    fn choose(&mut self, want: Want) {
        // Asked for in the window, the system is not probed at all.
        if self.options.type_paths {
            self.dialog = Some(Dialog::type_path(want, false));
            return;
        }
        if !chooser::available() {
            self.dialog = Some(Dialog::type_path(want, true));
            return;
        }
        let d = rfd::FileDialog::new();
        let picked: Vec<PathBuf> = match &want {
            Want::Add => {
                let d = d.add_filter(self.lang.t("Music"), musiccraft_library::AUDIO_EXTENSIONS);
                d.pick_files().unwrap_or_default()
            }
            Want::AddFolder => d.pick_folder().into_iter().collect(),
            Want::Library => {
                let mut d = d;
                if let Some(parent) = default_folder().as_deref().and_then(Path::parent) {
                    d = d.set_directory(parent);
                }
                d.pick_folder().into_iter().collect()
            }
            Want::Playlist => {
                d.add_filter(self.lang.t("Playlists"), &["m3u", "m3u8", "pls"]).pick_file().into_iter().collect()
            }
            Want::LibraryFile => {
                d.add_filter(self.lang.t("Exported library (XML)"), &["xml"]).pick_file().into_iter().collect()
            }
            Want::SongList => {
                let d = d.add_filter(self.lang.t("Song lists"), &["csv", "tsv", "txt"]);
                d.pick_file().into_iter().collect()
            }
            Want::SavePlaylist { name, .. } => d
                .add_filter(self.lang.t("Playlists"), &["m3u8"])
                .set_file_name(playlist_file(name))
                .save_file()
                .into_iter()
                .collect(),
        };
        self.chosen(want, picked);
    }

    /// Does what the paths were chosen for. None chosen is a cancel.
    pub(crate) fn chosen(&mut self, want: Want, paths: Vec<PathBuf>) {
        if matches!(want, Want::Add | Want::AddFolder) {
            self.add(paths);
            return;
        }
        let Some(path) = paths.into_iter().next() else { return };
        match want {
            Want::Add | Want::AddFolder => {}
            Want::Library => self.open_library(path),
            Want::Playlist => match self.engine.run(Command::PlaylistImport { path }) {
                Ok(v) => {
                    if let Some(id) = v.get("id").and_then(serde_json::Value::as_str) {
                        self.view = View::Playlist(id.to_owned());
                    }
                    let n = |k: &str| v.get(k).and_then(serde_json::Value::as_u64).unwrap_or(0);
                    if n("missing") > 0 {
                        self.engine.error("Songs of the playlist not in the library: {}", &n("missing").to_string());
                    }
                }
                Err(e) => self.engine.error("The playlist could not be read: {}", &e),
            },
            Want::LibraryFile => {
                if let Err(e) = self.engine.run(Command::ExportedRead { path }) {
                    self.engine.error("The library file could not be read: {}", &e);
                }
            }
            Want::SongList => {
                if let Err(e) = self.engine.run(Command::SongsImport { path }) {
                    self.engine.error("The song list could not be read: {}", &e);
                }
            }
            Want::SavePlaylist { id, .. } => match self.engine.run(Command::PlaylistExport { id, path }) {
                Ok(v) => {
                    let left = v.get("left_out").and_then(serde_json::Value::as_u64).unwrap_or(0);
                    if left > 0 {
                        self.engine.error("Songs left out because they play from a service: {}", &left.to_string());
                    }
                }
                Err(e) => self.engine.error("The playlist could not be written: {}", &e),
            },
        }
    }

    fn add(&mut self, paths: Vec<PathBuf>) {
        if paths.is_empty() {
            return;
        }
        if let Err(e) = self.engine.run(Command::Import { paths }) {
            self.engine.error("Some files could not be added: {}", &e);
        }
    }

    pub fn open_library(&mut self, path: PathBuf) {
        match self.engine.run(Command::LibraryOpen { path }) {
            Ok(_) => {
                self.demo = false;
                self.view = View::RecentlyAdded;
                self.rows.clear();
            }
            Err(e) => self.engine.error("The library could not be opened: {}", &e),
        }
    }

    /// The shared frame of every state of the window.
    fn frame(&mut self, ui: &mut egui::Ui) {
        let t = tokens(ui);
        let bar = |fill| egui::Frame::new().fill(fill).inner_margin(egui::Margin::symmetric(8, 4));
        if !self.compact {
            egui::Panel::top("menus").frame(bar(t.header)).show_separator_line(false).show(ui, |ui| self.menu_bar(ui));
        }
        egui::Panel::top("header").frame(bar(t.header)).show(ui, |ui| self.header(ui));
        if self.compact {
            return;
        }
        egui::Panel::bottom("status").frame(bar(t.sidebar)).show(ui, |ui| self.status_bar(ui));
        if self.engine.library().is_none() {
            egui::CentralPanel::default().show(ui, |ui| self.welcome(ui));
            return;
        }
        egui::Panel::top("tabs").frame(bar(t.sidebar)).show(ui, |ui| self.tabs(ui));
        egui::Panel::left("sidebar")
            .frame(egui::Frame::new().fill(t.sidebar).inner_margin(egui::Margin::symmetric(6, 8)))
            .default_size(theme::SIDEBAR)
            .size_range(150.0..=360.0)
            .resizable(true)
            .show(ui, |ui| self.sidebar(ui));
        if self.show_queue {
            egui::Panel::right("queue")
                .frame(egui::Frame::new().fill(t.sidebar).inner_margin(8))
                .default_size(290.0)
                .size_range(220.0..=460.0)
                .resizable(true)
                .show(ui, |ui| self.queue_panel(ui));
        }
        egui::CentralPanel::default().frame(egui::Frame::new().fill(t.content)).show(ui, |ui| self.content(ui));
    }

    fn screenshot(&mut self, ctx: &egui::Context) {
        let Some(path) = self.options.screenshot.clone() else { return };
        self.frames += 1;
        ctx.request_repaint();
        // By then the library is read and the pictures are there; a slow computer gets longer.
        let ready = !self.engine.is_busy() || self.frames > 600;
        if self.frames >= 120 && ready && !self.shot_asked {
            self.shot_asked = true;
            let done = self.shot.clone();
            ctx.request_screenshot(move |image| {
                let result = save_png(&path, &image);
                *done.lock().unwrap_or_else(PoisonError::into_inner) = Some(result);
            });
        }
        let result = self.shot.lock().unwrap_or_else(PoisonError::into_inner).take();
        if let Some(result) = result {
            if let Err(e) = result {
                eprintln!("the picture could not be saved: {e}");
                std::process::exit(1);
            }
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
    }

    /// Speaks a message without moving the focus.
    fn announce(&mut self, ui: &mut egui::Ui, text: &str) {
        let id = ui.scope_id().with("announce");
        let rect = egui::Rect::from_min_size(ui.max_rect().left_bottom(), egui::vec2(1.0, 1.0));
        let r = ui.interact(rect, id, egui::Sense::hover());
        r.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Status, true, text));
        ui.ctx().accesskit_node_builder(id, |b| b.set_live(egui::accesskit::Live::Polite));
        if self.announced != text {
            text.clone_into(&mut self.announced);
        }
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.engine.tick();
        if let Some(lib) = self.engine.library() {
            let texts = lib.tracks().flat_map(|t: &Track| [t.info.title.as_str(), &t.info.artist, &t.info.album]);
            self.fonts.check(&ctx, lib.revision(), texts);
        }
        self.shortcuts(&ctx);
        let dropped: Vec<PathBuf> = ctx.input(|i| i.raw.dropped_files.iter().map(|f| f.path().to_owned()).collect());
        if self.engine.library().is_some() {
            self.add(dropped);
        }
        self.frame(ui);
        if self.dialog.is_none()
            && let Some(i) = self.engine.incoming.take()
        {
            self.dialog = Some(Dialog::Incoming { incoming: i, copy: true });
        }
        dialogs::show(self, &ctx);
        let now = self.engine.now();
        let warning = self.warning.take();
        let spoken = match (now.status, self.engine.notices.last()) {
            _ if warning.is_some() => warning.unwrap_or_default(),
            (_, Some(n)) => self.lang.f(&n.text, self.lang.t(&n.detail)),
            (NowStatus::Playing, _) => self.lang.f2("Playing: {}, {}", &now.meta.title, &now.meta.artist),
            (NowStatus::Elsewhere, _) => self.lang.f("Opened in {}", &now.via),
            (NowStatus::Paused, _) => self.lang.t("Paused").to_owned(),
            _ => String::new(),
        };
        self.announce(ui, &spoken);
        self.apply(&ctx);
        for page in std::mem::take(&mut self.engine.to_open) {
            ctx.open_url(egui::OpenUrl::new_tab(page));
        }
        self.screenshot(&ctx);
        let playing = matches!(now.status, NowStatus::Playing | NowStatus::Loading);
        let wait = if playing || self.engine.is_busy() { 200 } else { 1000 };
        ctx.request_repaint_after(Duration::from_millis(wait));
    }

    fn on_exit(&mut self) {
        self.engine.save_now();
    }
}

pub fn set_theme(ctx: &egui::Context, theme: Theme) {
    ctx.set_theme(match theme {
        Theme::System => egui::ThemePreference::System,
        Theme::Light => egui::ThemePreference::Light,
        Theme::Dark => egui::ThemePreference::Dark,
    });
}

fn save_png(path: &Path, image: &egui::ColorImage) -> Result<(), String> {
    let mut rgba = Vec::with_capacity(image.pixels.len() * 4);
    for p in &image.pixels {
        rgba.extend([p.r(), p.g(), p.b(), 255]);
    }
    let (w, h) = (image.size[0] as u32, image.size[1] as u32);
    if let Some(dir) = path.parent() {
        // A missing folder is made; if it cannot be, saving says why.
        std::fs::create_dir_all(dir).ok();
    }
    image::save_buffer(path, &rgba, w, h, image::ColorType::Rgba8).map_err(|e| e.to_string())
}

/// The file name offered for a playlist that is saved.
pub(crate) fn playlist_file(name: &str) -> String {
    format!("{}.m3u8", musiccraft_library::import::safe(name, "Playlist"))
}

/// Opens a folder in the system's file manager.
fn open_folder(dir: &Path) {
    let program = if cfg!(windows) {
        "explorer"
    } else if cfg!(target_os = "macos") {
        "open"
    } else {
        "xdg-open"
    };
    // Nothing to do where there is no file manager.
    std::process::Command::new(program).arg(dir).spawn().ok();
}
