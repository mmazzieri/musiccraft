//! Playing on a Spotify device: commands go out in order on one thread, which also asks the
//! device now and then what it is doing.

use crate::lock;
use musiccraft_services::spotify::{Playback, Spotify};
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender, channel};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// How often a playing device is asked what it is doing.
const POLL: Duration = Duration::from_secs(3);
/// A song this close to its end, when the device moves on, has ended.
const END_MS: u64 = 8_000;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Status {
    #[default]
    Idle,
    Loading,
    Playing,
    Paused,
    Ended,
    Failed,
}

#[derive(Debug, Clone)]
pub struct State {
    pub status: Status,
    pub uri: String,
    position_ms: u64,
    at: Instant,
    duration_ms: u64,
    /// The picture of what plays, as the service gives it now.
    pub art: String,
    pub error: String,
}

impl State {
    /// The position now, reckoned from the last answer.
    pub fn position_ms(&self) -> u64 {
        if self.status == Status::Playing {
            let since = u64::try_from(self.at.elapsed().as_millis()).unwrap_or(0);
            self.position_ms.saturating_add(since).min(self.duration_ms.max(self.position_ms))
        } else {
            self.position_ms
        }
    }
}

enum Cmd {
    Play { uri: String, device: String },
    Resume,
    Pause,
    Seek(u64),
    Volume(u32),
}

pub struct Remote {
    tx: Sender<Cmd>,
    state: Arc<Mutex<State>>,
}

impl Remote {
    pub fn new(account: Arc<Spotify>) -> Remote {
        let (tx, rx) = channel();
        let state = Arc::new(Mutex::new(State {
            status: Status::Idle,
            uri: String::new(),
            position_ms: 0,
            at: Instant::now(),
            duration_ms: 0,
            art: String::new(),
            error: String::new(),
        }));
        let shared = state.clone();
        if let Err(e) = crate::spawn("spotify", move || run(&account, &rx, &shared)) {
            let mut s = lock(&state);
            s.status = Status::Failed;
            s.error = e;
        }
        Remote { tx, state }
    }

    fn send(&self, cmd: Cmd) {
        // The thread ends only when this handle goes.
        self.tx.send(cmd).ok();
    }

    pub fn state(&self) -> State {
        lock(&self.state).clone()
    }

    pub fn play(&self, uri: &str, duration_ms: u64, device: &str) {
        {
            let mut s = lock(&self.state);
            s.status = Status::Loading;
            uri.clone_into(&mut s.uri);
            s.position_ms = 0;
            s.duration_ms = duration_ms;
            s.art.clear();
            s.error.clear();
        }
        self.send(Cmd::Play { uri: uri.to_owned(), device: device.to_owned() });
    }

    fn set(&self, status: Status) {
        let mut s = lock(&self.state);
        s.position_ms = s.position_ms();
        s.at = Instant::now();
        s.status = status;
    }

    pub fn resume(&self) {
        self.set(Status::Playing);
        self.send(Cmd::Resume);
    }

    pub fn pause(&self) {
        self.set(Status::Paused);
        self.send(Cmd::Pause);
    }

    /// Pauses the device when it is playing for us, and stops following it.
    pub fn stop(&self) {
        let was = lock(&self.state).status;
        if matches!(was, Status::Playing | Status::Loading) {
            self.send(Cmd::Pause);
        }
        self.set(Status::Idle);
    }

    pub fn seek(&self, ms: u64) {
        {
            let mut s = lock(&self.state);
            s.position_ms = ms;
            s.at = Instant::now();
        }
        self.send(Cmd::Seek(ms));
    }

    pub fn volume(&self, percent: u32) {
        self.send(Cmd::Volume(percent));
    }
}

fn run(account: &Spotify, rx: &Receiver<Cmd>, state: &Mutex<State>) {
    let mut quiet_until = Instant::now();
    loop {
        match rx.recv_timeout(POLL) {
            Ok(mut cmd) => {
                // Of several volume changes in a row only the last is sent.
                while let Cmd::Volume(_) = cmd {
                    match rx.recv_timeout(Duration::from_millis(250)) {
                        Ok(next) => cmd = next,
                        Err(_) => break,
                    }
                }
                let result = match &cmd {
                    Cmd::Play { uri, device } => account.play(uri, device),
                    Cmd::Resume => account.resume(),
                    Cmd::Pause => account.pause(),
                    Cmd::Seek(ms) => account.seek(*ms),
                    Cmd::Volume(p) => account.volume(*p),
                };
                let mut s = lock(state);
                match result {
                    Ok(()) => {
                        if let Cmd::Play { uri, .. } = &cmd
                            && *uri == s.uri
                            && s.status == Status::Loading
                        {
                            s.status = Status::Playing;
                            s.at = Instant::now();
                        }
                    }
                    // Many devices refuse to change the volume; that is not a reason to stop.
                    Err(_) if matches!(cmd, Cmd::Volume(_)) => {}
                    Err(e) => {
                        s.status = Status::Failed;
                        s.error = e.to_string();
                    }
                }
                // The device takes a moment to report what it was just told.
                quiet_until = Instant::now() + Duration::from_millis(2500);
            }
            Err(RecvTimeoutError::Timeout) => {
                let status = lock(state).status;
                if !matches!(status, Status::Playing | Status::Paused) || Instant::now() < quiet_until {
                    continue;
                }
                if let Ok(answer) = account.playback() {
                    follow(&mut lock(state), answer);
                }
            }
            Err(RecvTimeoutError::Disconnected) => return,
        }
    }
}

/// Takes in what the device says.
fn follow(s: &mut State, answer: Option<Playback>) {
    if !matches!(s.status, Status::Playing | Status::Paused) {
        return;
    }
    let near_end = s.duration_ms > 0 && s.position_ms().saturating_add(END_MS) >= s.duration_ms;
    match answer {
        Some(p) if p.uri == s.uri && (p.playing || p.position_ms > 0 || !near_end) => {
            s.position_ms = p.position_ms;
            s.at = Instant::now();
            s.status = if p.playing { Status::Playing } else { Status::Paused };
            s.art = p.entry.art;
            if p.entry.duration_ms > 0 {
                s.duration_ms = p.entry.duration_ms;
            }
        }
        // It stopped, or went on to something of its own, where our song ended.
        _ if near_end && s.status == Status::Playing => s.status = Status::Ended,
        // Someone chose something else on the device: leave it to them.
        Some(p) if p.uri != s.uri => s.status = Status::Idle,
        _ => {
            s.position_ms = s.position_ms();
            s.status = Status::Paused;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{State, Status, follow};
    use musiccraft_services::spotify::Playback;
    use std::time::Instant;

    fn at(position_ms: u64) -> State {
        State {
            status: Status::Playing,
            uri: "a".into(),
            position_ms,
            at: Instant::now(),
            duration_ms: 200_000,
            art: String::new(),
            error: String::new(),
        }
    }

    fn says(uri: &str, playing: bool, position_ms: u64) -> Option<Playback> {
        Some(Playback { uri: uri.into(), playing, position_ms, ..Playback::default() })
    }

    /// The queue goes on only when our song reached its end. A pause on the phone pauses; a song
    /// chosen on the phone is left alone.
    #[test]
    fn the_end_of_a_song_is_told_from_a_pause_and_a_takeover() {
        let status = |from: u64, answer| {
            let mut s = at(from);
            follow(&mut s, answer);
            s.status
        };
        assert_eq!(status(198_000, says("a", false, 0)), Status::Ended);
        assert_eq!(status(198_000, says("b", true, 1_000)), Status::Ended);
        assert_eq!(status(198_000, None), Status::Ended);
        assert_eq!(status(50_000, says("a", false, 50_000)), Status::Paused);
        assert_eq!(status(50_000, says("b", true, 1_000)), Status::Idle);
        assert_eq!(status(50_000, None), Status::Paused);
        assert_eq!(status(50_000, says("a", true, 60_000)), Status::Playing);
    }
}
