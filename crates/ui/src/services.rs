//! Radio and streaming services.

use crate::albums::Tile;
use crate::app::{Action, App, View};
use crate::dialogs::Dialog;
use crate::table::{self, Row};
use crate::theme::{TILE, tokens};
use crate::views::Kind;
use egui::{Rect, Ui, vec2};
use musiccraft_engine::Command;
use musiccraft_engine::core::{Engine, entry_item};
use musiccraft_engine::pages::{Content, Page, PageState};
use musiccraft_library::{Meta, Source};
use musiccraft_services::Entry;
use musiccraft_services::subsonic::AlbumList;
use std::sync::Arc;

fn rows(source: &Source, entries: &[Entry]) -> Vec<Row> {
    entries
        .iter()
        .map(|e| Row {
            item: Some(entry_item(source.clone(), e)),
            number: e.track,
            title: e.title.clone(),
            artist: e.artist.clone(),
            album: e.album.clone(),
            year: e.year,
            duration_ms: e.duration_ms,
            ..Row::default()
        })
        .collect()
}

impl App {
    /// The content of a page, or a message while it is not there.
    fn page(&mut self, ui: &mut Ui, page: &Page) -> Option<Arc<Content>> {
        let t = tokens(ui);
        match self.engine.page(page) {
            PageState::Ready(c) => Some(c),
            PageState::Loading => {
                ui.vertical_centered(|ui| {
                    ui.add_space(60.0);
                    ui.spinner();
                    ui.label(self.lang.t("Asking the service…"));
                });
                None
            }
            PageState::Failed(e) => {
                ui.vertical_centered(|ui| {
                    ui.add_space(60.0);
                    ui.label(egui::RichText::new(self.lang.t("The service did not answer as expected.")).size(18.0));
                    ui.label(egui::RichText::new(self.lang.t(&e)).color(t.error));
                    ui.add_space(6.0);
                    if ui.button(self.lang.t("Try Again")).clicked() {
                        self.cmd(Command::Refresh);
                    }
                });
                None
            }
        }
    }

    fn entries(&mut self, ui: &mut Ui, key: &str, title: &str, source: Source, songs: &[Entry]) {
        let rows = rows(&source, songs);
        self.list_header(ui, title, &rows);
        let kind = Kind { columns: table::SERVICE, sortable: false, playlist: None };
        self.song_table(ui, key, &rows, &kind, title);
    }

    // ---- radio ----

    pub(crate) fn stations(&mut self, ui: &mut Ui) {
        let mine: Vec<(String, Entry)> = self
            .engine
            .library()
            .map(|l| {
                l.stations()
                    .iter()
                    .map(|s| {
                        let e = Entry {
                            id: s.url.clone(),
                            title: s.name.clone(),
                            artist: s.detail.clone(),
                            ..Entry::default()
                        };
                        (s.id.clone(), e)
                    })
                    .collect()
            })
            .unwrap_or_default();
        let t = tokens(ui);
        egui::Frame::new().inner_margin(18).show(ui, |ui| {
            ui.label(egui::RichText::new(self.lang.t("My Stations")).size(24.0));
            if mine.is_empty() {
                ui.add_space(10.0);
                ui.label(
                    egui::RichText::new(self.lang.t("Find stations in the directory and keep the ones you like."))
                        .color(t.weak),
                );
                ui.add_space(6.0);
                if ui.button(self.lang.t("Find Stations")).clicked() {
                    self.act(Action::Show(View::RadioDirectory));
                }
                return;
            }
            egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                for (id, e) in &mine {
                    ui.push_id(id, |ui| {
                        ui.horizontal(|ui| {
                            let play = self.lang.f("Play {}", &e.title);
                            let r = ui.button(self.lang.t("Play"));
                            r.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, &play));
                            if r.clicked() {
                                let item = entry_item(Source::Radio, e);
                                self.cmd(Command::Play { items: vec![item], start: 0 });
                            }
                            let remove = self.lang.f("Remove {}", &e.title);
                            let r = ui.button(self.lang.t("Remove"));
                            r.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, &remove));
                            if r.clicked() {
                                self.cmd(Command::StationRemove { id: id.clone() });
                            }
                            ui.vertical(|ui| {
                                ui.label(&e.title);
                                ui.label(egui::RichText::new(&e.artist).color(t.weak).size(13.0));
                            });
                        });
                        ui.separator();
                    });
                }
            });
        });
    }

    pub(crate) fn radio_directory(&mut self, ui: &mut Ui) {
        let t = tokens(ui);
        egui::Frame::new().inner_margin(egui::Margin::symmetric(18, 12)).show(ui, |ui| {
            ui.label(egui::RichText::new(self.lang.t("Find Stations")).size(24.0));
            let note = self
                .lang
                .t("From Radio Browser, a community directory. Type above and press Enter. Double-click plays.");
            ui.label(egui::RichText::new(note).color(t.weak));
        });
        if self.options_offline() {
            return;
        }
        let Some(c) = self.page(ui, &Page::Radio(self.radio_query.clone())) else { return };
        let rows = rows(&Source::Radio, &c.songs);
        let selected = self.tables.get("directory").map(|s| s.selected.clone()).unwrap_or_default();
        ui.horizontal(|ui| {
            ui.add_space(18.0);
            ui.add_enabled_ui(!selected.is_empty(), |ui| {
                if ui.button(self.lang.t("Keep in My Stations")).clicked() {
                    for e in selected.iter().filter_map(|i| c.songs.get(*i)) {
                        self.cmd(Command::StationAdd {
                            name: e.title.clone(),
                            url: e.id.clone(),
                            detail: e.artist.clone(),
                        });
                    }
                }
            });
        });
        let name = self.lang.t("Find Stations");
        let kind = Kind { columns: table::STATIONS, sortable: false, playlist: None };
        self.song_table(ui, "directory", &rows, &kind, name);
    }

    // ---- personal servers ----

    pub(crate) fn server(&mut self, ui: &mut Ui, name: &str) {
        if !self.service_search.is_empty() {
            let Some(c) = self.page(ui, &Page::ServerSearch(name.to_owned(), self.service_search.clone())) else {
                return;
            };
            let title = self.lang.f("Results for “{}”", &self.service_search);
            let source = Source::Server { account: name.to_owned() };
            self.entries(ui, &format!("search{name}"), &title, source, &c.songs);
            return;
        }
        egui::Frame::new().inner_margin(egui::Margin::symmetric(18, 12)).show(ui, |ui| {
            ui.label(egui::RichText::new(name).size(24.0));
            ui.horizontal(|ui| {
                for (list, text) in [
                    (AlbumList::Newest, self.lang.t("Newest")),
                    (AlbumList::Recent, self.lang.t("Recently Played")),
                    (AlbumList::Frequent, self.lang.t("Most Played")),
                    (AlbumList::ByArtist, self.lang.t("By Artist")),
                    (AlbumList::Random, self.lang.t("Random")),
                ] {
                    ui.selectable_value(&mut self.server_list, list, text);
                }
            });
        });
        let Some(c) = self.page(ui, &Page::ServerAlbums(name.to_owned(), self.server_list)) else { return };
        if c.albums.is_empty() {
            self.nothing(ui);
            return;
        }
        let tiles: Vec<(Tile, Entry)> = c
            .albums
            .iter()
            .map(|a| {
                let item = entry_item(Source::Server { account: name.to_owned() }, a);
                let meta = Meta { art: a.art.clone(), ..Meta::default() };
                let tile = Tile { key: a.id.clone(), title: a.title.clone(), sub: a.artist.clone(), item, meta };
                (tile, a.clone())
            })
            .collect();
        let width = ui.available_width() - 28.0;
        let columns = (((width + 18.0) / (TILE + 18.0)).floor() as usize).max(1);
        let side = ((width + 18.0) / columns as f32 - 18.0).min(TILE * 1.25);
        let height = side + 60.0;
        egui::ScrollArea::vertical().id_salt(("server", name)).auto_shrink([false, false]).show_rows(
            ui,
            height,
            tiles.len().div_ceil(columns),
            |ui, range| {
                for row in range {
                    let (line, _) = ui.allocate_exact_size(vec2(width, height), egui::Sense::hover());
                    for col in 0..columns {
                        let Some((tile, album)) = tiles.get(row * columns + col) else { break };
                        let at = line.min + vec2(14.0 + col as f32 * (side + 18.0), 8.0);
                        let rect = Rect::from_min_size(at, vec2(side, side + 42.0));
                        if self.tile(ui, rect, side, tile, false).clicked() {
                            self.act(Action::Show(View::ServerAlbum(name.to_owned(), album.clone())));
                        }
                    }
                }
            },
        );
    }

    pub(crate) fn server_album(&mut self, ui: &mut Ui, name: &str, album: &Entry) {
        self.back(ui, View::Server(name.to_owned()));
        let Some(c) = self.page(ui, &Page::ServerAlbum(name.to_owned(), album.id.clone())) else { return };
        let title = format!("{} — {}", album.title, album.artist);
        self.entries(ui, &album.id, &title, Source::Server { account: name.to_owned() }, &c.songs);
    }

    pub(crate) fn server_playlist(&mut self, ui: &mut Ui, name: &str, list: &Entry) {
        let Some(c) = self.page(ui, &Page::ServerPlaylist(name.to_owned(), list.id.clone())) else { return };
        self.entries(ui, &list.id, &list.title, Source::Server { account: name.to_owned() }, &c.songs);
    }

    // ---- Spotify ----

    pub(crate) fn spotify_saved(&mut self, ui: &mut Ui) {
        let search = self.service_search.clone();
        let (page, title) = if search.is_empty() {
            (Page::SpotifySaved, self.lang.t("Saved Songs").to_owned())
        } else {
            (Page::SpotifySearch(search.clone()), self.lang.f("Results for “{}”", &search))
        };
        let Some(c) = self.page(ui, &page) else { return };
        self.entries(ui, "spotify", &title, Source::Spotify, &c.songs);
    }

    pub(crate) fn spotify_playlist(&mut self, ui: &mut Ui, list: &Entry) {
        if !self.service_search.is_empty() {
            self.spotify_saved(ui);
            return;
        }
        let Some(c) = self.page(ui, &Page::SpotifyPlaylist(list.id.clone())) else { return };
        self.entries(ui, &list.id, &list.title, Source::Spotify, &c.songs);
    }

    // ---- accounts ----

    pub(crate) fn services(&mut self, ui: &mut Ui) {
        let t = tokens(ui);
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            egui::Frame::new().inner_margin(22).show(ui, |ui| {
                ui.set_max_width(760.0);
                ui.label(egui::RichText::new(self.lang.t("Streaming Accounts")).size(24.0));
                ui.label(
                    egui::RichText::new(self.lang.t(
                        "Passwords and tokens stay in this computer's credential store, never in the shared library folder.",
                    ))
                    .color(t.weak),
                );
                ui.add_space(14.0);
                self.card(ui, |app, ui| app.server_accounts(ui));
                ui.add_space(12.0);
                self.card(ui, |app, ui| app.spotify_account(ui));
                ui.add_space(12.0);
                self.card(ui, |app, ui| {
                    ui.label(egui::RichText::new(app.lang.t("Other services")).size(18.0));
                    ui.label(app.lang.t(
                        "Apple Music, TIDAL, Amazon Music and YouTube Music are not supported: MusicCraft uses only official interfaces that are open to independent desktop programs.",
                    ));
                });
            });
        });
    }

    fn card(&mut self, ui: &mut Ui, add: impl FnOnce(&mut App, &mut Ui)) {
        let t = tokens(ui);
        egui::Frame::new()
            .fill(t.sidebar)
            .stroke(egui::Stroke::new(1.0, t.line))
            .corner_radius(8)
            .inner_margin(16)
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                add(self, ui);
            });
    }

    fn server_accounts(&mut self, ui: &mut Ui) {
        let t = tokens(ui);
        ui.label(egui::RichText::new(self.lang.t("Personal music servers")).size(18.0));
        ui.label(
            egui::RichText::new(self.lang.t("Navidrome, Gonic, Airsonic and others that speak the Subsonic API."))
                .color(t.weak),
        );
        ui.add_space(6.0);
        let servers = self.engine.settings.servers.clone();
        for s in servers {
            ui.horizontal(|ui| {
                ui.label(&s.name);
                ui.label(egui::RichText::new(format!("{} · {}", s.url, s.user)).color(t.weak));
                if ui.button(self.lang.t("Open")).clicked() {
                    self.act(Action::Show(View::Server(s.name.clone())));
                }
                if ui.button(self.lang.t("Remove")).clicked() {
                    self.cmd(Command::ServerRemove { name: s.name.clone() });
                }
            });
        }
        if ui.button(self.lang.t("Add a Server…")).clicked() {
            self.act(Action::Open(Dialog::server()));
        }
    }

    fn spotify_account(&mut self, ui: &mut Ui) {
        let t = tokens(ui);
        ui.label(egui::RichText::new("Spotify").size(18.0));
        ui.label(
            egui::RichText::new(self.lang.t(
                "Experimental, and needs Premium. MusicCraft shows your library and tells a Spotify player what to play: the Spotify app on any device, or Spotify Soloist on this computer.",
            ))
            .color(t.weak),
        );
        ui.add_space(6.0);
        if self.engine.sign_in_page.is_some() {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label(self.lang.t("Waiting for the sign-in in the browser…"));
            });
            return;
        }
        if self.engine.spotify().is_none() {
            ui.label(self.lang.t("1. At developer.spotify.com create an app of your own (it is free)."));
            ui.horizontal(|ui| {
                ui.label(self.lang.t("2. As its redirect address enter"));
                ui.add(egui::Label::new(egui::RichText::new("http://127.0.0.1/callback").monospace()).selectable(true));
            });
            ui.label(self.lang.t("3. Paste its Client ID here and sign in."));
            ui.horizontal(|ui| {
                let label = ui.label(self.lang.t("Client ID"));
                let mut id = self.engine.settings.spotify_client_id.clone();
                let r = ui.add(egui::TextEdit::singleline(&mut id).desired_width(300.0)).labelled_by(label.id);
                if r.changed() {
                    self.engine.settings.spotify_client_id.clone_from(&id);
                }
                ui.add_enabled_ui(!id.trim().is_empty(), |ui| {
                    if ui.button(self.lang.t("Sign In…")).clicked() {
                        self.act(Action::SignIn(id.clone()));
                    }
                });
                if ui.button(self.lang.t("Open the Page")).clicked() {
                    self.act(Action::Browse("https://developer.spotify.com/dashboard".to_owned()));
                }
            });
            return;
        }
        ui.horizontal(|ui| {
            let who = match self.engine.spotify_name.as_str() {
                "" => self.lang.t("Signed in").to_owned(),
                name => self.lang.f("Signed in as {}", name),
            };
            ui.label(who);
            if ui.button(self.lang.t("Open")).clicked() {
                self.act(Action::Show(View::SpotifySaved));
            }
            if ui.button(self.lang.t("Sign Out")).clicked() {
                self.cmd(Command::SpotifySignOut);
            }
        });
        ui.add_space(6.0);
        self.spotify_device(ui);
        ui.add_space(6.0);
        self.soloist(ui);
    }

    fn spotify_device(&mut self, ui: &mut Ui) {
        let devices = match self.engine.page(&Page::SpotifyDevices) {
            PageState::Ready(c) => c.devices.clone(),
            _ => Vec::new(),
        };
        let chosen = self.engine.settings.spotify_device.clone();
        let auto = self.lang.t("The one in use");
        let shown = devices.iter().find(|d| d.id == chosen).map_or(auto, |d| d.name.as_str()).to_owned();
        ui.horizontal(|ui| {
            let label = ui.label(self.lang.t("Play on"));
            egui::ComboBox::from_id_salt("device")
                .selected_text(shown)
                .show_ui(ui, |ui| {
                    if ui.selectable_label(chosen.is_empty(), auto).clicked() {
                        self.cmd(Command::SpotifyDevice { id: String::new() });
                    }
                    for d in &devices {
                        if ui.selectable_label(d.id == chosen, &d.name).clicked() {
                            self.cmd(Command::SpotifyDevice { id: d.id.clone() });
                        }
                    }
                })
                .response
                .labelled_by(label.id);
            if ui.button(self.lang.t("Look Again")).clicked() {
                self.cmd(Command::Refresh);
            }
        });
    }

    fn soloist(&mut self, ui: &mut Ui) {
        let t = tokens(ui);
        ui.label(self.lang.t("Sound from this computer without the Spotify app:"));
        if self.engine.soloist_runs() {
            ui.horizontal(|ui| {
                ui.label(self.lang.f("Spotify Soloist runs as “{}”.", Engine::soloist_name()));
                if ui.button(self.lang.t("Stop")).clicked() {
                    self.cmd(Command::SoloistStop);
                }
            });
            ui.label(
                egui::RichText::new(
                    self.lang
                        .t("The first time, choose it once among the devices in a Spotify app on the same network."),
                )
                .color(t.weak),
            );
        } else if ui.button(self.lang.t("Set Up Spotify Soloist…")).clicked() {
            self.act(Action::Open(Dialog::soloist(&self.engine.settings.soloist_path)));
        }
    }

    fn options_offline(&self) -> bool {
        // Pictures of the demonstration make no requests.
        self.demo && self.engine.library().is_some() && self.radio_query.is_empty()
    }
}
