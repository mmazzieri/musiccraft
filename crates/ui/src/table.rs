//! The song table: sortable columns, many rows, keyboard and pointer selection. Only rows on
//! screen are laid out; each is one node with its own name and state for assistive technology.

use crate::i18n::{Lang, time};
use crate::icons::{self, Icon};
use crate::theme::{ROW, Tokens, chosen, tokens};
use egui::{Align2, Color32, FontId, Key, Modifiers, Rect, Sense, Ui, pos2, vec2};
use musiccraft_library::Item;
use musiccraft_library::model::Presence;
use musiccraft_library::query::Sort;
use std::collections::BTreeSet;

/// One row.
#[derive(Debug, Clone, Default)]
pub struct Row {
    pub item: Option<Item>,
    pub number: u32,
    pub title: String,
    pub artist: String,
    pub album: String,
    pub genre: String,
    pub year: u32,
    pub duration_ms: u64,
    pub loved: bool,
    pub rating: u8,
    pub plays: u64,
    pub added: u64,
    pub presence: Presence,
    /// The service in use was asked for it and does not have it.
    pub lacking: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Column {
    Number,
    Title,
    Time,
    Artist,
    Album,
    Genre,
    Year,
    Loved,
    Rating,
    Plays,
    Added,
}

pub const SONGS: &[Column] = &[
    Column::Title,
    Column::Time,
    Column::Artist,
    Column::Album,
    Column::Genre,
    Column::Loved,
    Column::Rating,
    Column::Plays,
];
pub const LISTED: &[Column] = &[
    Column::Number,
    Column::Title,
    Column::Time,
    Column::Artist,
    Column::Album,
    Column::Genre,
    Column::Loved,
    Column::Rating,
    Column::Plays,
];
pub const RECENT: &[Column] = &[
    Column::Title,
    Column::Time,
    Column::Artist,
    Column::Album,
    Column::Added,
    Column::Loved,
    Column::Rating,
    Column::Plays,
];
pub const SERVICE: &[Column] =
    &[Column::Number, Column::Title, Column::Time, Column::Artist, Column::Album, Column::Year];
pub const STATIONS: &[Column] = &[Column::Title, Column::Artist, Column::Album];

impl Column {
    /// Fixed width, or share of what is left.
    fn width(self) -> (f32, f32) {
        match self {
            Column::Number => (44.0, 0.0),
            Column::Title => (0.0, 3.2),
            Column::Time => (58.0, 0.0),
            Column::Artist => (0.0, 2.2),
            Column::Album => (0.0, 2.4),
            Column::Genre => (0.0, 1.2),
            Column::Year => (56.0, 0.0),
            Column::Loved => (40.0, 0.0),
            Column::Rating => (96.0, 0.0),
            Column::Plays => (64.0, 0.0),
            Column::Added => (104.0, 0.0),
        }
    }

    fn sort(self) -> Option<Sort> {
        Some(match self {
            Column::Number => return None,
            Column::Title => Sort::Title,
            Column::Time => Sort::Time,
            Column::Artist => Sort::Artist,
            Column::Album => Sort::Album,
            Column::Genre => Sort::Genre,
            Column::Year => Sort::Year,
            Column::Loved => Sort::Loved,
            Column::Rating => Sort::Rating,
            Column::Plays => Sort::Plays,
            Column::Added => Sort::Added,
        })
    }

    fn right(self) -> bool {
        matches!(self, Column::Number | Column::Time | Column::Plays | Column::Year)
    }

    pub fn name<'a>(self, l: &Lang) -> &'a str {
        match self {
            Column::Number => "#",
            Column::Title => l.t("Title"),
            Column::Time => l.t("Time"),
            Column::Artist => l.t("Artist"),
            Column::Album => l.t("Album"),
            Column::Genre => l.t("Genre"),
            Column::Year => l.t("Year"),
            Column::Loved => l.t("Love"),
            Column::Rating => l.t("Rating"),
            Column::Plays => l.t("Plays"),
            Column::Added => l.t("Added"),
        }
    }
}

/// What a table remembers.
#[derive(Debug, Clone, Default)]
pub struct State {
    pub sort: Option<(Sort, bool)>,
    pub selected: BTreeSet<usize>,
    anchor: usize,
    cursor: Option<usize>,
    /// Row to bring into view.
    reveal: Option<usize>,
    focus_cursor: bool,
    cursor_id: Option<egui::Id>,
}

impl State {
    /// Forgets the selection, when the rows changed.
    pub fn reset(&mut self) {
        self.selected.clear();
        self.cursor = None;
        self.anchor = 0;
    }

    /// Whether the keyboard is on one of its rows.
    pub fn has_focus(&self, ctx: &egui::Context) -> bool {
        self.cursor_id.is_some_and(|id| ctx.memory(|m| m.has_focus(id)))
    }

    fn select(&mut self, row: usize, m: Modifiers) {
        if m.shift {
            self.selected = (self.anchor.min(row)..=self.anchor.max(row)).collect();
        } else if m.command {
            if !self.selected.remove(&row) {
                self.selected.insert(row);
            }
            self.anchor = row;
        } else {
            self.selected = BTreeSet::from([row]);
            self.anchor = row;
        }
        self.cursor = Some(row);
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Event {
    Play(usize),
    Sort(Sort, bool),
    Love(usize, bool),
    Rate(usize, u8),
    /// Delete was pressed on the selection.
    Delete,
}

/// The row a cell belongs to.
struct At<'a> {
    t: &'a Tokens,
    i: usize,
    row: &'a Row,
    hover: bool,
    selected: bool,
}

pub struct Table<'a> {
    pub id: &'a str,
    pub rows: &'a [Row],
    pub columns: &'a [Column],
    pub lang: &'a Lang,
    /// The item playing now.
    pub playing: Option<&'a Item>,
    /// Whether the headers sort.
    pub sortable: bool,
    /// Whether love and rating can be changed.
    pub editable: bool,
    /// The name of the table.
    pub name: &'a str,
}

impl Table<'_> {
    pub fn show(&self, ui: &mut Ui, state: &mut State, mut menu: impl FnMut(&mut Ui, &BTreeSet<usize>)) -> Vec<Event> {
        let t = tokens(ui);
        let mut events = Vec::new();
        let (id, name) = (ui.scope_id(), self.name.to_owned());
        ui.ctx().accesskit_node_builder(id, |b| {
            b.set_role(egui::Role::Table);
            b.set_label(name);
            b.set_row_count(self.rows.len());
        });
        let width = ui.available_width();
        let widths = self.widths(width - 14.0);
        self.header(ui, &t, &widths, state, &mut events);
        self.keys(ui, state, &mut events);

        let mut area = egui::ScrollArea::vertical().id_salt(self.id).auto_shrink([false, false]);
        if let Some(row) = state.reveal.take() {
            let view = ui.available_height();
            let key = ui.scope_id().with(self.id);
            let at: f32 = ui.data(|d| d.get_temp(key)).unwrap_or(0.0);
            let (top, bottom) = (row as f32 * ROW, (row + 1) as f32 * ROW);
            if top < at {
                area = area.vertical_scroll_offset(top);
            } else if bottom > at + view {
                area = area.vertical_scroll_offset(bottom - view);
            }
        }
        ui.spacing_mut().item_spacing.y = 0.0;
        let out = area.show_rows(ui, ROW, self.rows.len(), |ui, range| {
            for i in range {
                if let Some(row) = self.rows.get(i) {
                    self.row(ui, (&t, &widths), (i, row), state, &mut menu, &mut events);
                }
            }
        });
        let key = ui.scope_id().with(self.id);
        ui.data_mut(|d| d.insert_temp(key, out.state.clamped_offset().y));
        events
    }

    fn widths(&self, total: f32) -> Vec<f32> {
        let (fixed, share) = self.columns.iter().fold((0.0, 0.0), |(f, s), c| {
            let (a, b) = c.width();
            (f + a, s + b)
        });
        let unit = if share > 0.0 { ((total - fixed) / share).max(24.0) } else { 0.0 };
        self.columns.iter().map(|c| c.width().0 + c.width().1 * unit).collect()
    }

    fn header(&self, ui: &mut Ui, t: &Tokens, widths: &[f32], state: &State, events: &mut Vec<Event>) {
        let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), ROW), Sense::hover());
        ui.painter().rect_filled(rect, 0.0, t.sidebar);
        ui.painter().hline(rect.x_range(), rect.bottom(), egui::Stroke::new(1.0, t.line));
        let mut x = rect.left() + 6.0;
        for (c, w) in self.columns.iter().zip(widths) {
            let cell = Rect::from_min_size(pos2(x, rect.top()), vec2(*w, ROW));
            x += w;
            let name = c.name(self.lang);
            let sort = if self.sortable { c.sort() } else { None };
            let active = state.sort.filter(|(s, _)| Some(*s) == sort);
            let mut color = t.weak;
            if let Some(sort) = sort {
                let r = ui.interact(cell, ui.scope_id().with((self.id, "head", name)), Sense::click());
                let order = match active {
                    Some((_, true)) => self.lang.t("sorted, last first"),
                    Some((_, false)) => self.lang.t("sorted"),
                    None => "",
                };
                let label = if order.is_empty() { name.to_owned() } else { format!("{name}, {order}") };
                r.widget_info(|| egui::WidgetInfo::selected(egui::Role::Button, true, active.is_some(), &label));
                if r.hovered() || active.is_some() {
                    color = t.text;
                }
                if r.has_focus() {
                    ui.painter().rect_stroke(
                        cell.shrink(1.0),
                        3.0,
                        ui.visuals().widgets.active.bg_stroke,
                        egui::StrokeKind::Inside,
                    );
                }
                if r.clicked() {
                    // Counts and dates start with the greatest.
                    let first =
                        matches!(sort, Sort::Plays | Sort::Added | Sort::Rating | Sort::Loved | Sort::LastPlayed);
                    events.push(Event::Sort(sort, active.map_or(first, |(_, d)| !d)));
                }
            }
            let shown = if *c == Column::Loved { "" } else { name };
            cell_text(ui, cell, shown, color, c.right(), 13.0);
            if *c == Column::Loved {
                icons::draw(ui.painter(), Rect::from_center_size(cell.center(), vec2(12.0, 12.0)), Icon::Heart, color);
            }
            if let Some((_, desc)) = active {
                let at = Rect::from_center_size(
                    pos2(cell.right() - if c.right() { cell.width() - 8.0 } else { 10.0 }, cell.center().y),
                    vec2(9.0, 9.0),
                );
                icons::draw(ui.painter(), at, if desc { Icon::Down } else { Icon::Up }, t.accent);
            }
        }
    }

    fn row_id(&self, ui: &Ui, i: usize) -> egui::Id {
        ui.ctx().viewport_id().0.with(("row", self.id, i))
    }

    fn keys(&self, ui: &mut Ui, state: &mut State, events: &mut Vec<Event>) {
        let n = self.rows.len();
        state.cursor_id = None;
        let Some(cursor) = state.cursor.filter(|c| *c < n) else { return };
        state.cursor_id = Some(self.row_id(ui, cursor));
        if !ui.memory(|m| m.has_focus(self.row_id(ui, cursor))) {
            return;
        }
        let page = ((ui.available_height() / ROW) as usize).max(2) - 1;
        let mods = ui.input(|i| i.modifiers);
        let mut to = None;
        ui.input_mut(|i| {
            let mut hit =
                |key: Key| i.consume_key(Modifiers::NONE, key) || (mods.shift && i.consume_key(Modifiers::SHIFT, key));
            if hit(Key::ArrowDown) {
                to = Some((cursor + 1).min(n - 1));
            } else if hit(Key::ArrowUp) {
                to = Some(cursor.saturating_sub(1));
            } else if hit(Key::PageDown) {
                to = Some((cursor + page).min(n - 1));
            } else if hit(Key::PageUp) {
                to = Some(cursor.saturating_sub(page));
            } else if hit(Key::Home) {
                to = Some(0);
            } else if hit(Key::End) {
                to = Some(n - 1);
            } else if i.consume_key(Modifiers::NONE, Key::Enter) {
                events.push(Event::Play(cursor));
            } else if i.consume_key(Modifiers::COMMAND, Key::A) {
                state.selected = (0..n).collect();
            } else if i.consume_key(Modifiers::NONE, Key::Delete) {
                events.push(Event::Delete);
            }
        });
        if let Some(row) = to {
            state.select(row, Modifiers { shift: mods.shift, ..Modifiers::NONE });
            state.reveal = Some(row);
            state.focus_cursor = true;
        }
    }

    fn row(
        &self,
        ui: &mut Ui,
        (t, widths): (&Tokens, &[f32]),
        (i, row): (usize, &Row),
        state: &mut State,
        menu: &mut impl FnMut(&mut Ui, &BTreeSet<usize>),
        events: &mut Vec<Event>,
    ) {
        let (_, rect) = ui.allocate_space(vec2(ui.available_width(), ROW));
        let id = self.row_id(ui, i);
        let r = ui.interact(rect, id, Sense::click());
        let selected = state.selected.contains(&i);
        let playing = row.item.is_some() && row.item.as_ref() == self.playing;
        if r.has_focus() {
            ui.memory_mut(|m| {
                let lock = egui::EventFilter { vertical_arrows: true, ..Default::default() };
                m.set_focus_lock_filter(id, lock);
            });
        }
        let back = match (selected, r.hovered(), i % 2) {
            (true, _, _) => t.selection,
            (_, true, _) => t.row_hover,
            (_, _, 1) => t.row_alt,
            _ => t.content,
        };
        ui.painter().rect_filled(rect, 0.0, back);
        if r.has_focus() && !selected {
            ui.painter().rect_stroke(
                rect.shrink(1.0),
                2.0,
                ui.visuals().widgets.active.bg_stroke,
                egui::StrokeKind::Inside,
            );
        }
        let absent = row.lacking || row.presence == Presence::Missing;
        let text = match (selected, absent, playing) {
            (true, _, _) => t.on_selection,
            (_, true, _) => t.weak,
            (_, _, true) => t.accent,
            _ => t.text,
        };
        let soft = if selected { t.on_selection } else { t.weak };
        let at = At { t, i, row, hover: r.hovered(), selected };
        let mut x = rect.left() + 6.0;
        for (c, w) in self.columns.iter().zip(widths) {
            let cell = Rect::from_min_size(pos2(x, rect.top()), vec2(*w, ROW));
            x += w;
            match c {
                Column::Number => {
                    let n = if row.number > 0 { row.number.to_string() } else { (i + 1).to_string() };
                    cell_text(ui, cell, &n, soft, true, 13.0);
                }
                Column::Title => {
                    let mut inner = cell;
                    let mark = match row.presence {
                        _ if playing => Some(Icon::Speaker),
                        _ if row.lacking => Some(Icon::Missing),
                        Presence::Stream => Some(Icon::Waves),
                        Presence::Cloud => Some(Icon::Cloud),
                        Presence::Missing => Some(Icon::Missing),
                        Presence::Here => None,
                    };
                    if let Some(icon) = mark {
                        let at = Rect::from_center_size(pos2(cell.left() + 12.0, cell.center().y), vec2(14.0, 14.0));
                        icons::draw(ui.painter(), at, icon, if playing && !selected { t.accent } else { soft });
                        inner.min.x += 20.0;
                    }
                    cell_text(ui, inner, &row.title, text, false, 14.5);
                }
                Column::Time => {
                    let s = if row.duration_ms > 0 { time(row.duration_ms) } else { String::new() };
                    cell_text(ui, cell, &s, soft, true, 13.5);
                }
                Column::Artist => cell_text(ui, cell, &row.artist, text, false, 14.5),
                Column::Album => cell_text(ui, cell, &row.album, text, false, 14.5),
                Column::Genre => cell_text(ui, cell, &row.genre, soft, false, 14.0),
                Column::Year => {
                    let s = if row.year > 0 { row.year.to_string() } else { String::new() };
                    cell_text(ui, cell, &s, soft, true, 13.5);
                }
                Column::Plays => {
                    let s = if row.plays > 0 { row.plays.to_string() } else { String::new() };
                    cell_text(ui, cell, &s, soft, true, 13.5);
                }
                Column::Added => cell_text(ui, cell, &date(row.added), soft, false, 13.5),
                Column::Loved => self.love(ui, cell, &at, events),
                Column::Rating => self.rating(ui, cell, &at, events),
            }
        }
        let l = self.lang;
        r.widget_info(|| {
            let mut name = format!("{}, {}, {}", row.title, row.artist, row.album);
            let state_text = match row.presence {
                _ if playing => l.t("playing"),
                _ if row.lacking => l.t("the service in use does not have it"),
                Presence::Stream => l.t("plays from the streaming service in use"),
                Presence::Cloud => l.t("in the cloud, not downloaded yet"),
                Presence::Missing => l.t("the file is not on this computer"),
                Presence::Here => "",
            };
            for extra in [time(row.duration_ms).as_str(), state_text, if row.loved { l.t("loved") } else { "" }] {
                if !extra.is_empty() {
                    name.push_str(", ");
                    name.push_str(extra);
                }
            }
            egui::WidgetInfo::selected(egui::Role::Row, true, selected, name)
        });
        chosen(ui, &r, selected);
        if r.clicked() || (r.secondary_clicked() && !selected) {
            let m = ui.input(|i| i.modifiers);
            state.select(i, if r.secondary_clicked() { Modifiers::NONE } else { m });
            r.request_focus();
        }
        if r.gained_focus() && state.cursor != Some(i) {
            state.select(i, Modifiers::NONE);
        }
        if state.focus_cursor && state.cursor == Some(i) {
            state.focus_cursor = false;
            r.request_focus();
        }
        if r.double_clicked() {
            events.push(Event::Play(i));
        }
        r.context_menu(|ui| menu(ui, &state.selected));
    }

    fn love(&self, ui: &mut Ui, cell: Rect, at: &At<'_>, events: &mut Vec<Event>) {
        let At { t, i, row, hover, selected } = *at;
        if !self.editable {
            return;
        }
        let hit = Rect::from_center_size(cell.center(), vec2(24.0, 24.0));
        let r = ui.interact(hit, self.row_id(ui, i).with("love"), Sense::click());
        let name = if row.loved { self.lang.t("Remove love") } else { self.lang.t("Love") };
        r.widget_info(|| {
            egui::WidgetInfo::selected(egui::Role::Button, true, row.loved, format!("{name}: {}", row.title))
        });
        if row.loved || hover || selected || r.has_focus() {
            let color = match (selected, row.loved) {
                (true, _) => t.on_selection,
                (_, true) => t.love,
                _ => t.weak,
            };
            let icon = if row.loved { Icon::HeartFull } else { Icon::Heart };
            icons::draw(ui.painter(), Rect::from_center_size(hit.center(), vec2(13.0, 13.0)), icon, color);
        }
        focus_ring(ui, hit, &r);
        if r.clicked() {
            events.push(Event::Love(i, !row.loved));
        }
    }

    fn rating(&self, ui: &mut Ui, cell: Rect, at: &At<'_>, events: &mut Vec<Event>) {
        let At { t, i, row, hover, selected } = *at;
        if !self.editable {
            return;
        }
        // One control with a value from 0 to 5, rather than five small buttons.
        let hit = Rect::from_min_size(pos2(cell.left() + 6.0, cell.center().y - 12.0), vec2(80.0, 24.0));
        let r = ui.interact(hit, self.row_id(ui, i).with("rating"), Sense::click());
        r.widget_info(|| {
            let mut info = egui::WidgetInfo::labeled(
                egui::Role::Slider,
                true,
                format!("{}: {}", self.lang.t("Rating"), row.title),
            );
            info.value = Some(f64::from(row.rating));
            info
        });
        if row.rating > 0 || hover || r.hovered() || r.has_focus() || selected {
            let color = if selected { t.on_selection } else { t.star };
            for n in 0..5u8 {
                let at = Rect::from_center_size(
                    pos2(hit.left() + 8.0 + f32::from(n) * 16.0, hit.center().y),
                    vec2(13.0, 13.0),
                );
                let full = n < row.rating;
                let ink = if full { color } else { t.weak };
                let ink = if selected { t.on_selection } else { ink };
                icons::draw(ui.painter(), at, if full { Icon::StarFull } else { Icon::Star }, ink);
            }
        }
        focus_ring(ui, hit, &r);
        if r.clicked()
            && let Some(p) = r.interact_pointer_pos()
        {
            let n = (((p.x - hit.left()) / 16.0).floor() as i32 + 1).clamp(1, 5) as u8;
            events.push(Event::Rate(i, if n == row.rating { 0 } else { n }));
        }
        if r.has_focus() {
            let (less, more) = ui.input_mut(|i| {
                (i.consume_key(Modifiers::NONE, Key::ArrowLeft), i.consume_key(Modifiers::NONE, Key::ArrowRight))
            });
            if more {
                events.push(Event::Rate(i, (row.rating + 1).min(5)));
            } else if less {
                events.push(Event::Rate(i, row.rating.saturating_sub(1)));
            }
        }
    }
}

fn focus_ring(ui: &Ui, rect: Rect, r: &egui::Response) {
    if r.has_focus() {
        ui.painter().rect_stroke(rect, 4.0, ui.visuals().widgets.active.bg_stroke, egui::StrokeKind::Inside);
    }
}

/// One line of text in a cell, cut with an ellipsis.
pub fn cell_text(ui: &Ui, cell: Rect, text: &str, color: Color32, right: bool, size: f32) {
    if text.is_empty() {
        return;
    }
    let width = (cell.width() - 12.0).max(4.0);
    let mut job = egui::text::LayoutJob::single_section(
        text.to_owned(),
        egui::TextFormat::simple(FontId::proportional(size), color),
    );
    job.wrap = egui::text::TextWrapping::truncate_at_width(width);
    let galley = ui.painter().layout_job(job);
    let x = if right { cell.right() - 6.0 - galley.size().x } else { cell.left() + 6.0 };
    let y = Align2::LEFT_CENTER.pos_in_rect(&cell).y - galley.size().y / 2.0;
    ui.painter().with_clip_rect(cell.intersect(ui.clip_rect())).galley(pos2(x, y), galley, color);
}

/// A date as year-month-day, in the computer's time zone.
pub fn date(ms: u64) -> String {
    i64::try_from(ms)
        .ok()
        .filter(|m| *m > 0)
        .and_then(chrono::DateTime::from_timestamp_millis)
        .map(|d| d.with_timezone(&chrono::Local).format("%Y-%m-%d").to_string())
        .unwrap_or_default()
}
