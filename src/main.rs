//! MusicCraft, the desktop app.
#![forbid(unsafe_code)]
#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::unimplemented,
    clippy::todo,
    clippy::unreachable
)]
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use musiccraft_ui::{App, Options};
use std::path::PathBuf;

/// Fixed install identity; also the Wayland app id.
const APP_ID: &str = "it.mauromazzieri.musiccraft";

/// Window icon, 64x64 RGBA, written by tools/icons.py.
const ICON: &[u8] = include_bytes!("../assets/app-icon/window-64.rgba");

const HELP: &str = "MusicCraft

  musiccraft                        open the window
  musiccraft --demo                 open it with a demonstration library
  musiccraft --type-paths           ask for paths in the window, not in the system's file chooser
  musiccraft --screenshot FILE.png  save a picture of the window and end
      --view NAME     albums, album, lyrics, artists, songs, genres, radio, services or welcome
      --theme NAME    light or dark
      --language CODE en or it
  musiccraft --version";

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut options = Options::default();
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        let mut value = || args.next().ok_or(format!("{a} needs a value"));
        match a.as_str() {
            "--demo" => options.demo = true,
            "--type-paths" => options.type_paths = true,
            "--screenshot" => options.screenshot = Some(PathBuf::from(value()?)),
            "--view" => options.view = value()?,
            "--theme" => options.theme = value()?,
            "--language" => options.language = value()?,
            "--version" | "-V" => {
                println!("MusicCraft {}", env!("CARGO_PKG_VERSION"));
                return Ok(());
            }
            "--help" | "-h" => {
                println!("{HELP}");
                return Ok(());
            }
            other => return Err(format!("{other} is not an option of MusicCraft\n\n{HELP}").into()),
        }
    }
    let shot = options.screenshot.is_some();
    let mut viewport = egui::ViewportBuilder::default()
        .with_title("MusicCraft")
        .with_app_id(APP_ID)
        .with_icon(egui::IconData { rgba: ICON.to_vec(), width: 64, height: 64 })
        .with_inner_size([1180.0, 760.0])
        .with_min_inner_size([760.0, 480.0])
        .with_drag_and_drop(true);
    if shot {
        viewport = viewport.with_active(false);
        if cfg!(windows) {
            viewport = viewport.with_window_level(egui::WindowLevel::AlwaysOnBottom);
        }
    }
    let mut native = eframe::NativeOptions { viewport, persist_window: !shot, ..Default::default() };
    if shot {
        // A picture is always of the same size, and leaves no window position behind.
        native.persistence_path = Some(std::env::temp_dir().join("musiccraft-shot.ron"));
    }
    #[cfg(target_os = "macos")]
    if shot {
        native.event_loop_builder = Some(Box::new(|b| {
            use winit::platform::macos::EventLoopBuilderExtMacOS;
            b.with_activate_ignoring_other_apps(false);
        }));
    }
    eframe::run_native("MusicCraft", native, Box::new(move |cc| Ok(Box::new(App::new(&cc.egui_ctx, options)))))?;
    Ok(())
}
