//! The main area.

use crate::app::{Action, App, View};
use crate::dialogs::Dialog;
use crate::icons::{self, Icon};
use crate::sidebar;
use crate::table::{self, Column, Event, Row, Table};
use crate::theme::{place, tokens};
use egui::{Align, Layout, Rect, Sense, Ui, UiBuilder, Vec2, vec2};
use musiccraft_engine::{Command, Engine};
use musiccraft_library::model::Presence;
use musiccraft_library::query::{self, Album, Search, Sort, sort_key};
use musiccraft_library::smart::BuiltIn;
use musiccraft_library::{Item, Source, Track, default_folder, now_ms};
use std::path::PathBuf;

/// `service`: the key of the streaming service in use.
pub fn track_row(t: &Track, service: &str) -> Row {
    // A song whose file is missing here plays from the service too.
    let presence = if t.presence.needs_service() && !service.is_empty() { Presence::Stream } else { t.presence };
    Row {
        item: Some(Item::local(&t.id)),
        number: 0,
        title: t.info.title.clone(),
        artist: t.info.artist.clone(),
        album: t.info.album.clone(),
        genre: t.info.genre.clone(),
        year: t.info.year,
        duration_ms: t.info.duration_ms,
        loved: t.loved,
        rating: t.rating,
        plays: t.plays,
        added: t.added,
        presence,
        lacking: t.presence.needs_service() && t.links.get(service).is_some_and(|l| l.id.is_empty()),
    }
}

/// The key of the streaming service in use, or nothing.
pub fn service_key(engine: &Engine) -> String {
    engine.active().map(|s| s.key()).unwrap_or_default()
}

fn item_row(engine: &Engine, item: &Item) -> Row {
    if item.source == Source::Local
        && let Some(t) = engine.library().and_then(|l| l.track(&item.id))
    {
        return track_row(t, &service_key(engine));
    }
    let m = engine.meta(item);
    let gone = item.source == Source::Local;
    Row {
        item: if gone { None } else { Some(item.clone()) },
        title: m.title,
        artist: m.artist,
        album: m.album,
        duration_ms: m.duration_ms,
        presence: if gone { Presence::Missing } else { Presence::Here },
        ..Row::default()
    }
}

/// How a table of songs behaves.
pub(crate) struct Kind<'a> {
    pub columns: &'a [Column],
    pub sortable: bool,
    /// The playlist whose order it shows.
    pub playlist: Option<&'a str>,
}

impl App {
    pub(crate) fn content(&mut self, ui: &mut Ui) {
        match self.view.clone() {
            View::RecentlyAdded => self.recent(ui),
            View::Albums => self.all_albums(ui),
            View::Songs => self.songs(ui),
            View::Artists => self.by_name(ui, true),
            View::Genres => self.by_name(ui, false),
            View::BuiltIn(b) => self.built_in(ui, b),
            View::Playlist(id) => self.playlist(ui, &id),
            View::Stations => self.stations(ui),
            View::RadioDirectory => self.radio_directory(ui),
            View::Services => self.services(ui),
            View::Server(name) => self.server(ui, &name),
            View::ServerAlbum(name, album) => self.server_album(ui, &name, &album),
            View::ServerPlaylist(name, list) => self.server_playlist(ui, &name, &list),
            View::SpotifySaved => self.spotify_saved(ui),
            View::SpotifyPlaylist(list) => self.spotify_playlist(ui, &list),
        }
    }

    /// The library is empty, or nothing matches the search.
    pub(crate) fn nothing(&mut self, ui: &mut Ui) {
        let t = tokens(ui);
        let empty = self.engine.library().is_none_or(musiccraft_library::Library::is_empty);
        let busy = self.engine.scan_progress().is_some();
        ui.add_space(ui.available_height() * 0.28);
        ui.vertical_centered(|ui| {
            let (r, _) = ui.allocate_exact_size(Vec2::splat(64.0), Sense::hover());
            icons::draw(ui.painter(), r, Icon::Note, t.line);
            ui.add_space(8.0);
            if busy && empty {
                ui.heading(self.lang.t("Reading the music folder…"));
            } else if empty && self.search.is_empty() {
                ui.heading(self.lang.t("The library is empty"));
                ui.label(egui::RichText::new(self.lang.t("Drop music files here, or add them.")).color(t.weak));
                ui.add_space(8.0);
                if ui.button(self.lang.t("Add Files to Library…")).clicked() {
                    self.act(Action::AddFiles);
                }
                if ui.button(self.lang.t("Add Folder to Library…")).clicked() {
                    self.act(Action::AddFolder);
                }
            } else {
                ui.heading(self.lang.t("Nothing found"));
            }
        });
    }

    fn albums_where(&self, keep: impl Fn(&Track) -> bool) -> Vec<Album> {
        let s = Search::new(&self.search);
        match self.engine.library() {
            Some(lib) => query::albums(lib.tracks().filter(|t| s.matches(t) && keep(t))),
            None => Vec::new(),
        }
    }

    fn recent(&mut self, ui: &mut Ui) {
        let mut albums = self.albums_where(|_| true);
        // Added together means within the same hour; inside it, the usual order holds.
        albums.sort_by_key(|a| std::cmp::Reverse(a.added / 3_600_000));
        self.album_grid(ui, "recent", &albums);
    }

    fn all_albums(&mut self, ui: &mut Ui) {
        let mut albums = self.albums_where(|_| true);
        albums.sort_by_cached_key(|a| (sort_key(&a.title), sort_key(&a.artist)));
        self.album_grid(ui, "albums", &albums);
    }

    /// A table of rows with its menu and events.
    pub(crate) fn song_table(&mut self, ui: &mut Ui, id: &str, rows: &[Row], kind: &Kind<'_>, name: &str) {
        if rows.is_empty() {
            self.nothing(ui);
            return;
        }
        let mut state = self.tables.remove(id).unwrap_or_default();
        let playing = self.engine.now().item;
        let lang = self.lang.clone();
        let table = Table {
            id,
            rows,
            columns: kind.columns,
            lang: &lang,
            playing: playing.as_ref(),
            sortable: kind.sortable,
            editable: kind.columns.contains(&Column::Loved),
            name,
        };
        let mut opened: Option<Vec<usize>> = None;
        let mut pending: Vec<Action> = Vec::new();
        let events = {
            let app = &mut *self;
            table.show(ui, &mut state, |ui, selected| {
                let items: Vec<Item> =
                    selected.iter().filter_map(|i| rows.get(*i)).filter_map(|r| r.item.clone()).collect();
                if ui.button(app.lang.t("Play")).clicked() {
                    opened = Some(selected.iter().copied().collect());
                }
                app.item_menu(ui, &items);
                if let Some(list) = kind.playlist {
                    ui.separator();
                    if ui.button(app.lang.t("Remove from Playlist")).clicked() {
                        pending.push(app.without(list, rows, selected));
                    }
                    let first = selected.first().copied().unwrap_or(0);
                    ui.add_enabled_ui(selected.len() == 1, |ui| {
                        if ui.button(app.lang.t("Move Up")).clicked() {
                            pending.extend(app.moved(list, first, false));
                        }
                        if ui.button(app.lang.t("Move Down")).clicked() {
                            pending.extend(app.moved(list, first, true));
                        }
                    });
                }
            })
        };
        self.actions.extend(pending);
        if let Some(first) = opened.and_then(|v| v.first().copied()) {
            self.play_rows(rows, first);
        }
        for e in events {
            let local = |i: usize| {
                rows.get(i).and_then(|r| r.item.as_ref()).filter(|i| i.source == Source::Local).map(|i| i.id.clone())
            };
            match e {
                Event::Play(i) => self.play_rows(rows, i),
                Event::Sort(s, d) => state.sort = Some((s, d)),
                Event::Love(i, on) => {
                    if let Some(id) = local(i) {
                        self.cmd(Command::Love { ids: vec![id], on });
                    }
                }
                Event::Rate(i, stars) => {
                    if let Some(id) = local(i) {
                        self.cmd(Command::Rate { ids: vec![id], stars });
                    }
                }
                Event::Delete => match kind.playlist {
                    Some(list) => {
                        let a = self.without(list, rows, &state.selected);
                        self.act(a);
                    }
                    None => {
                        let ids: Vec<String> = state.selected.iter().filter_map(|i| local(*i)).collect();
                        if !ids.is_empty() {
                            self.act(Action::Open(Dialog::Remove { ids }));
                        }
                    }
                },
            }
        }
        self.tables.insert(id.to_owned(), state);
    }

    /// The playlist without the chosen rows.
    fn without(&self, list: &str, rows: &[Row], selected: &std::collections::BTreeSet<usize>) -> Action {
        let items = self.engine.playlist_items(list).unwrap_or_default();
        let keep = if items.len() == rows.len() {
            items.into_iter().enumerate().filter(|(i, _)| !selected.contains(i)).map(|(_, x)| x).collect()
        } else {
            items
        };
        Action::Run(Command::PlaylistSet { id: list.to_owned(), items: keep })
    }

    fn moved(&self, list: &str, at: usize, later: bool) -> Option<Action> {
        let mut items = self.engine.playlist_items(list)?;
        let to = if later { at + 1 } else { at.checked_sub(1)? };
        if at >= items.len() || to >= items.len() {
            return None;
        }
        items.swap(at, to);
        Some(Action::Run(Command::PlaylistSet { id: list.to_owned(), items }))
    }

    /// Rows of the library's songs, searched and sorted.
    fn library_rows(
        &mut self,
        key: &str,
        default: (Sort, bool),
        extra: &str,
        keep: impl Fn(&Track) -> bool,
    ) -> std::rc::Rc<Vec<Row>> {
        let state = self.tables.entry(key.to_owned()).or_default();
        let (sort, desc) = *state.sort.get_or_insert(default);
        let service = service_key(&self.engine);
        let stamp = format!("{}|{}|{sort:?}|{desc}|{extra}|{service}", self.revision(), self.search);
        let search = Search::new(&self.search);
        self.rows(key, stamp, |e| {
            let Some(lib) = e.library() else { return Vec::new() };
            let mut list: Vec<&Track> = lib.tracks().filter(|t| search.matches(t) && keep(t)).collect();
            query::sort(&mut list, sort, desc);
            let service = service_key(e);
            list.into_iter().map(|t| track_row(t, &service)).collect()
        })
    }

    fn songs(&mut self, ui: &mut Ui) {
        if self.engine.settings.column_browser {
            self.column_browser(ui);
        }
        let (genre, artist) =
            if self.engine.settings.column_browser { (self.genre.clone(), self.artist.clone()) } else { (None, None) };
        let extra = format!("{genre:?}{artist:?}");
        let rows = self.library_rows("songs", (Sort::Artist, false), &extra, |t| {
            genre.as_ref().is_none_or(|g| sort_key(&t.info.genre) == sort_key(g))
                && artist.as_ref().is_none_or(|a| sort_key(t.album_artist()) == sort_key(a))
        });
        let name = self.lang.t("Songs");
        let kind = Kind { columns: table::SONGS, sortable: true, playlist: None };
        self.song_table(ui, "songs", &rows, &kind, name);
    }

    /// Genre and artist lists over the song table.
    fn column_browser(&mut self, ui: &mut Ui) {
        let (genres, artists) = match self.engine.library() {
            Some(lib) => {
                let genres = query::facet(lib.tracks(), |t| t.info.genre.as_str());
                let chosen = self.genre.clone();
                let in_genre =
                    lib.tracks().filter(|t| chosen.as_ref().is_none_or(|g| sort_key(&t.info.genre) == sort_key(g)));
                (genres, query::facet(in_genre, Track::album_artist))
            }
            None => return,
        };
        let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 148.0), Sense::hover());
        let half = rect.width() / 2.0;
        let t = tokens(ui);
        for (i, list) in [genres, artists].into_iter().enumerate() {
            let part = Rect::from_min_size(rect.min + vec2(i as f32 * half, 0.0), vec2(half - 1.0, rect.height()));
            let chosen = if i == 0 { self.genre.clone() } else { self.artist.clone() };
            let all = if i == 0 { self.lang.t("All genres") } else { self.lang.t("All artists") };
            let icon = if i == 0 { Icon::Tag } else { Icon::Person };
            let mut pick: Option<Option<String>> = None;
            place(ui, UiBuilder::new().max_rect(part).id_salt(i), |ui| {
                ui.spacing_mut().item_spacing.y = 0.0;
                egui::ScrollArea::vertical().id_salt(("browser", i)).auto_shrink([false, false]).show(ui, |ui| {
                    if sidebar::entry(ui, icon, all, chosen.is_none()).clicked() {
                        pick = Some(None);
                    }
                    for (name, n) in &list {
                        let shown = if name.is_empty() { self.lang.t("Unknown") } else { name };
                        let r = sidebar::entry(
                            ui,
                            icon,
                            &format!("{shown} ({n})"),
                            chosen.as_deref() == Some(name.as_str()),
                        );
                        if r.clicked() {
                            pick = Some(Some(name.clone()));
                        }
                    }
                });
            });
            match (i, pick) {
                (0, Some(p)) => {
                    self.genre = p;
                    self.artist = None;
                }
                (_, Some(p)) => self.artist = p,
                _ => {}
            }
        }
        ui.painter().vline(rect.left() + half, rect.y_range(), egui::Stroke::new(1.0, t.line));
        ui.painter().hline(rect.x_range(), rect.bottom(), egui::Stroke::new(1.0, t.line));
    }

    /// Artists or genres on the left, the albums of the chosen one on the right.
    fn by_name(&mut self, ui: &mut Ui, artists: bool) {
        let s = Search::new(&self.search);
        let list = match self.engine.library() {
            Some(lib) => {
                let found = lib.tracks().filter(|t| s.matches(t));
                if artists {
                    query::facet(found, Track::album_artist)
                } else {
                    query::facet(found, |t| t.info.genre.as_str())
                }
            }
            None => Vec::new(),
        };
        if list.is_empty() {
            self.nothing(ui);
            return;
        }
        let chosen = if artists { self.artist.clone() } else { self.genre.clone() };
        let chosen = chosen
            .filter(|c| list.iter().any(|(n, _)| n == c))
            .or_else(|| list.first().map(|(n, _)| n.clone()))
            .unwrap_or_default();
        let t = tokens(ui);
        let id = if artists { "artists" } else { "genres" };
        egui::Panel::left(id)
            .frame(egui::Frame::new().fill(t.content).inner_margin(6))
            .default_size(226.0)
            .size_range(150.0..=380.0)
            .resizable(true)
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 0.0;
                let h = crate::theme::ROW + 1.0;
                egui::ScrollArea::vertical().auto_shrink([false, false]).show_rows(ui, h, list.len(), |ui, range| {
                    for i in range {
                        let Some((name, _)) = list.get(i) else { continue };
                        let unknown = if artists { self.lang.t("Unknown Artist") } else { self.lang.t("Unknown") };
                        let shown = if name.is_empty() { unknown } else { name };
                        let icon = if artists { Icon::Person } else { Icon::Tag };
                        if ui.push_id(i, |ui| sidebar::entry(ui, icon, shown, *name == chosen)).inner.clicked() {
                            let slot = if artists { &mut self.artist } else { &mut self.genre };
                            *slot = Some(name.clone());
                        }
                    }
                });
            });
        let key = sort_key(&chosen);
        let albums =
            self.albums_where(|t| key == sort_key(if artists { t.album_artist() } else { t.info.genre.as_str() }));
        egui::ScrollArea::vertical().id_salt((id, &chosen)).auto_shrink([false, false]).show(ui, |ui| {
            let width = ui.available_width();
            egui::Frame::new().inner_margin(18).show(ui, |ui| {
                let title = if chosen.is_empty() { self.lang.t("Unknown").to_owned() } else { chosen.clone() };
                ui.label(egui::RichText::new(title).size(26.0));
                let songs: usize = albums.iter().map(|a| a.tracks.len()).sum();
                ui.horizontal(|ui| {
                    let n = self.lang.n(albums.len(), "{} album", "{} albums");
                    let m = self.lang.n(songs, "{} song", "{} songs");
                    ui.label(egui::RichText::new(format!("{n} · {m}")).color(t.weak));
                    let items = |s: &[Album]| -> Vec<Item> {
                        s.iter().flat_map(|a| &a.tracks).map(|id| Item::local(id)).collect()
                    };
                    if ui.button(self.lang.t("Play All")).clicked() {
                        self.cmd(Command::Play { items: items(&albums), start: 0 });
                    }
                    if ui.button(self.lang.t("Shuffle All")).clicked() {
                        self.cmd(Command::Shuffle { on: true });
                        self.cmd(Command::Play { items: items(&albums), start: 0 });
                    }
                });
                ui.add_space(14.0);
                for a in &albums {
                    let inner = width - 50.0;
                    let columns = if inner - 168.0 > 540.0 && a.tracks.len() > 1 { 2 } else { 1 };
                    let h = (a.tracks.len().div_ceil(columns) as f32 * crate::theme::ROW + 90.0).max(160.0);
                    let (rect, _) = ui.allocate_exact_size(vec2(inner, h), Sense::hover());
                    if ui.is_rect_visible(rect) {
                        place(ui, UiBuilder::new().max_rect(rect).id_salt(&a.key), |ui| {
                            self.album_block(ui, a, rect, 150.0);
                        });
                    }
                    ui.add_space(10.0);
                    ui.separator();
                    ui.add_space(10.0);
                }
            });
        });
    }

    /// Title and buttons over a list of songs.
    pub(crate) fn list_header(&mut self, ui: &mut Ui, title: &str, rows: &[Row]) {
        let t = tokens(ui);
        egui::Frame::new().inner_margin(egui::Margin::symmetric(18, 12)).show(ui, |ui| {
            ui.label(egui::RichText::new(title).size(24.0));
            ui.horizontal(|ui| {
                let ms = rows.iter().map(|r| r.duration_ms).sum();
                let n = self.lang.n(rows.len(), "{} song", "{} songs");
                let text = if ms > 0 { format!("{n} · {}", self.long_time(ms)) } else { n };
                ui.label(egui::RichText::new(text).color(t.weak));
                ui.add_enabled_ui(!rows.is_empty(), |ui| {
                    if ui.button(self.lang.t("Play")).clicked() {
                        self.play_rows(rows, 0);
                    }
                    if ui.button(self.lang.t("Play Shuffled")).clicked() {
                        self.cmd(Command::Shuffle { on: true });
                        self.play_rows(rows, 0);
                    }
                    let of_service = |r: &Row| r.item.as_ref().is_some_and(|i| i.source.key().is_some());
                    if rows.iter().any(of_service) && ui.button(self.lang.t("Add All to Library")).clicked() {
                        let items = rows.iter().filter_map(|r| r.item.clone()).collect();
                        self.cmd(Command::LibraryAdd { items });
                    }
                });
            });
        });
    }

    fn built_in(&mut self, ui: &mut Ui, b: BuiltIn) {
        let key = format!("builtin{b:?}");
        // The lists by date change as time passes.
        let service = service_key(&self.engine);
        let stamp = format!("{}|{}|{}|{service}", self.revision(), self.search, now_ms() / 60_000);
        let search = Search::new(&self.search);
        let rows = self.rows(&key, stamp, |e| {
            let Some(lib) = e.library() else { return Vec::new() };
            b.rules()
                .select(lib.tracks().filter(|t| search.matches(t)), now_ms())
                .into_iter()
                .map(|t| track_row(t, &service))
                .collect()
        });
        let name = self.built_in_name(b);
        self.list_header(ui, name, &rows);
        let columns = if b == BuiltIn::RecentlyPlayed { table::RECENT } else { table::LISTED };
        self.song_table(ui, &key, &rows, &Kind { columns, sortable: false, playlist: None }, name);
    }

    fn playlist(&mut self, ui: &mut Ui, id: &str) {
        let Some((name, smart)) =
            self.engine.library().and_then(|l| l.playlist(id)).map(|p| (p.name.clone(), p.smart.is_some()))
        else {
            self.act(Action::Show(View::RecentlyAdded));
            return;
        };
        let key = format!("list{id}");
        let stamp = format!("{}|{}|{}", self.revision(), self.search, service_key(&self.engine));
        let words = musiccraft_library::model::fold(&self.search);
        let rows = self.rows(&key, stamp, |e| {
            let items = e.playlist_items(id).unwrap_or_default();
            let all = items.iter().map(|i| item_row(e, i));
            all.filter(|r| {
                let text = musiccraft_library::model::fold(&format!("{} {} {}", r.title, r.artist, r.album));
                words.split_whitespace().all(|w| text.contains(w))
            })
            .collect()
        });
        ui.horizontal(|ui| {
            ui.vertical(|ui| self.list_header(ui, &name, &rows));
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.add_space(14.0);
                ui.menu_button(self.lang.t("Playlist"), |ui| self.playlist_menu(ui, id, &name, smart));
            });
        });
        if rows.is_empty() && self.search.is_empty() {
            let t = tokens(ui);
            ui.vertical_centered(|ui| {
                ui.add_space(40.0);
                let text = if smart {
                    self.lang.t("No song matches the rules.")
                } else {
                    self.lang.t("Right-click songs and choose Add to Playlist.")
                };
                ui.label(egui::RichText::new(text).color(t.weak));
            });
            return;
        }
        // Moving and removing need the whole list on screen.
        let editable = !smart && self.search.is_empty();
        let kind = Kind { columns: table::LISTED, sortable: false, playlist: editable.then_some(id) };
        self.song_table(ui, &key, &rows, &kind, &name);
    }

    /// Before there is a library.
    pub(crate) fn welcome(&mut self, ui: &mut Ui) {
        let t = tokens(ui);
        let folder: Option<PathBuf> = default_folder();
        ui.add_space(ui.available_height() * 0.16);
        ui.vertical_centered(|ui| {
            let (r, _) = ui.allocate_exact_size(Vec2::splat(72.0), Sense::hover());
            icons::draw(ui.painter(), r, Icon::Waves, t.accent);
            ui.add_space(10.0);
            ui.label(egui::RichText::new(self.lang.t("Welcome to MusicCraft")).size(28.0));
            ui.add_space(6.0);
            ui.set_max_width(560.0);
            ui.label(self.lang.t("Your music and your library stay together in one folder that you choose."));
            ui.label(
                egui::RichText::new(self.lang.t(
                    "Put it in OneDrive, Google Drive or another synced folder to share it between your computers.",
                ))
                .color(t.weak),
            );
            ui.add_space(16.0);
            if let Some(folder) = folder {
                ui.label(egui::RichText::new(folder.display().to_string()).monospace());
                ui.add_space(6.0);
                let start = egui::Button::new(
                    egui::RichText::new(self.lang.t("Use This Folder")).size(16.0).color(t.on_selection),
                )
                .fill(t.selection)
                .min_size(vec2(220.0, 34.0));
                if ui.add(start).clicked() {
                    self.act(Action::Run(Command::LibraryOpen { path: folder }));
                }
                ui.add_space(4.0);
            }
            if ui.button(self.lang.t("Choose Another Folder…")).clicked() {
                self.act(Action::ChooseLibrary);
            }
        });
    }

    /// A bar with the list's name over a page, with a way back.
    pub(crate) fn back(&mut self, ui: &mut Ui, to: View) {
        egui::Frame::new().inner_margin(egui::Margin { left: 18, right: 18, top: 10, bottom: 0 }).show(ui, |ui| {
            if ui.button(self.lang.t("Back")).clicked() {
                self.act(Action::Show(to));
            }
        });
    }
}
