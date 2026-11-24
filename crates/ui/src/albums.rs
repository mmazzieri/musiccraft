//! Album grids. A chosen album opens in place, under its row.

use crate::app::App;
use crate::header::picture;
use crate::i18n::time;
use crate::icons::{self, Icon};
use crate::table::cell_text;
use crate::theme::{ROW, TILE, place, tokens};
use egui::{Rect, Sense, Ui, UiBuilder, Vec2, pos2, vec2};
use musiccraft_engine::Command;
use musiccraft_library::query::Album;
use musiccraft_library::{Item, Meta};

const GAP: f32 = 18.0;
const LABEL: f32 = 42.0;

/// One tile of a grid.
pub struct Tile {
    pub key: String,
    pub title: String,
    pub sub: String,
    /// What gives the picture.
    pub item: Item,
    pub meta: Meta,
}

impl App {
    /// A grid of the library's albums.
    pub(crate) fn album_grid(&mut self, ui: &mut Ui, id: &str, albums: &[Album]) {
        if albums.is_empty() {
            self.nothing(ui);
            return;
        }
        let width = ui.available_width() - 28.0;
        let columns = (((width + GAP) / (TILE + GAP)).floor() as usize).max(1);
        let side = ((width + GAP) / columns as f32 - GAP).min(TILE * 1.25);
        let row_height = side + LABEL + GAP;
        let open = self.open_album.as_ref().and_then(|k| albums.iter().position(|a| a.key == *k));
        let detail = open.and_then(|i| albums.get(i)).cloned();
        let detail_height = detail.as_ref().map_or(0.0, |a| self.detail_height(a, width));
        egui::ScrollArea::vertical().id_salt(id).auto_shrink([false, false]).show_viewport(ui, |ui, view| {
            let rows = albums.len().div_ceil(columns);
            let total = rows as f32 * row_height + detail_height + GAP;
            ui.set_height(total);
            let origin = ui.max_rect().min + vec2(14.0, GAP);
            let mut y = 0.0;
            for row in 0..rows {
                let top = y;
                y += row_height;
                let opened = open.is_some_and(|i| i / columns == row);
                if opened {
                    y += detail_height;
                }
                if y < view.min.y - GAP || top > view.max.y {
                    continue;
                }
                for col in 0..columns {
                    let Some(a) = albums.get(row * columns + col) else { break };
                    let at = origin + vec2(col as f32 * (side + GAP), top);
                    let rect = Rect::from_min_size(at, vec2(side, side + LABEL));
                    let tile = self.album_tile(a);
                    let on = self.open_album.as_deref() == Some(&a.key);
                    let r = self.tile(ui, rect, side, &tile, on);
                    if r.clicked() {
                        self.open_album = if on { None } else { Some(a.key.clone()) };
                        self.reveal_album = !on;
                    }
                    if r.double_clicked() {
                        self.play_album(a, false);
                    }
                    r.context_menu(|ui| self.album_menu(ui, a));
                }
                if opened && let Some(a) = &detail {
                    let rect = Rect::from_min_size(
                        origin + vec2(-6.0, top + row_height - GAP * 0.5),
                        vec2(width + 12.0, detail_height - GAP * 0.5),
                    );
                    if std::mem::take(&mut self.reveal_album) {
                        ui.scroll_to_rect(rect.expand(GAP), None);
                    }
                    let column = open.map_or(0, |i| i % columns);
                    let pointer = origin.x + column as f32 * (side + GAP) + side / 2.0;
                    self.album_detail(ui, rect, pointer, a);
                }
            }
        });
    }

    fn album_tile(&self, a: &Album) -> Tile {
        let item = Item::local(a.tracks.first().map_or("", String::as_str));
        Tile {
            key: a.key.clone(),
            title: if a.title.is_empty() { self.lang.t("Unknown Album").to_owned() } else { a.title.clone() },
            sub: if a.artist.is_empty() { self.lang.t("Unknown Artist").to_owned() } else { a.artist.clone() },
            meta: Meta::default(),
            item,
        }
    }

    /// A picture with two lines under it.
    pub(crate) fn tile(&mut self, ui: &mut Ui, rect: Rect, side: f32, tile: &Tile, on: bool) -> egui::Response {
        let t = tokens(ui);
        let r = ui.interact(rect, ui.scope_id().with(("tile", &tile.key)), Sense::click());
        r.widget_info(|| {
            egui::WidgetInfo::selected(egui::Role::Button, true, on, format!("{}, {}", tile.title, tile.sub))
        });
        let art = Rect::from_min_size(rect.min, Vec2::splat(side));
        ui.painter().rect_filled(
            art.translate(vec2(0.0, 2.0)).expand(1.0),
            6.0,
            egui::Color32::from_black_alpha(if ui.visuals().dark_mode { 90 } else { 34 }),
        );
        let dark = ui.visuals().dark_mode;
        match self.texture(ui.ctx(), &tile.item, &tile.meta) {
            Some(id) => picture(ui, id, art),
            None => icons::blank_cover(ui.painter(), art, &tile.title, dark),
        }
        if on || r.has_focus() {
            ui.painter().rect_stroke(art.expand(3.0), 7.0, egui::Stroke::new(2.5, t.accent), egui::StrokeKind::Middle);
        } else if r.hovered() {
            ui.painter().rect_stroke(art, 4.0, egui::Stroke::new(1.5, t.weak), egui::StrokeKind::Inside);
        }
        let line = |n: f32| {
            Rect::from_min_size(pos2(art.left() - 5.0, art.bottom() + 4.0 + n * 18.0), vec2(side + 10.0, 18.0))
        };
        cell_text(ui, line(0.0), &tile.title, t.text, false, 14.0);
        cell_text(ui, line(1.0), &tile.sub, t.weak, false, 13.0);
        r
    }

    fn album_menu(&mut self, ui: &mut Ui, a: &Album) {
        let items: Vec<Item> = a.tracks.iter().map(|id| Item::local(id)).collect();
        if ui.button(self.lang.t("Play")).clicked() {
            self.play_album(a, false);
        }
        if ui.button(self.lang.t("Play Shuffled")).clicked() {
            self.play_album(a, true);
        }
        ui.separator();
        self.item_menu(ui, &items);
    }

    pub(crate) fn play_album(&mut self, a: &Album, shuffled: bool) {
        let items: Vec<Item> = a.tracks.iter().map(|id| Item::local(id)).collect();
        if shuffled {
            self.cmd(Command::Shuffle { on: true });
        }
        self.cmd(Command::Play { items, start: 0 });
    }

    fn detail_height(&self, a: &Album, width: f32) -> f32 {
        let rows = if width > 760.0 { a.tracks.len().div_ceil(2) } else { a.tracks.len() };
        (rows as f32 * ROW + 112.0).max(216.0) + GAP
    }

    /// An album opened: its songs beside its picture.
    fn album_detail(&mut self, ui: &mut Ui, rect: Rect, pointer: f32, a: &Album) {
        let t = tokens(ui);
        ui.painter().rect(rect, 8.0, t.sidebar, egui::Stroke::new(1.0, t.line), egui::StrokeKind::Inside);
        ui.painter().add(egui::Shape::convex_polygon(
            vec![
                pos2(pointer - 9.0, rect.top() + 1.0),
                pos2(pointer, rect.top() - 8.0),
                pos2(pointer + 9.0, rect.top() + 1.0),
            ],
            t.sidebar,
            egui::Stroke::NONE,
        ));
        let inner = rect.shrink(16.0);
        place(ui, UiBuilder::new().max_rect(inner), |ui| self.album_block(ui, a, inner, 168.0));
    }

    /// Title, buttons and songs of an album, in `rect`.
    pub(crate) fn album_block(&mut self, ui: &mut Ui, a: &Album, rect: Rect, art_side: f32) {
        let t = tokens(ui);
        let tile = self.album_tile(a);
        let art = Rect::from_min_size(pos2(rect.right() - art_side, rect.top()), Vec2::splat(art_side));
        let dark = ui.visuals().dark_mode;
        match self.texture(ui.ctx(), &tile.item, &tile.meta) {
            Some(id) => picture(ui, id, art),
            None => icons::blank_cover(ui.painter(), art, &tile.title, dark),
        }
        let left = Rect::from_min_max(rect.min, pos2(art.left() - 18.0, rect.bottom()));
        let line = |y: f32, h: f32| Rect::from_min_size(pos2(left.left() - 6.0, left.top() + y), vec2(left.width(), h));
        cell_text(ui, line(0.0, 26.0), &tile.title, t.text, false, 20.0);
        let mut sub = vec![tile.sub.clone()];
        if !a.genre.is_empty() {
            sub.push(a.genre.clone());
        }
        if a.year > 0 {
            sub.push(a.year.to_string());
        }
        sub.push(self.lang.n(a.tracks.len(), "{} song", "{} songs"));
        sub.push(self.long_time(a.duration_ms));
        cell_text(ui, line(26.0, 20.0), &sub.join(" · "), t.weak, false, 13.5);
        let buttons = line(48.0, 28.0).translate(vec2(6.0, 0.0));
        place(ui, UiBuilder::new().max_rect(buttons).layout(egui::Layout::left_to_right(egui::Align::Center)), |ui| {
            if ui.button(self.lang.t("Play")).clicked() {
                self.play_album(a, false);
            }
            if ui.button(self.lang.t("Play Shuffled")).clicked() {
                self.play_album(a, true);
            }
        });
        let top = left.top() + 82.0;
        let columns = if left.width() > 540.0 && a.tracks.len() > 1 { 2 } else { 1 };
        let per = a.tracks.len().div_ceil(columns);
        let width = left.width() / columns as f32;
        for (i, id) in a.tracks.iter().enumerate() {
            let cell = Rect::from_min_size(
                pos2(left.left() + (i / per) as f32 * width, top + (i % per) as f32 * ROW),
                vec2(width - 8.0, ROW),
            );
            self.album_song(ui, a, i, id, cell);
        }
    }

    fn album_song(&mut self, ui: &mut Ui, a: &Album, i: usize, id: &str, cell: Rect) {
        let t = tokens(ui);
        let Some(track) = self.engine.library().and_then(|l| l.track(id)).cloned() else { return };
        let playing = self.engine.now().item.is_some_and(|n| n == Item::local(id));
        let r = ui.interact(cell, ui.scope_id().with(("song", id)), Sense::click());
        r.widget_info(|| {
            egui::WidgetInfo::selected(
                egui::Role::ListItem,
                true,
                playing,
                format!("{}, {}", track.info.title, time(track.info.duration_ms)),
            )
        });
        if r.hovered() {
            ui.painter().rect_filled(cell, 4.0, t.row_hover);
        }
        if r.has_focus() {
            ui.painter().rect_stroke(cell, 4.0, ui.visuals().widgets.active.bg_stroke, egui::StrokeKind::Inside);
        }
        let number = Rect::from_min_size(cell.min, vec2(34.0, ROW));
        if playing {
            let at = Rect::from_center_size(number.center(), Vec2::splat(13.0));
            icons::draw(ui.painter(), at, Icon::Speaker, t.accent);
        } else {
            let n = if track.info.track > 0 { track.info.track } else { u32::try_from(i + 1).unwrap_or(0) };
            cell_text(ui, number, &n.to_string(), t.weak, true, 13.0);
        }
        let clock = Rect::from_min_max(pos2(cell.right() - 54.0, cell.top()), cell.max);
        let mut name = Rect::from_min_max(pos2(number.right(), cell.top()), pos2(clock.left(), cell.bottom()));
        if track.loved {
            name.max.x -= 18.0;
            let at = Rect::from_center_size(pos2(name.right() + 8.0, cell.center().y), Vec2::splat(12.0));
            icons::draw(ui.painter(), at, Icon::HeartFull, t.love);
        }
        cell_text(ui, name, &track.info.title, if playing { t.accent } else { t.text }, false, 14.0);
        cell_text(ui, clock, &time(track.info.duration_ms), t.weak, true, 13.0);
        let enter = r.has_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
        if r.double_clicked() || enter {
            let items = a.tracks.iter().map(|id| Item::local(id)).collect();
            self.cmd(Command::Play { items, start: i });
        }
        let one = [Item::local(id)];
        r.context_menu(|ui| self.item_menu(ui, &one));
    }
}
