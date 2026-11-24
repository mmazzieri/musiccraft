//! The menu bar and the menus of songs.

use crate::app::{Action, App, View};
use crate::dialogs::Dialog;
use egui::{Key, KeyboardShortcut, Modifiers, Ui};
use musiccraft_engine::core::next_repeat;
use musiccraft_engine::{Command, NowStatus};
use musiccraft_library::{Item, Source};

fn keys(ui: &Ui, m: Modifiers, k: Key) -> String {
    ui.ctx().format_shortcut(&KeyboardShortcut::new(m, k))
}

impl App {
    fn pick(&mut self, ui: &mut Ui, text: &str, shortcut: Option<(Modifiers, Key)>, action: Action) {
        let mut b = egui::Button::new(text);
        if let Some((m, k)) = shortcut {
            b = b.shortcut_text(keys(ui, m, k));
        }
        if ui.add(b).clicked() {
            self.act(action);
            ui.close();
        }
    }

    pub(crate) fn menu_bar(&mut self, ui: &mut Ui) {
        let cmd = Modifiers::COMMAND;
        let open = self.engine.library().is_some();
        egui::MenuBar::new().ui(ui, |ui| {
            ui.menu_button(self.lang.t("File"), |ui| {
                ui.add_enabled_ui(open, |ui| {
                    self.pick(
                        ui,
                        self.lang.t("New Playlist…"),
                        Some((cmd, Key::N)),
                        Action::Open(Dialog::new_playlist(Vec::new())),
                    );
                    self.pick(
                        ui,
                        self.lang.t("New Smart Playlist…"),
                        Some((cmd | Modifiers::SHIFT, Key::N)),
                        Action::Open(Dialog::smart_new()),
                    );
                    ui.separator();
                    self.pick(ui, self.lang.t("Add Files to Library…"), Some((cmd, Key::O)), Action::AddFiles);
                    self.pick(ui, self.lang.t("Add Folder to Library…"), None, Action::AddFolder);
                    self.pick(ui, self.lang.t("Add a Song Without a File…"), None, Action::Open(Dialog::song()));
                    self.pick(ui, self.lang.t("Import Song List…"), None, Action::ImportSongs);
                    self.pick(ui, self.lang.t("Import Playlist…"), None, Action::ImportPlaylist);
                    self.pick(ui, self.lang.t("Import Library File…"), None, Action::ImportLibraryFile);
                    ui.separator();
                });
                self.pick(ui, self.lang.t("Choose Library Folder…"), None, Action::ChooseLibrary);
                ui.separator();
                self.pick(ui, self.lang.t("Exit"), None, Action::Quit);
            });
            ui.menu_button(self.lang.t("Edit"), |ui| {
                self.pick(ui, self.lang.t("Find"), Some((cmd, Key::F)), Action::FocusSearch);
                self.pick(ui, self.lang.t("Bring Back Removed Songs"), None, Action::Run(Command::RestoreRemoved));
                ui.separator();
                self.pick(ui, self.lang.t("Settings…"), Some((cmd, Key::Comma)), Action::Open(Dialog::settings()));
            });
            ui.menu_button(self.lang.t("View"), |ui| {
                for (name, view) in [
                    (self.lang.t("Recently Added"), View::RecentlyAdded),
                    (self.lang.t("Artists"), View::Artists),
                    (self.lang.t("Albums"), View::Albums),
                    (self.lang.t("Songs"), View::Songs),
                    (self.lang.t("Genres"), View::Genres),
                ] {
                    self.pick(ui, name, None, Action::Show(view));
                }
                ui.separator();
                let mut queue = self.show_queue;
                if ui.checkbox(&mut queue, self.lang.t("Play queue")).clicked() {
                    self.show_queue = queue;
                }
                let mut browser = self.engine.settings.column_browser;
                if ui.checkbox(&mut browser, self.lang.t("Column Browser in Songs")).clicked() {
                    self.engine.change_settings(|s| s.column_browser = browser);
                }
                self.pick(ui, self.lang.t("Compact Player"), None, Action::Compact(true));
                ui.separator();
                self.pick(
                    ui,
                    self.lang.t("Look for New Files"),
                    Some((Modifiers::NONE, Key::F5)),
                    Action::Run(Command::Scan),
                );
            });
            ui.menu_button(self.lang.t("Controls"), |ui| {
                let running = matches!(self.engine.now().status, NowStatus::Playing | NowStatus::Loading);
                let play = if running { self.lang.t("Pause") } else { self.lang.t("Play") };
                self.pick(ui, play, Some((Modifiers::NONE, Key::Space)), Action::Run(Command::PlayPause));
                self.pick(ui, self.lang.t("Stop"), None, Action::Run(Command::Stop));
                self.pick(ui, self.lang.t("Next"), Some((cmd, Key::ArrowRight)), Action::Run(Command::Next));
                self.pick(ui, self.lang.t("Previous"), Some((cmd, Key::ArrowLeft)), Action::Run(Command::Previous));
                ui.separator();
                let v = self.engine.settings.volume;
                self.pick(
                    ui,
                    self.lang.t("Louder"),
                    Some((cmd, Key::ArrowUp)),
                    Action::Run(Command::Volume { percent: v.saturating_add(5).min(100) }),
                );
                self.pick(
                    ui,
                    self.lang.t("Quieter"),
                    Some((cmd, Key::ArrowDown)),
                    Action::Run(Command::Volume { percent: v.saturating_sub(5) }),
                );
                ui.separator();
                let mut on = self.engine.queue.shuffle();
                if ui.checkbox(&mut on, self.lang.t("Shuffle")).clicked() {
                    self.cmd(Command::Shuffle { on });
                }
                let mode = next_repeat(self.engine.queue.repeat);
                self.pick(ui, self.lang.t("Change Repeat"), None, Action::Run(Command::Repeat { mode }));
            });
            ui.menu_button(self.lang.t("Account"), |ui| {
                self.pick(ui, self.lang.t("Streaming Accounts"), None, Action::Show(View::Services));
                let active = self.engine.active();
                ui.menu_button(self.lang.t("Service in Use"), |ui| {
                    for s in self.engine.choices() {
                        let on = active.as_ref() == Some(&s);
                        if ui.radio(on, s.name()).clicked() {
                            self.cmd(Command::ActiveService { service: s.key() });
                            ui.close();
                        }
                    }
                });
                self.pick(ui, self.lang.t("Refresh Services"), None, Action::Run(Command::Refresh));
            });
            ui.menu_button(self.lang.t("Help"), |ui| {
                self.pick(
                    ui,
                    self.lang.t("MusicCraft on the Web"),
                    None,
                    Action::Browse(env!("CARGO_PKG_REPOSITORY").to_owned()),
                );
                self.pick(ui, self.lang.t("About MusicCraft"), None, Action::Open(Dialog::About));
            });
        });
    }

    /// The menu of chosen songs.
    pub(crate) fn item_menu(&mut self, ui: &mut Ui, items: &[Item]) {
        if items.is_empty() {
            return;
        }
        let ids: Vec<String> = items.iter().filter(|i| i.source == Source::Local).map(|i| i.id.clone()).collect();
        self.pick(ui, self.lang.t("Play Next"), None, Action::Run(Command::QueueNext { items: items.to_vec() }));
        self.pick(ui, self.lang.t("Add to Queue"), None, Action::Run(Command::QueueAdd { items: items.to_vec() }));
        let lists: Vec<(String, String)> = self
            .engine
            .library()
            .map(|l| {
                let plain = l.playlists().iter().filter(|p| p.smart.is_none());
                plain.map(|p| (p.id.clone(), p.name.clone())).collect()
            })
            .unwrap_or_default();
        ui.menu_button(self.lang.t("Add to Playlist"), |ui| {
            self.pick(ui, self.lang.t("New Playlist…"), None, Action::Open(Dialog::new_playlist(items.to_vec())));
            if !lists.is_empty() {
                ui.separator();
            }
            for (id, name) in lists {
                self.pick(ui, &name, None, Action::Run(Command::PlaylistAdd { id, items: items.to_vec() }));
            }
        });
        if items.iter().any(|i| i.source.key().is_some()) {
            let add = Command::LibraryAdd { items: items.to_vec() };
            self.pick(ui, self.lang.t("Add to Library"), None, Action::Run(add));
        }
        if let [Item { source: Source::Spotify, id, .. }] = items
            && let Some(track) = id.strip_prefix("spotify:track:")
        {
            let page = format!("https://open.spotify.com/track/{track}");
            self.pick(ui, self.lang.t("Open in Spotify"), None, Action::Browse(page));
        }
        if ids.is_empty() {
            return;
        }
        ui.separator();
        let all_loved =
            self.engine.library().is_some_and(|l| ids.iter().all(|id| l.track(id).is_some_and(|t| t.loved)));
        let love = if all_loved { self.lang.t("Remove love") } else { self.lang.t("Love") };
        self.pick(ui, love, None, Action::Run(Command::Love { ids: ids.clone(), on: !all_loved }));
        ui.menu_button(self.lang.t("Rating"), |ui| {
            for stars in (0..=5u8).rev() {
                let name = match stars {
                    0 => self.lang.t("No rating").to_owned(),
                    n => self.lang.n(usize::from(n), "{} star", "{} stars"),
                };
                self.pick(ui, &name, None, Action::Run(Command::Rate { ids: ids.clone(), stars }));
            }
        });
        ui.separator();
        let info = self.lang.t("Song Information…");
        let dialog = self.info_dialog(&ids);
        self.pick(ui, info, None, Action::Open(dialog));
        if let [id] = ids.as_slice() {
            let has_file = self.engine.library().and_then(|l| l.file(id)).is_some();
            if has_file {
                self.pick(ui, self.lang.t("Show in Folder"), None, Action::Reveal(id.clone()));
            }
            if let Some(d) = self.link_dialog(id) {
                let name = self.engine.active().map(|s| s.name()).unwrap_or_default();
                let text = self.lang.f("The Song on {}…", name);
                self.pick(ui, &text, None, Action::Open(d));
            }
        }
        ui.separator();
        self.pick(ui, self.lang.t("Remove from Library…"), None, Action::Open(Dialog::Remove { ids }));
    }
}
