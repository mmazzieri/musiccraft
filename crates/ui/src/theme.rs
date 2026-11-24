//! Colours and sizes. Every view takes them from here, so both themes apply everywhere.

use egui::{Color32, CornerRadius, Stroke};

#[derive(Debug, Clone, Copy)]
pub struct Tokens {
    pub header: Color32,
    pub sidebar: Color32,
    pub content: Color32,
    pub row_alt: Color32,
    pub row_hover: Color32,
    /// The now-playing display.
    pub display: Color32,
    pub line: Color32,
    pub text: Color32,
    pub weak: Color32,
    pub accent: Color32,
    /// Behind selected rows.
    pub selection: Color32,
    pub on_selection: Color32,
    pub love: Color32,
    pub star: Color32,
    pub error: Color32,
}

const LIGHT: Tokens = Tokens {
    header: Color32::from_rgb(0xEC, 0xEE, 0xF1),
    sidebar: Color32::from_rgb(0xF4, 0xF5, 0xF7),
    content: Color32::from_rgb(0xFF, 0xFF, 0xFF),
    row_alt: Color32::from_rgb(0xF5, 0xF7, 0xF9),
    row_hover: Color32::from_rgb(0xE6, 0xEE, 0xF2),
    display: Color32::from_rgb(0xFB, 0xFB, 0xFA),
    line: Color32::from_rgb(0xD3, 0xD7, 0xDC),
    text: Color32::from_rgb(0x1B, 0x1F, 0x24),
    weak: Color32::from_rgb(0x59, 0x62, 0x6C),
    accent: Color32::from_rgb(0x0B, 0x6E, 0x7D),
    selection: Color32::from_rgb(0x0B, 0x6E, 0x7D),
    on_selection: Color32::from_rgb(0xFF, 0xFF, 0xFF),
    love: Color32::from_rgb(0xB4, 0x2B, 0x51),
    star: Color32::from_rgb(0x8A, 0x5A, 0x00),
    error: Color32::from_rgb(0xA8, 0x1E, 0x1E),
};

const DARK: Tokens = Tokens {
    header: Color32::from_rgb(0x2A, 0x2D, 0x31),
    sidebar: Color32::from_rgb(0x23, 0x26, 0x2A),
    content: Color32::from_rgb(0x1B, 0x1D, 0x20),
    row_alt: Color32::from_rgb(0x21, 0x24, 0x28),
    row_hover: Color32::from_rgb(0x2C, 0x35, 0x3A),
    display: Color32::from_rgb(0x34, 0x38, 0x3D),
    line: Color32::from_rgb(0x41, 0x46, 0x4C),
    text: Color32::from_rgb(0xEC, 0xEE, 0xF0),
    weak: Color32::from_rgb(0xAE, 0xB6, 0xBE),
    accent: Color32::from_rgb(0x55, 0xC5, 0xD3),
    selection: Color32::from_rgb(0x12, 0x68, 0x76),
    on_selection: Color32::from_rgb(0xFF, 0xFF, 0xFF),
    love: Color32::from_rgb(0xFF, 0x86, 0xA5),
    star: Color32::from_rgb(0xF2, 0xC1, 0x4E),
    error: Color32::from_rgb(0xFF, 0x9A, 0x8F),
};

pub fn tokens(ui: &egui::Ui) -> Tokens {
    if ui.visuals().dark_mode { DARK } else { LIGHT }
}

/// Draws in a given place without moving where the next widget goes.
pub fn place<R>(ui: &mut egui::Ui, at: egui::UiBuilder, add: impl FnOnce(&mut egui::Ui) -> R) -> R {
    add(&mut ui.new_child(at))
}

/// Says that a row, tab or list entry is chosen. egui itself reports such things as pressed, which
/// is for buttons.
pub fn chosen(ui: &egui::Ui, r: &egui::Response, on: bool) {
    ui.ctx().accesskit_node_builder(r.id, |b| {
        b.clear_toggled();
        b.set_selected(on);
    });
}

/// Width given to the sidebar at first.
pub const SIDEBAR: f32 = 212.0;
pub const ROW: f32 = 26.0;
pub const TILE: f32 = 158.0;

/// Puts the colours into egui, for both themes.
pub fn install(ctx: &egui::Context) {
    for (theme, t) in [(egui::Theme::Light, LIGHT), (egui::Theme::Dark, DARK)] {
        let mut v = if theme == egui::Theme::Dark { egui::Visuals::dark() } else { egui::Visuals::light() };
        v.panel_fill = t.content;
        v.window_fill = t.sidebar;
        v.extreme_bg_color = t.content;
        v.override_text_color = Some(t.text);
        v.weak_text_color = Some(t.weak);
        v.hyperlink_color = t.accent;
        v.selection.bg_fill = t.selection;
        v.selection.stroke = Stroke::new(1.0, t.on_selection);
        v.widgets.noninteractive.bg_stroke = Stroke::new(1.0, t.line);
        v.widgets.noninteractive.fg_stroke = Stroke::new(1.0, t.text);
        let round = CornerRadius::same(5);
        for w in [&mut v.widgets.inactive, &mut v.widgets.hovered, &mut v.widgets.active, &mut v.widgets.open] {
            w.corner_radius = round;
            w.fg_stroke = Stroke::new(1.5, t.text);
        }
        v.widgets.inactive.weak_bg_fill = t.row_hover;
        v.widgets.inactive.bg_fill = t.line;
        v.widgets.hovered.weak_bg_fill = t.line;
        v.widgets.hovered.bg_stroke = Stroke::new(1.0, t.weak);
        v.widgets.active.weak_bg_fill = t.line;
        v.widgets.active.bg_stroke = Stroke::new(2.0, t.accent);
        v.slider_trailing_fill = true;
        ctx.set_visuals_of(theme, v);
    }
    ctx.all_styles_mut(|s| {
        s.spacing.item_spacing = egui::vec2(8.0, 4.0);
        s.spacing.button_padding = egui::vec2(8.0, 4.0);
        s.spacing.interact_size.y = 26.0;
        s.interaction.selectable_labels = false;
        for font in s.text_styles.values_mut() {
            font.size = (font.size * 1.06).round();
        }
    });
}

#[cfg(test)]
mod tests {
    use super::{DARK, LIGHT};
    use egui::Color32;

    fn luminance(c: Color32) -> f32 {
        let f = |v: u8| {
            let v = f32::from(v) / 255.0;
            if v <= 0.040_45 { v / 12.92 } else { ((v + 0.055) / 1.055).powf(2.4) }
        };
        0.2126 * f(c.r()) + 0.7152 * f(c.g()) + 0.0722 * f(c.b())
    }

    fn contrast(a: Color32, b: Color32) -> f32 {
        let (x, y) = (luminance(a), luminance(b));
        (x.max(y) + 0.05) / (x.min(y) + 0.05)
    }

    /// Text is readable on every surface it is drawn on, in both themes (WCAG 2.2 AA).
    #[test]
    fn text_has_enough_contrast() {
        for t in [LIGHT, DARK] {
            for back in [t.header, t.sidebar, t.content, t.row_alt, t.row_hover, t.display] {
                for front in [t.text, t.weak, t.accent, t.error] {
                    assert!(contrast(front, back) >= 4.5, "{front:?} on {back:?}: {}", contrast(front, back));
                }
                for mark in [t.love, t.star] {
                    assert!(contrast(mark, back) >= 3.0, "{mark:?} on {back:?}");
                }
            }
            assert!(contrast(t.on_selection, t.selection) >= 4.5);
        }
    }
}
