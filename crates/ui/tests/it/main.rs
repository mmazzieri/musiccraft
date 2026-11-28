//! The window driven by keyboard through its accessibility tree.

use egui::accesskit::Role;
use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT, Queryable};
use musiccraft_ui::{App, Options};
use std::sync::{Mutex, MutexGuard, PoisonError};

/// The windows share one demonstration library on disk, so only one is open at a time.
static ONE: Mutex<()> = Mutex::new(());

fn alone() -> MutexGuard<'static, ()> {
    ONE.lock().unwrap_or_else(PoisonError::into_inner)
}

fn window(view: &str) -> Harness<'static, App> {
    window_with(view, false)
}

fn window_with(view: &str, type_paths: bool) -> Harness<'static, App> {
    let options = Options {
        demo: true,
        // With a picture asked for, the demonstration shows a song without opening a sound device.
        screenshot: Some(std::env::temp_dir().join("musiccraft-test-never-taken.png")),
        view: view.to_owned(),
        language: "en".to_owned(),
        theme: "light".to_owned(),
        type_paths,
    };
    let mut h = Harness::builder().with_size([1180.0, 760.0]).build_eframe(move |cc| App::new(&cc.egui_ctx, options));
    h.run_steps(4);
    h
}

fn rows(h: &Harness<'_, App>) -> Vec<(String, bool)> {
    h.query_all_by_role(Role::Row)
        .map(|n| {
            let n = n.accesskit_node();
            (n.label().unwrap_or_default(), n.is_selected().unwrap_or(false))
        })
        .collect()
}

/// In the song table the arrow keys move both the selection and the real focus, Shift extends,
/// and each row's name says the song, the artist and the album.
#[test]
fn the_song_table_works_by_keyboard() {
    let _one = alone();
    let mut h = window("songs");
    let all = rows(&h);
    assert!(all.len() > 10, "{all:?}");
    assert!(all[0].0.matches(", ").count() >= 3, "{:?}", all[0]);
    h.query_all_by_role(Role::Row).next().unwrap().focus();
    h.run_steps(2);
    h.key_press(egui::Key::ArrowDown);
    h.run_steps(2);
    assert_eq!(rows(&h).iter().map(|r| r.1).collect::<Vec<_>>()[..3], [false, true, false]);
    let focused = h.query_all_by_role(Role::Row).position(|n| n.accesskit_node().is_focused());
    assert_eq!(focused, Some(1));
    h.key_press_modifiers(egui::Modifiers::SHIFT, egui::Key::ArrowDown);
    h.run_steps(2);
    assert_eq!(rows(&h).iter().map(|r| r.1).collect::<Vec<_>>()[..4], [false, true, true, false]);
}

/// The new-playlist dialog opens from the keyboard, takes the focus in its name field, and Esc
/// closes it without making a playlist.
#[test]
fn a_dialog_takes_the_focus_and_closes_with_escape() {
    let _one = alone();
    let mut h = window("songs");
    let before = h.query_all_by_label("New Playlist").count();
    h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::N);
    h.run_steps(3);
    let field = h.get_by_role_and_label(Role::TextInput, "Name");
    assert!(field.accesskit_node().is_focused());
    h.key_press(egui::Key::Escape);
    h.run_steps(3);
    assert_eq!(h.query_all_by_label("New Playlist").count(), before);
}

/// Every control of the header has its own name, in the person's words.
#[test]
fn the_header_is_named() {
    let _one = alone();
    let h = window("albums");
    for name in ["Previous", "Play", "Next", "Shuffle", "Repeat: off", "Play queue"] {
        assert!(h.query_all_by_role_and_label(Role::Button, name).next().is_some(), "{name}");
    }
    assert!(h.query_all_by_role(Role::SearchInput).next().is_some());
    assert_eq!(h.query_all_by_role(Role::Tab).count(), 3);
}

/// Without the system's file chooser, adding files asks for the path in a dialog of the window,
/// which takes the focus, refuses a path where nothing is (and says so aloud), and closes with Esc.
#[test]
fn a_path_is_typed_when_there_is_no_file_chooser() {
    let _one = alone();
    let mut h = window_with("songs", true);
    h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::O);
    h.run_steps(3);
    let field = h.get_by_role_and_label(Role::TextInput, "File or folder to add");
    assert!(field.accesskit_node().is_focused());
    field.type_text("/no/such/place/for/musiccraft");
    h.run_steps(3);
    // Shown beside the field, and spoken by the live region while the focus stays in the field.
    assert!(h.query_all_by_label("Nothing is at this path.").count() >= 2);
    assert!(
        h.query_all_by_role(Role::Status)
            .any(|n| n.accesskit_node().label().as_deref() == Some("Nothing is at this path."))
    );
    assert!(h.get_by_role_and_label(Role::TextInput, "File or folder to add").accesskit_node().is_focused());
    h.key_press(egui::Key::Escape);
    h.run_steps(3);
    assert!(h.query_by_label("File or folder to add").is_none());
}
