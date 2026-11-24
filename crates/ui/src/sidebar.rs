//! The sidebar: what can be shown, for the tab that is open.

use crate::app::{Action, App, Tab, View};
use crate::dialogs::Dialog;
use crate::icons::{self, Icon};
use crate::table::cell_text;
use crate::theme::{ROW, chosen, place, tokens};
use egui::{Rect, Response, Sense, Ui, UiBuilder, Vec2, pos2, vec2};
use musiccraft_engine::Command;
use musiccraft_engine::pages::{Page, PageState};
use musiccraft_library::smart::{BUILT_IN, BuiltIn};

impl App {
    pub(crate) fn sidebar(&mut self, ui: &mut Ui) {
        ui.spacing_mut().item_spacing.y = 1.0;
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| match self.view.tab() {
            Tab::Library => self.library_sidebar(ui),
            Tab::Radio => self.radio_sidebar(ui),
            Tab::Streaming => self.streaming_sidebar(ui),
        });
    }

    fn heading(&mut self, ui: &mut Ui, text: &str) {
        ui.add_space(8.0);
        let t = tokens(ui);
        ui.label(egui::RichText::new(text).color(t.weak).size(12.5).strong());
        ui.add_space(2.0);
    }

    fn entry(&mut self, ui: &mut Ui, icon: Icon, text: &str, view: View) -> Response {
        let on = self.view == view;
        let r = entry(ui, icon, text, on);
        if r.clicked() {
            self.act(Action::Show(view));
        }
        r
    }

    fn library_sidebar(&mut self, ui: &mut Ui) {
        let text = self.lang.t("Library");
        self.heading(ui, text);
        let fixed = [
            (Icon::Clock, self.lang.t("Recently Added"), View::RecentlyAdded),
            (Icon::Person, self.lang.t("Artists"), View::Artists),
            (Icon::Disc, self.lang.t("Albums"), View::Albums),
            (Icon::Note, self.lang.t("Songs"), View::Songs),
            (Icon::Tag, self.lang.t("Genres"), View::Genres),
        ];
        for (icon, text, view) in fixed {
            self.entry(ui, icon, text, view);
        }
        let text = self.lang.t("Smart Lists");
        self.heading(ui, text);
        for b in BUILT_IN.into_iter().filter(|b| *b != BuiltIn::RecentlyAdded) {
            let text = self.built_in_name(b);
            let icon = if b == BuiltIn::Loved { Icon::Heart } else { Icon::Smart };
            self.entry(ui, icon, text, View::BuiltIn(b));
        }
        let text = self.lang.t("Playlists");
        self.heading(ui, text);
        let plus = Rect::from_min_size(pos2(ui.max_rect().right() - 28.0, ui.cursor().top() - 26.0), Vec2::splat(24.0));
        place(ui, UiBuilder::new().max_rect(plus), |ui| {
            let t = tokens(ui);
            if icons::button(ui, Icon::Plus, 20.0, self.lang.t("New Playlist"), t.text).clicked() {
                self.act(Action::Open(Dialog::new_playlist(Vec::new())));
            }
        });
        let lists: Vec<(String, String, bool)> = self
            .engine
            .library()
            .map(|l| l.playlists().iter().map(|p| (p.id.clone(), p.name.clone(), p.smart.is_some())).collect())
            .unwrap_or_default();
        if lists.is_empty() {
            let t = tokens(ui);
            ui.label(egui::RichText::new(self.lang.t("No playlists yet")).color(t.weak));
        }
        for (id, name, smart) in lists {
            let icon = if smart { Icon::Smart } else { Icon::List };
            let r = ui.push_id(&id, |ui| self.entry(ui, icon, &name, View::Playlist(id.clone()))).inner;
            r.context_menu(|ui| self.playlist_menu(ui, &id, &name, smart));
        }
    }

    pub(crate) fn playlist_menu(&mut self, ui: &mut Ui, id: &str, name: &str, smart: bool) {
        let id = id.to_owned();
        if ui.button(self.lang.t("Play")).clicked()
            && let Some(items) = self.engine.playlist_items(&id)
        {
            self.cmd(Command::Play { items, start: 0 });
        }
        if smart && ui.button(self.lang.t("Edit Rules…")).clicked() {
            let rules = self.engine.library().and_then(|l| l.playlist(&id)).and_then(|p| p.smart.clone());
            self.act(Action::Open(Dialog::smart_edit(&id, name, rules.unwrap_or_default())));
        }
        if ui.button(self.lang.t("Rename…")).clicked() {
            self.act(Action::Open(Dialog::Rename { id: id.clone(), name: name.to_owned() }));
        }
        if ui.button(self.lang.t("Export…")).clicked() {
            self.act(Action::ExportPlaylist(id.clone(), name.to_owned()));
        }
        ui.separator();
        if ui.button(self.lang.t("Delete…")).clicked() {
            self.act(Action::Open(Dialog::DeletePlaylist { id, name: name.to_owned() }));
        }
    }

    pub(crate) fn built_in_name(&self, b: BuiltIn) -> &'static str {
        match b {
            BuiltIn::RecentlyAdded => self.lang.t("Recently Added"),
            BuiltIn::RecentlyPlayed => self.lang.t("Recently Played"),
            BuiltIn::MostPlayed => self.lang.t("Most Played"),
            BuiltIn::Loved => self.lang.t("Loved Songs"),
        }
    }

    fn radio_sidebar(&mut self, ui: &mut Ui) {
        let text = self.lang.t("Radio");
        self.heading(ui, text);
        let fixed = [
            (Icon::Heart, self.lang.t("My Stations"), View::Stations),
            (Icon::Search, self.lang.t("Find Stations"), View::RadioDirectory),
        ];
        for (icon, text, view) in fixed {
            self.entry(ui, icon, text, view);
        }
        ui.add_space(6.0);
        if ui.button(self.lang.t("Add a Station by Address…")).clicked() {
            self.act(Action::Open(Dialog::Station { name: String::new(), url: String::new() }));
        }
    }

    fn streaming_sidebar(&mut self, ui: &mut Ui) {
        let text = self.lang.t("Services");
        self.heading(ui, text);
        let text = self.lang.t("Accounts");
        self.entry(ui, Icon::Gear, text, View::Services);
        let servers: Vec<String> = self.engine.settings.servers.iter().map(|s| s.name.clone()).collect();
        for name in servers {
            self.heading(ui, &name);
            ui.push_id(&name, |ui| {
                let text = self.lang.t("Albums");
                self.entry(ui, Icon::Disc, text, View::Server(name.clone()));
                if let PageState::Ready(c) = self.engine.page(&Page::ServerPlaylists(name.clone())) {
                    for p in &c.albums {
                        ui.push_id(&p.id, |ui| {
                            self.entry(ui, Icon::List, &p.title, View::ServerPlaylist(name.clone(), p.clone()));
                        });
                    }
                }
            });
        }
        if self.engine.spotify().is_some() {
            self.heading(ui, "Spotify");
            let text = self.lang.t("Saved Songs");
            self.entry(ui, Icon::Heart, text, View::SpotifySaved);
            if let PageState::Ready(c) = self.engine.page(&Page::SpotifyPlaylists) {
                for p in &c.albums {
                    ui.push_id(&p.id, |ui| {
                        self.entry(ui, Icon::List, &p.title, View::SpotifyPlaylist(p.clone()));
                    });
                }
            }
        }
    }
}

/// A line of a list, with an icon.
pub fn entry(ui: &mut Ui, icon: Icon, text: &str, on: bool) -> Response {
    let t = tokens(ui);
    let (rect, r) = ui.allocate_exact_size(vec2(ui.available_width(), ROW + 1.0), Sense::click());
    r.widget_info(|| egui::WidgetInfo::selected(egui::Role::ListItem, true, on, text));
    chosen(ui, &r, on);
    let back = match (on, r.hovered()) {
        (true, _) => Some(t.selection),
        (_, true) => Some(t.row_hover),
        _ => None,
    };
    if let Some(c) = back {
        ui.painter().rect_filled(rect, 5.0, c);
    }
    if r.has_focus() {
        let stroke = if on { egui::Stroke::new(2.0, t.on_selection) } else { ui.visuals().widgets.active.bg_stroke };
        ui.painter().rect_stroke(rect.shrink(1.0), 5.0, stroke, egui::StrokeKind::Inside);
    }
    let at = Rect::from_center_size(pos2(rect.left() + 16.0, rect.center().y), Vec2::splat(15.0));
    icons::draw(ui.painter(), at, icon, if on { t.on_selection } else { t.accent });
    let label = Rect::from_min_max(pos2(rect.left() + 24.0, rect.top()), rect.max);
    cell_text(ui, label, text, if on { t.on_selection } else { t.text }, false, 14.5);
    r
}
