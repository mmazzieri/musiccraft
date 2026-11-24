//! Icons, drawn from lines and simple shapes. All are original.

use egui::{Color32, Painter, Pos2, Rect, Response, Sense, Shape, Stroke, Ui, Vec2, pos2, vec2};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Icon {
    Play,
    Pause,
    Next,
    Previous,
    Volume,
    Shuffle,
    Repeat,
    RepeatOne,
    Queue,
    Heart,
    HeartFull,
    Star,
    StarFull,
    Cloud,
    Missing,
    Plus,
    Search,
    Note,
    Disc,
    Person,
    Tag,
    Clock,
    List,
    Smart,
    Gear,
    Waves,
    Speaker,
    Close,
    Up,
    Down,
    Fold,
    Grow,
}

/// Draws an icon centred in `rect`.
pub fn draw(p: &Painter, rect: Rect, icon: Icon, color: Color32) {
    let s = rect.width().min(rect.height());
    let c = rect.center();
    // Points are given on a grid from -1 to 1.
    let at = |x: f32, y: f32| c + vec2(x, y) * s * 0.5;
    let w = (s * 0.1).max(1.4);
    let line = Stroke::new(w, color);
    let seg = |a: (f32, f32), b: (f32, f32)| {
        p.line_segment([at(a.0, a.1), at(b.0, b.1)], line);
    };
    let fill = |pts: &[(f32, f32)]| {
        p.add(Shape::convex_polygon(pts.iter().map(|q| at(q.0, q.1)).collect(), color, Stroke::NONE));
    };
    let path = |pts: Vec<Pos2>| {
        p.add(Shape::line(pts, line));
    };
    let arc = |cx: f32, cy: f32, r: f32, from: f32, to: f32| -> Vec<Pos2> {
        (0..=16)
            .map(|i| {
                let a = (from + (to - from) * i as f32 / 16.0).to_radians();
                at(cx + r * a.cos(), cy + r * a.sin())
            })
            .collect()
    };
    let circle = |cx: f32, cy: f32, r: f32| {
        p.circle_stroke(at(cx, cy), r * s * 0.5, line);
    };
    let dot = |cx: f32, cy: f32, r: f32| {
        p.circle_filled(at(cx, cy), r * s * 0.5, color);
    };
    let bar = |x0: f32, y0: f32, x1: f32, y1: f32| {
        p.rect_filled(Rect::from_two_pos(at(x0, y0), at(x1, y1)), s * 0.04, color);
    };
    match icon {
        Icon::Play => fill(&[(-0.55, -0.8), (0.8, 0.0), (-0.55, 0.8)]),
        Icon::Pause => {
            bar(-0.6, -0.75, -0.18, 0.75);
            bar(0.18, -0.75, 0.6, 0.75);
        }
        Icon::Next => {
            fill(&[(-0.85, -0.6), (0.0, 0.0), (-0.85, 0.6)]);
            fill(&[(-0.05, -0.6), (0.8, 0.0), (-0.05, 0.6)]);
        }
        Icon::Previous => {
            fill(&[(0.85, -0.6), (0.0, 0.0), (0.85, 0.6)]);
            fill(&[(0.05, -0.6), (-0.8, 0.0), (0.05, 0.6)]);
        }
        Icon::Volume | Icon::Speaker => {
            bar(-0.85, -0.28, -0.5, 0.28);
            fill(&[(-0.5, -0.28), (0.0, -0.7), (0.0, 0.7), (-0.5, 0.28)]);
            path(arc(0.0, 0.0, 0.4, -45.0, 45.0));
            path(arc(0.0, 0.0, 0.8, -45.0, 45.0));
        }
        Icon::Shuffle => {
            path(vec![at(-0.9, -0.5), at(-0.45, -0.5), at(0.3, 0.5), at(0.6, 0.5)]);
            path(vec![at(-0.9, 0.5), at(-0.45, 0.5), at(0.3, -0.5), at(0.6, -0.5)]);
            fill(&[(0.5, -0.8), (0.95, -0.5), (0.5, -0.2)]);
            fill(&[(0.5, 0.2), (0.95, 0.5), (0.5, 0.8)]);
        }
        Icon::Repeat | Icon::RepeatOne => {
            path(vec![at(-0.75, 0.05), at(-0.75, -0.45), at(0.55, -0.45)]);
            path(vec![at(0.75, -0.05), at(0.75, 0.45), at(-0.55, 0.45)]);
            fill(&[(0.4, -0.78), (0.85, -0.45), (0.4, -0.12)]);
            fill(&[(-0.4, 0.12), (-0.85, 0.45), (-0.4, 0.78)]);
            if icon == Icon::RepeatOne {
                dot(0.0, 0.0, 0.2);
            }
        }
        Icon::Queue => {
            for y in [-0.6, 0.0, 0.6] {
                seg((-0.35, y), (0.85, y));
                dot(-0.75, y, 0.13);
            }
        }
        Icon::Heart | Icon::HeartFull => {
            let mut pts = arc(-0.4, -0.25, 0.42, 150.0, 340.0);
            pts.extend(arc(0.4, -0.25, 0.42, 200.0, 390.0));
            pts.push(at(0.0, 0.8));
            if icon == Icon::HeartFull {
                // Two lobes and a point, each convex.
                dot(-0.4, -0.25, 0.43);
                dot(0.4, -0.25, 0.43);
                fill(&[(-0.76, -0.02), (0.0, -0.35), (0.76, -0.02), (0.0, 0.8)]);
            } else {
                p.add(Shape::closed_line(pts, Stroke::new(w * 0.9, color)));
            }
        }
        Icon::Star | Icon::StarFull => {
            let pts: Vec<Pos2> = (0..10)
                .map(|i| {
                    let a = (i as f32 * 36.0 - 90.0).to_radians();
                    let r = if i % 2 == 0 { 0.9 } else { 0.4 };
                    at(r * a.cos(), r * a.sin())
                })
                .collect();
            if icon == Icon::StarFull {
                // Each arm is a triangle from the centre, so no concave shape is filled.
                for i in (0..10).step_by(2) {
                    let pick = |k: usize| pts.get(k % 10).copied().unwrap_or(c);
                    p.add(Shape::convex_polygon(vec![pick(i + 9), pick(i), pick(i + 1), c], color, Stroke::NONE));
                }
            } else {
                dot(0.0, 0.0, 0.14);
            }
        }
        Icon::Cloud => {
            let mut pts = arc(-0.45, 0.2, 0.4, 90.0, 260.0);
            pts.extend(arc(0.0, -0.15, 0.5, 200.0, 340.0));
            pts.extend(arc(0.5, 0.2, 0.4, -80.0, 90.0));
            p.add(Shape::closed_line(pts, Stroke::new(w * 0.8, color)));
        }
        Icon::Missing => {
            circle(0.0, 0.0, 0.8);
            seg((0.0, -0.4), (0.0, 0.1));
            dot(0.0, 0.42, 0.1);
        }
        Icon::Plus => {
            seg((-0.65, 0.0), (0.65, 0.0));
            seg((0.0, -0.65), (0.0, 0.65));
        }
        Icon::Search => {
            circle(-0.15, -0.15, 0.55);
            seg((0.27, 0.27), (0.8, 0.8));
        }
        Icon::Note => {
            dot(-0.4, 0.55, 0.3);
            seg((-0.15, 0.55), (-0.15, -0.8));
            seg((-0.15, -0.75), (0.6, -0.45));
        }
        Icon::Disc => {
            circle(0.0, 0.0, 0.85);
            circle(0.0, 0.0, 0.25);
        }
        Icon::Person => {
            circle(0.0, -0.4, 0.38);
            path(arc(0.0, 0.85, 0.75, 180.0, 360.0));
        }
        Icon::Tag => {
            p.add(Shape::closed_line(
                vec![at(-0.8, -0.7), at(0.1, -0.7), at(0.85, 0.1), at(0.1, 0.8), at(-0.8, 0.0)],
                line,
            ));
            dot(-0.4, -0.3, 0.13);
        }
        Icon::Clock => {
            circle(0.0, 0.0, 0.8);
            path(vec![at(0.0, -0.5), at(0.0, 0.0), at(0.35, 0.2)]);
        }
        Icon::List => {
            for y in [-0.55, 0.0] {
                seg((-0.8, y), (0.8, y));
            }
            seg((-0.8, 0.55), (0.1, 0.55));
            dot(0.6, 0.55, 0.22);
        }
        Icon::Smart | Icon::Gear => {
            circle(0.0, 0.0, 0.45);
            for i in 0..8 {
                let a = (i as f32 * 45.0).to_radians();
                seg((0.55 * a.cos(), 0.55 * a.sin()), (0.85 * a.cos(), 0.85 * a.sin()));
            }
        }
        Icon::Waves => {
            dot(0.0, 0.0, 0.18);
            for r in [0.45, 0.8] {
                path(arc(0.0, 0.0, r, -50.0, 50.0));
                path(arc(0.0, 0.0, r, 130.0, 230.0));
            }
        }
        Icon::Close => {
            seg((-0.5, -0.5), (0.5, 0.5));
            seg((-0.5, 0.5), (0.5, -0.5));
        }
        Icon::Up => path(vec![at(-0.5, 0.25), at(0.0, -0.3), at(0.5, 0.25)]),
        Icon::Down => path(vec![at(-0.5, -0.25), at(0.0, 0.3), at(0.5, -0.25)]),
        Icon::Fold | Icon::Grow => {
            p.rect_stroke(Rect::from_two_pos(at(-0.8, -0.6), at(0.8, 0.6)), s * 0.08, line, egui::StrokeKind::Middle);
            if icon == Icon::Fold {
                bar(-0.05, 0.0, 0.8, 0.6);
            } else {
                seg((-0.8, -0.2), (0.8, -0.2));
            }
        }
    }
}

/// A button that shows an icon. `name` is what it is called for assistive technology and in its
/// tooltip.
pub fn button(ui: &mut Ui, icon: Icon, size: f32, name: &str, color: Color32) -> Response {
    let side = size.max(24.0);
    let (rect, r) = ui.allocate_exact_size(Vec2::splat(side), Sense::click());
    paint_button(ui, rect, &r, icon, size, color);
    r.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, ui.is_enabled(), name));
    r.on_hover_text(name)
}

/// A button that is on or off.
pub fn toggle(ui: &mut Ui, icon: Icon, size: f32, name: &str, on: bool, color: Color32, off: Color32) -> Response {
    let side = size.max(24.0);
    let (rect, r) = ui.allocate_exact_size(Vec2::splat(side), Sense::click());
    paint_button(ui, rect, &r, icon, size, if on { color } else { off });
    if on {
        // Not by colour alone: a dot below an active button.
        ui.painter().circle_filled(pos2(rect.center().x, rect.bottom() - 1.0), 2.0, color);
    }
    r.widget_info(|| egui::WidgetInfo::selected(egui::Role::Button, ui.is_enabled(), on, name));
    r.on_hover_text(name)
}

fn paint_button(ui: &Ui, rect: Rect, r: &Response, icon: Icon, size: f32, color: Color32) {
    if !ui.is_rect_visible(rect) {
        return;
    }
    let v = ui.visuals();
    if r.hovered() || r.is_pointer_button_down_on() {
        ui.painter().rect_filled(rect, 5.0, v.widgets.hovered.weak_bg_fill);
    }
    if r.has_focus() {
        ui.painter().rect_stroke(rect, 5.0, v.widgets.active.bg_stroke, egui::StrokeKind::Inside);
    }
    draw(ui.painter(), Rect::from_center_size(rect.center(), Vec2::splat(size * 0.66)), icon, color);
}

/// A stand-in for an album without a picture.
pub fn blank_cover(p: &Painter, rect: Rect, key: &str, dark: bool) {
    let h = key.bytes().fold(7u32, |h, b| h.wrapping_mul(31).wrapping_add(u32::from(b)));
    let base = if dark { 52 } else { 205 };
    let tint = |shift: u32| u8::try_from(base + (h >> shift) % 26).unwrap_or(128);
    p.rect_filled(rect, 4.0, Color32::from_rgb(tint(0), tint(5), tint(10)));
    let ink = if dark { Color32::from_gray(150) } else { Color32::from_gray(110) };
    draw(p, Rect::from_center_size(rect.center(), rect.size() * 0.38), Icon::Note, ink);
}
