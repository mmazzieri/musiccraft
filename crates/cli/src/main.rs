//! MusicCraft without a window: every command of the engine, with JSON answers.
#![forbid(unsafe_code)]
#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::unimplemented,
    clippy::todo,
    clippy::unreachable
)]

use musiccraft_engine::settings::data_dir;
use musiccraft_engine::{Command, Engine, NowStatus};
use musiccraft_library::Item;
use musiccraft_library::query::Sort;
use serde_json::Value;
use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Duration;

const HELP: &str = "MusicCraft without a window

  musiccraft-cli [--library FOLDER] COMMAND

Commands
  info                     the library: folder, songs, computers, files it could not read
  scan                     look for new files
  songs [WORDS]            songs that match, as JSON
  albums | playlists | stations
  add PATH...              copy files or folders into the library
  play WORDS               play what matches, until it ends
  run JSON                 any command of the engine, for example
                           {\"cmd\":\"playlist_new\",\"name\":\"Mix\"}
  demo FOLDER              write a demonstration library

Without --library, the library chosen in the window is used.";

/// How long background work may take before the program ends.
const WAIT: Duration = Duration::from_secs(3600);

fn main() -> ExitCode {
    match run() {
        Ok(Value::Null) => ExitCode::SUCCESS,
        Ok(v) => {
            println!("{}", serde_json::to_string_pretty(&v).unwrap_or_default());
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("{e}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<Value, String> {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    let mut library = None;
    if args.first().is_some_and(|a| a == "--library") {
        library = Some(PathBuf::from(args.get(1).ok_or("--library needs a folder")?));
        args.drain(..2);
    }
    let Some(name) = args.first().cloned() else { return Err(HELP.to_owned()) };
    let rest = args.get(1..).unwrap_or_default().to_vec();
    match name.as_str() {
        "--help" | "-h" | "help" => {
            println!("{HELP}");
            return Ok(Value::Null);
        }
        "--version" | "-V" => {
            println!("MusicCraft {}", env!("CARGO_PKG_VERSION"));
            return Ok(Value::Null);
        }
        "demo" => {
            let dir = PathBuf::from(rest.first().ok_or("demo needs a folder")?);
            musiccraft_engine::demo::write_library(&dir)?;
            return Ok(Value::Null);
        }
        _ => {}
    }
    let dir = data_dir().ok_or("there is no data folder on this system")?;
    let mut engine = match &library {
        // A library named here is opened without changing the one chosen in the window.
        Some(path) => Engine::isolated(&dir.join("cli"), Some(path.clone())),
        None => Engine::new(&dir),
    };
    if engine.library().is_none() {
        let why = engine.notices.pop().map(|n| n.detail).unwrap_or_default();
        return Err(format!("no library is open; choose one in the window or with --library. {why}"));
    }
    engine.settle(WAIT);
    let songs = |engine: &mut Engine, words: &[String]| {
        engine.run(Command::Songs { search: words.join(" "), sort: Sort::Artist, descending: false, limit: 0 })
    };
    let answer = match name.as_str() {
        "info" => engine.run(Command::Info),
        "scan" => engine.run(Command::Info),
        "songs" => songs(&mut engine, &rest),
        "albums" => engine.run(Command::Albums),
        "playlists" => engine.run(Command::Playlists),
        "stations" => engine.run(Command::Stations),
        "add" => engine.run(Command::Import { paths: rest.iter().map(PathBuf::from).collect() }),
        "run" => {
            let json = rest.first().ok_or("run needs a command in JSON")?;
            let command: Command = serde_json::from_str(json).map_err(|e| e.to_string())?;
            engine.run(command)
        }
        "play" => {
            let found = songs(&mut engine, &rest)?;
            let ids = found.as_array().into_iter().flatten().filter_map(|t| t.get("id")?.as_str());
            let items: Vec<Item> = ids.map(Item::local).collect();
            if items.is_empty() {
                return Err("nothing in the library matches".to_owned());
            }
            engine.run(Command::Play { items, start: 0 })?;
            play(&mut engine)
        }
        other => Err(format!("{other} is not a command\n\n{HELP}")),
    }?;
    engine.settle(WAIT);
    match engine.notices.iter().find(|n| n.error) {
        Some(n) => Err(n.text.replacen("{}", &n.detail, 1)),
        None => Ok(answer),
    }
}

/// Follows the queue to its end, naming each song.
fn play(engine: &mut Engine) -> Result<Value, String> {
    let mut last = None;
    loop {
        engine.tick();
        let now = engine.now();
        if now.item.is_none() || now.status == NowStatus::Failed {
            return Ok(Value::Null);
        }
        if now.item != last {
            println!("{} — {}", now.meta.title, now.meta.artist);
            last = now.item;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}
