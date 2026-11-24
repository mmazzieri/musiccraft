//! The header: controls, what is playing, search; the tabs; the status bar; the play queue.

use crate::app::{Action, App, Tab, View};
use crate::i18n::time;
use crate::icons::{self, Icon};
use crate::table::cell_text;
use crate::theme::{chosen, place, tokens};
use egui::{Align, Layout, Rect, Sense, Ui, UiBuilder, Vec2, pos2, vec2};
use musiccraft_engine::core::next_repeat;
use musiccraft_engine::{Command, NowStatus, Repeat};

const HEIGHT: f32 = 62.0;

impl App {
    pub(crate) fn header(&mut self, ui: &mut Ui) {
        let width = ui.available_width();
        let (rect, _) = ui.allocate_exact_size(vec2(width, HEIGHT), Sense::hover());
        let compact = self.compact;
        let left = if compact { 130.0 } else { (width * 0.22).clamp(210.0, 300.0) };
        let right = if compact { 40.0 } else { (width * 0.22).clamp(190.0, 290.0) };
        let display = Rect::from_min_max(
            pos2(rect.left() + left + 8.0, rect.top() + 2.0),
            pos2(rect.right() - right - 8.0, rect.bottom() - 2.0),
        );
        let part = |r: Rect| UiBuilder::new().max_rect(r);
        let controls = Rect::from_min_size(rect.min, vec2(left, HEIGHT));
        place(ui, part(controls).layout(Layout::left_to_right(Align::Center)), |ui| self.controls(ui));
        place(ui, part(display), |ui| self.display(ui, display));
        let end = Rect::from_min_max(pos2(rect.right() - right, rect.top()), rect.max);
        place(ui, part(end).layout(Layout::right_to_left(Align::Center)), |ui| {
            if compact {
                let t = tokens(ui);
                if icons::button(ui, Icon::Grow, 26.0, self.lang.t("Full window"), t.text).clicked() {
                    self.act(Action::Compact(false));
                }
            } else {
                self.search_field(ui);
            }
        });
    }

    fn controls(&mut self, ui: &mut Ui) {
        let t = tokens(ui);
        let now = self.engine.now();
        ui.spacing_mut().item_spacing.x = 2.0;
        if icons::button(ui, Icon::Previous, 32.0, self.lang.t("Previous"), t.text).clicked() {
            self.cmd(Command::Previous);
        }
        let running = matches!(now.status, NowStatus::Playing | NowStatus::Loading);
        let (icon, name) =
            if running { (Icon::Pause, self.lang.t("Pause")) } else { (Icon::Play, self.lang.t("Play")) };
        if icons::button(ui, icon, 42.0, name, t.text).clicked() {
            self.cmd(Command::PlayPause);
        }
        if icons::button(ui, Icon::Next, 32.0, self.lang.t("Next"), t.text).clicked() {
            self.cmd(Command::Next);
        }
        if self.compact {
            return;
        }
        ui.add_space(8.0);
        let (r, _) = ui.allocate_exact_size(Vec2::splat(18.0), Sense::hover());
        icons::draw(ui.painter(), r, Icon::Volume, t.weak);
        let mut volume = u32::from(self.engine.settings.volume);
        ui.spacing_mut().slider_width = (ui.available_width() - 14.0).clamp(40.0, 110.0);
        let slider = ui.add(egui::Slider::new(&mut volume, 0..=100).show_value(false));
        let label = self.lang.t("Volume");
        slider.widget_info(|| egui::WidgetInfo::slider(true, f64::from(volume), label));
        if slider.changed() {
            self.cmd(Command::Volume { percent: u8::try_from(volume).unwrap_or(100) });
        }
        slider.on_hover_text(self.lang.f("Volume {}%", volume));
    }

    /// What is playing: picture, names, position, shuffle and repeat.
    fn display(&mut self, ui: &mut Ui, rect: Rect) {
        let t = tokens(ui);
        ui.painter().rect(rect, 8.0, t.display, egui::Stroke::new(1.0, t.line), egui::StrokeKind::Inside);
        let now = self.engine.now();
        let side = rect.height() - 8.0;
        let art = Rect::from_min_size(rect.min + vec2(4.0, 4.0), Vec2::splat(side));
        let Some(item) = now.item.clone() else {
            icons::draw(ui.painter(), Rect::from_center_size(rect.center(), Vec2::splat(30.0)), Icon::Waves, t.accent);
            return;
        };
        let dark = ui.visuals().dark_mode;
        match self.texture(ui.ctx(), &item, &now.meta) {
            Some(id) => picture(ui, id, art),
            None => icons::blank_cover(ui.painter(), art, &now.meta.album, dark),
        }
        let side_width = 34.0;
        let middle = Rect::from_min_max(
            pos2(art.right() + 8.0 + side_width, rect.top() + 4.0),
            pos2(rect.right() - side_width - 6.0, rect.bottom() - 2.0),
        );
        // Shuffle and repeat, on both sides of the names.
        let shuffle = self.engine.queue.shuffle();
        let at = Rect::from_center_size(pos2(art.right() + 4.0 + side_width / 2.0, rect.center().y), Vec2::splat(30.0));
        place(ui, UiBuilder::new().max_rect(at), |ui| {
            if icons::toggle(ui, Icon::Shuffle, 26.0, self.lang.t("Shuffle"), shuffle, t.accent, t.weak).clicked() {
                self.cmd(Command::Shuffle { on: !shuffle });
            }
        });
        let repeat = self.engine.queue.repeat;
        let at =
            Rect::from_center_size(pos2(rect.right() - 4.0 - side_width / 2.0, rect.center().y), Vec2::splat(30.0));
        place(ui, UiBuilder::new().max_rect(at), |ui| {
            let (icon, name) = match repeat {
                Repeat::Off => (Icon::Repeat, self.lang.t("Repeat: off")),
                Repeat::All => (Icon::Repeat, self.lang.t("Repeat: all")),
                Repeat::One => (Icon::RepeatOne, self.lang.t("Repeat: one song")),
            };
            if icons::toggle(ui, icon, 26.0, name, repeat != Repeat::Off, t.accent, t.weak).clicked() {
                self.cmd(Command::Repeat { mode: next_repeat(repeat) });
            }
        });
        let line =
            |row: f32, h: f32| Rect::from_min_size(pos2(middle.left(), middle.top() + row), vec2(middle.width(), h));
        let (title, sub) = if now.live {
            let station = now.meta.title.clone();
            if now.stream_title.is_empty() {
                (station, self.lang.t("Live radio").to_owned())
            } else {
                (now.stream_title.clone(), station)
            }
        } else {
            let parts = [now.meta.artist.as_str(), now.meta.album.as_str()];
            let sub: Vec<&str> = parts.into_iter().filter(|s| !s.is_empty()).collect();
            (now.meta.title.clone(), sub.join(" — "))
        };
        centered(ui, line(0.0, 19.0), &title, t.text, 15.0);
        let sub = if now.status == NowStatus::Loading { self.lang.t("Loading…").to_owned() } else { sub };
        centered(ui, line(18.0, 16.0), &sub, t.weak, 13.0);
        let bottom = line(34.0, 20.0);
        if now.live {
            centered(ui, bottom, &time(now.position_ms), t.weak, 12.0);
            return;
        }
        self.seek_bar(ui, bottom, now.position_ms, now.duration_ms);
    }

    fn seek_bar(&mut self, ui: &mut Ui, rect: Rect, position: u64, duration: u64) {
        let t = tokens(ui);
        let shown = self.seeking.unwrap_or(position).min(duration.max(position));
        let w = 46.0;
        let left = Rect::from_min_size(rect.min, vec2(w, rect.height()));
        let right = Rect::from_min_max(pos2(rect.right() - w, rect.top()), rect.max);
        cell_text(ui, left, &time(shown), t.weak, true, 12.0);
        cell_text(ui, right, &format!("-{}", time(duration.saturating_sub(shown))), t.weak, false, 12.0);
        let bar = Rect::from_min_max(pos2(left.right() + 2.0, rect.top()), pos2(right.left() - 2.0, rect.bottom()));
        let r = ui.interact(bar, ui.scope_id().with("seek"), Sense::click_and_drag());
        let label = self.lang.t("Position in the song");
        let spoken = self.lang.f2("{} of {}", time(shown), time(duration));
        r.widget_info(|| egui::WidgetInfo::slider(duration > 0, shown as f64 / 1000.0, format!("{label}: {spoken}")));
        let track = Rect::from_center_size(bar.center(), vec2(bar.width(), 4.0));
        ui.painter().rect_filled(track, 2.0, t.line);
        let share = if duration > 0 { (shown as f32 / duration as f32).clamp(0.0, 1.0) } else { 0.0 };
        let x = track.left() + track.width() * share;
        ui.painter().rect_filled(Rect::from_min_max(track.min, pos2(x, track.bottom())), 2.0, t.accent);
        ui.painter().circle_filled(
            pos2(x, track.center().y),
            if r.hovered() || r.has_focus() { 6.0 } else { 4.5 },
            t.accent,
        );
        if r.has_focus() {
            ui.painter().rect_stroke(bar, 4.0, ui.visuals().widgets.active.bg_stroke, egui::StrokeKind::Inside);
        }
        if duration == 0 {
            return;
        }
        let at = |x: f32| ((((x - bar.left()) / bar.width()).clamp(0.0, 1.0)) * duration as f32) as u64;
        if let Some(p) = r.interact_pointer_pos()
            && (r.dragged() || r.clicked())
        {
            self.seeking = Some(at(p.x));
        }
        if (r.drag_stopped() || r.clicked())
            && let Some(ms) = self.seeking.take()
        {
            self.cmd(Command::Seek { ms });
        }
        if r.has_focus() {
            let (back, on) = ui.input_mut(|i| {
                (
                    i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowLeft),
                    i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowRight),
                )
            });
            if back {
                self.cmd(Command::Seek { ms: position.saturating_sub(5_000) });
            } else if on {
                self.cmd(Command::Seek { ms: (position + 5_000).min(duration) });
            }
        }
    }

    fn search_field(&mut self, ui: &mut Ui) {
        let t = tokens(ui);
        let queue = self.show_queue;
        if icons::toggle(ui, Icon::Queue, 28.0, self.lang.t("Play queue"), queue, t.accent, t.text).clicked() {
            self.show_queue = !queue;
        }
        let hint = match self.view.tab() {
            Tab::Library => self.lang.t("Search the library"),
            Tab::Radio => self.lang.t("Search stations"),
            Tab::Streaming => self.lang.t("Search this service"),
        };
        let field = egui::TextEdit::singleline(&mut self.search)
            .hint_text(hint)
            .desired_width((ui.available_width() - 34.0).max(60.0))
            .margin(egui::Margin { left: 26, right: 6, top: 5, bottom: 5 });
        let r = ui.add(field);
        r.widget_info(|| egui::WidgetInfo::labeled(egui::Role::SearchInput, true, hint));
        let glass = Rect::from_center_size(pos2(r.rect.left() + 14.0, r.rect.center().y), Vec2::splat(13.0));
        icons::draw(ui.painter(), glass, Icon::Search, t.weak);
        if std::mem::take(&mut self.focus_search) {
            r.request_focus();
        }
        let enter = r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
        if r.has_focus() && ui.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.search.clear();
        }
        match self.view.tab() {
            // Services are asked only when the person presses Enter.
            Tab::Radio if enter => {
                self.radio_query.clone_from(&self.search);
                self.act(Action::Show(View::RadioDirectory));
            }
            Tab::Streaming if enter || self.search.is_empty() => self.service_search.clone_from(&self.search),
            _ => {}
        }
    }

    pub(crate) fn tabs(&mut self, ui: &mut Ui) {
        let t = tokens(ui);
        let tabs = [
            (Tab::Library, self.lang.t("Library"), View::RecentlyAdded),
            (Tab::Radio, self.lang.t("Radio"), View::Stations),
            (Tab::Streaming, self.lang.t("Streaming"), View::Services),
        ];
        let current = self.view.tab();
        let size = vec2(112.0, 26.0);
        let (bar, _) = ui.allocate_exact_size(vec2(ui.available_width(), size.y), Sense::hover());
        let total = size.x * tabs.len() as f32;
        let mut x = bar.center().x - total / 2.0;
        for (tab, name, view) in tabs {
            let rect = Rect::from_min_size(pos2(x, bar.top()), size);
            x += size.x;
            let r = ui.interact(rect, ui.scope_id().with(("tab", name)), Sense::click());
            let on = tab == current;
            r.widget_info(|| egui::WidgetInfo::selected(egui::Role::Tab, true, on, name));
            chosen(ui, &r, on);
            if r.hovered() {
                ui.painter().rect_filled(rect, 5.0, t.row_hover);
            }
            if r.has_focus() {
                ui.painter().rect_stroke(rect, 5.0, ui.visuals().widgets.active.bg_stroke, egui::StrokeKind::Inside);
            }
            centered(ui, rect, name, if on { t.accent } else { t.text }, 14.5);
            if on {
                let under =
                    Rect::from_min_size(pos2(rect.left() + 18.0, rect.bottom() - 2.5), vec2(rect.width() - 36.0, 2.5));
                ui.painter().rect_filled(under, 1.0, t.accent);
            }
            if r.clicked() {
                self.act(Action::Show(view));
            }
        }
    }

    pub(crate) fn status_bar(&mut self, ui: &mut Ui) {
        let t = tokens(ui);
        ui.horizontal(|ui| {
            if let Some(n) = self.engine.notices.last().cloned() {
                let color = if n.error { t.error } else { t.text };
                let text = self.lang.f(&n.text, self.lang.t(&n.detail));
                if icons::button(ui, Icon::Close, 18.0, self.lang.t("Dismiss"), t.weak).clicked() {
                    self.act(Action::Dismiss);
                }
                ui.add(egui::Label::new(egui::RichText::new(text).color(color)).truncate());
                return;
            }
            let progress = if let Some((done, of)) = self.engine.import_progress() {
                self.lang.f2("Adding files: {} of {}", done, of)
            } else if let Some((found, read)) = self.engine.scan_progress() {
                self.lang.f2("Looking at the music folder: {} files, {} read", found, read)
            } else if self.engine.sign_in_page.is_some() {
                self.lang.t("Waiting for the sign-in in the browser…").to_owned()
            } else {
                String::new()
            };
            if !progress.is_empty() {
                ui.spinner();
                ui.label(progress);
                return;
            }
            let summary = self.summary();
            ui.with_layout(Layout::centered_and_justified(egui::Direction::LeftToRight), |ui| {
                ui.label(egui::RichText::new(summary).color(t.weak).size(13.0));
            });
        });
        ui.ctx().accesskit_node_builder(ui.scope_id(), |b| b.set_live(egui::accesskit::Live::Polite));
    }

    /// Songs, playing time and size of the library.
    fn summary(&self) -> String {
        let Some(lib) = self.engine.library() else { return String::new() };
        let (mut n, mut ms, mut size) = (0usize, 0u64, 0u64);
        for t in lib.tracks() {
            n += 1;
            ms = ms.saturating_add(t.info.duration_ms);
            size = size.saturating_add(t.info.size);
        }
        if n == 0 {
            return String::new();
        }
        let mut parts = vec![self.lang.n(n, "{} song", "{} songs"), self.long_time(ms)];
        let mb = size as f64 / 1_000_000.0;
        let text = if mb >= 1000.0 { format!("{:.1} GB", mb / 1000.0) } else { format!("{mb:.0} MB") };
        parts.push(if self.lang.code() == "it" { text.replace('.', ",") } else { text });
        let others = lib.devices_known().saturating_sub(1);
        if others > 0 {
            parts.push(self.lang.n(others, "shared with {} computer", "shared with {} computers"));
        }
        if self.demo {
            parts.push(self.lang.t("Demonstration library").to_owned());
        }
        parts.join(" · ")
    }

    pub(crate) fn long_time(&self, ms: u64) -> String {
        let minutes = ms / 60_000;
        if minutes >= 60 * 24 {
            let days = minutes as f64 / 1440.0;
            let text = format!("{days:.1}");
            self.lang.f("{} days", if self.lang.code() == "it" { text.replace('.', ",") } else { text })
        } else if minutes >= 60 {
            self.lang.f2("{} h {} min", minutes / 60, minutes % 60)
        } else {
            self.lang.f("{} min", minutes.max(1))
        }
    }

    pub(crate) fn queue_panel(&mut self, ui: &mut Ui) {
        let t = tokens(ui);
        ui.horizontal(|ui| {
            ui.heading(self.lang.t("Play queue"));
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if icons::button(ui, Icon::Close, 22.0, self.lang.t("Close the queue"), t.text).clicked() {
                    self.show_queue = false;
                }
                if ui.button(self.lang.t("Clear")).clicked() {
                    self.cmd(Command::QueueClear);
                }
            });
        });
        ui.separator();
        let pos = self.engine.queue.position();
        let items = self.engine.queue.items().to_vec();
        if items.len() <= pos + 1 {
            ui.add_space(12.0);
            ui.label(
                egui::RichText::new(self.lang.t("Nothing is waiting. Right-click a song to add it.")).color(t.weak),
            );
        }
        let start = pos.saturating_sub(5);
        let count = items.len().saturating_sub(start);
        egui::ScrollArea::vertical().auto_shrink([false, false]).show_rows(ui, 44.0, count, |ui, range| {
            for i in range {
                let i = i + start;
                let Some(item) = items.get(i) else { continue };
                let meta = self.engine.meta(item);
                ui.push_id(i, |ui| self.queue_row(ui, i, pos, item, &meta));
            }
        });
    }

    fn queue_row(
        &mut self,
        ui: &mut Ui,
        i: usize,
        pos: usize,
        item: &musiccraft_library::Item,
        meta: &musiccraft_library::Meta,
    ) {
        let t = tokens(ui);
        let (rect, r) = ui.allocate_exact_size(vec2(ui.available_width(), 44.0), Sense::click());
        let current = i == pos;
        let state = if current { self.lang.t("playing") } else { "" };
        r.widget_info(|| {
            egui::WidgetInfo::selected(
                egui::Role::ListItem,
                true,
                current,
                format!("{}, {} {state}", meta.title, meta.artist),
            )
        });
        if r.hovered() {
            ui.painter().rect_filled(rect, 5.0, t.row_hover);
        }
        if r.has_focus() {
            ui.painter().rect_stroke(rect, 5.0, ui.visuals().widgets.active.bg_stroke, egui::StrokeKind::Inside);
        }
        let art = Rect::from_min_size(rect.min + vec2(2.0, 4.0), Vec2::splat(36.0));
        let dark = ui.visuals().dark_mode;
        match self.texture(ui.ctx(), item, meta) {
            Some(id) => picture(ui, id, art),
            None => icons::blank_cover(ui.painter(), art, &meta.album, dark),
        }
        let buttons = if i > pos && (r.hovered() || r.has_focus()) { 78.0 } else { 0.0 };
        let text = Rect::from_min_max(pos2(art.right() + 2.0, rect.top()), pos2(rect.right() - buttons, rect.bottom()));
        let half =
            |n: f32| Rect::from_min_size(pos2(text.left(), text.top() + 4.0 + n * 18.0), vec2(text.width(), 18.0));
        let color = match (current, i < pos) {
            (true, _) => t.accent,
            (_, true) => t.weak,
            _ => t.text,
        };
        cell_text(ui, half(0.0), &meta.title, color, false, 14.0);
        cell_text(ui, half(1.0), &meta.artist, t.weak, false, 12.5);
        if r.double_clicked() || (r.has_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter))) {
            self.cmd(Command::QueueGo { index: i });
        }
        let name = self.lang.t("Play queue");
        r.context_menu(|ui| {
            ui.label(egui::RichText::new(name).color(t.weak));
            if ui.button(self.lang.t("Play now")).clicked() {
                self.cmd(Command::QueueGo { index: i });
            }
            ui.add_enabled_ui(i > pos, |ui| {
                if ui.button(self.lang.t("Earlier")).clicked() {
                    self.cmd(Command::QueueShift { index: i, later: false });
                }
                if ui.button(self.lang.t("Later")).clicked() {
                    self.cmd(Command::QueueShift { index: i, later: true });
                }
                if ui.button(self.lang.t("Remove")).clicked() {
                    self.cmd(Command::QueueRemove { index: i });
                }
            });
        });
        if buttons > 0.0 {
            let at = Rect::from_min_max(pos2(rect.right() - buttons, rect.top()), rect.max);
            place(ui, UiBuilder::new().max_rect(at).layout(Layout::left_to_right(Align::Center)), |ui| {
                ui.spacing_mut().item_spacing.x = 0.0;
                if icons::button(ui, Icon::Up, 20.0, self.lang.t("Earlier"), t.text).clicked() {
                    self.cmd(Command::QueueShift { index: i, later: false });
                }
                if icons::button(ui, Icon::Down, 20.0, self.lang.t("Later"), t.text).clicked() {
                    self.cmd(Command::QueueShift { index: i, later: true });
                }
                if icons::button(ui, Icon::Close, 20.0, self.lang.t("Remove"), t.text).clicked() {
                    self.cmd(Command::QueueRemove { index: i });
                }
            });
        }
    }
}

/// Draws a picture with slightly rounded corners, filling `rect`.
pub fn picture(ui: &Ui, id: egui::TextureId, rect: Rect) {
    egui::Image::new((id, rect.size())).corner_radius(4).paint_at(ui, rect);
}

fn centered(ui: &Ui, rect: Rect, text: &str, color: egui::Color32, size: f32) {
    let mut job = egui::text::LayoutJob::single_section(
        text.to_owned(),
        egui::TextFormat::simple(egui::FontId::proportional(size), color),
    );
    job.wrap = egui::text::TextWrapping::truncate_at_width(rect.width());
    let g = ui.painter().layout_job(job);
    ui.painter().galley(rect.center() - g.size() / 2.0, g, color);
}
