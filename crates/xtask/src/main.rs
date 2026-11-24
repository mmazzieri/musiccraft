//! `cargo xtask ci`: fmt, clippy and tests for the whole workspace.
#![forbid(unsafe_code)]

use std::process::{Command, ExitCode};

fn run(args: &[&str]) -> bool {
    println!("$ cargo {}", args.join(" "));
    Command::new(env!("CARGO")).args(args).status().map(|s| s.success()).unwrap_or(false)
}

fn main() -> ExitCode {
    let task = std::env::args().nth(1).unwrap_or_default();
    let ok = match task.as_str() {
        "ci" => {
            run(&["fmt", "--all", "--check"])
                && run(&["clippy", "--workspace", "--all-targets", "--", "-D", "warnings"])
                && run(&["test", "--workspace"])
        }
        _ => {
            eprintln!("usage: cargo xtask ci");
            false
        }
    };
    if ok { ExitCode::SUCCESS } else { ExitCode::FAILURE }
}
