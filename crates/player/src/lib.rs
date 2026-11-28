//! Audio playback: files and network streams, on a thread of its own.
//!
//! The sound device is opened at the first play, and a computer without one gets an error
//! message, never a crash. The next song can be handed over early ([`Player::preload`]), so
//! songs follow each other without a gap.
#![forbid(unsafe_code)]
#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::unimplemented,
    clippy::todo,
    clippy::unreachable
)]

mod health;
pub mod stream;

use rodio::decoder::DecoderBuilder;
use rodio::source::Source;
use std::path::PathBuf;
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender, channel};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

/// What to play.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Input {
    File(PathBuf),
    /// An address; `live` for an endless stream such as a radio station.
    Url {
        url: String,
        live: bool,
    },
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Status {
    #[default]
    Stopped,
    /// Opening a file or filling a stream's buffer.
    Loading,
    Playing,
    Paused,
    /// The song ended and nothing was preloaded.
    Ended,
    Failed,
}

/// What the player is doing now.
#[derive(Debug, Clone, Default)]
pub struct State {
    pub status: Status,
    /// The number given by the `load` or `preload` call of the current song.
    pub current: u64,
    pub position: Duration,
    pub error: String,
    /// The song a radio station says it is playing.
    pub stream_title: String,
}

enum Cmd {
    Load { input: Input, number: u64, start: Duration, paused: bool },
    Preload { input: Input, number: u64 },
    Opened { number: u64, cancel: stream::Cancel, result: Result<Opened, String> },
    Play,
    Pause,
    Seek(Duration),
    Volume(f32),
    Stop,
}

/// A handle to the playback thread.
pub struct Player {
    tx: Sender<Cmd>,
    state: Arc<Mutex<State>>,
    number: u64,
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

impl Default for Player {
    fn default() -> Self {
        Self::new()
    }
}

impl Player {
    pub fn new() -> Player {
        let (tx, rx) = channel();
        let state = Arc::new(Mutex::new(State::default()));
        let (thread_state, own) = (state.clone(), tx.clone());
        let spawned =
            std::thread::Builder::new().name("audio".to_owned()).spawn(move || Thread::new(thread_state, own).run(&rx));
        if let Err(e) = spawned {
            let mut s = lock(&state);
            s.status = Status::Failed;
            s.error = e.to_string();
        }
        Player { tx, state, number: 0 }
    }

    fn send(&self, cmd: Cmd) {
        // The thread ends only with the program.
        self.tx.send(cmd).ok();
    }

    /// Plays `input` from `start`, instead of what is playing. Returns the song's number.
    pub fn load(&mut self, input: Input, start: Duration, paused: bool) -> u64 {
        self.number += 1;
        let number = self.number;
        {
            let mut s = lock(&self.state);
            *s = State { status: Status::Loading, current: number, position: start, ..State::default() };
        }
        self.send(Cmd::Load { input, number, start, paused });
        number
    }

    /// Gets the next song ready, to follow the current one without a gap.
    pub fn preload(&mut self, input: Input) -> u64 {
        self.number += 1;
        self.send(Cmd::Preload { input, number: self.number });
        self.number
    }

    pub fn play(&self) {
        self.send(Cmd::Play);
    }

    pub fn pause(&self) {
        self.send(Cmd::Pause);
    }

    pub fn seek(&self, to: Duration) {
        self.send(Cmd::Seek(to));
    }

    /// 0 to 1.
    pub fn set_volume(&self, volume: f32) {
        self.send(Cmd::Volume(volume.clamp(0.0, 1.0)));
    }

    pub fn stop(&self) {
        self.send(Cmd::Stop);
    }

    pub fn state(&self) -> State {
        lock(&self.state).clone()
    }
}

type Boxed = Box<dyn Source + Send>;

struct Opened {
    source: Boxed,
    title: Arc<Mutex<String>>,
}

/// A song being opened on a thread of its own, because a slow station can take long to answer.
struct Opening {
    number: u64,
    cancel: stream::Cancel,
    start: Duration,
    paused: bool,
    /// It follows the current song instead of replacing it.
    next: bool,
}

struct Thread {
    state: Arc<Mutex<State>>,
    tx: Sender<Cmd>,
    opening: Option<Opening>,
    device: Option<rodio::stream::MixerDeviceSink>,
    /// The errors of the device's stream, while it is open.
    health: health::Health,
    player: Option<rodio::Player>,
    volume: f32,
    current: u64,
    next: Option<(u64, Arc<Mutex<String>>, stream::Cancel)>,
    title: Arc<Mutex<String>>,
    cancel: stream::Cancel,
}

impl Thread {
    fn new(state: Arc<Mutex<State>>, tx: Sender<Cmd>) -> Thread {
        Thread {
            state,
            tx,
            opening: None,
            device: None,
            health: health::Health::default(),
            player: None,
            volume: 1.0,
            current: 0,
            next: None,
            title: Arc::default(),
            cancel: stream::Cancel::default(),
        }
    }

    fn run(mut self, rx: &Receiver<Cmd>) {
        loop {
            match rx.recv_timeout(Duration::from_millis(60)) {
                Ok(cmd) => self.handle(cmd),
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => break,
            }
            self.watch();
        }
        self.cancel.cancel();
    }

    fn fail(&mut self, number: u64, error: String) {
        let mut s = lock(&self.state);
        if s.current == number {
            s.status = Status::Failed;
            s.error = error;
        }
    }

    fn set_status(&self, status: Status) {
        lock(&self.state).status = status;
    }

    fn drop_player(&mut self) {
        self.cancel.cancel();
        if let Some(o) = self.opening.take() {
            o.cancel.cancel();
        }
        if let Some((_, _, cancel)) = self.next.take() {
            cancel.cancel();
        }
        if let Some(p) = self.player.take() {
            p.stop();
        }
    }

    fn handle(&mut self, cmd: Cmd) {
        match cmd {
            Cmd::Load { input, number, start, paused } => {
                self.drop_player();
                self.current = number;
                self.begin(input, Opening { number, cancel: stream::Cancel::default(), start, paused, next: false });
            }
            Cmd::Preload { input, number } => {
                let idle = self.player.as_ref().is_none_or(rodio::Player::empty);
                if self.next.is_none() && self.opening.is_none() && !idle {
                    let cancel = stream::Cancel::default();
                    self.begin(input, Opening { number, cancel, start: Duration::ZERO, paused: false, next: true });
                }
            }
            Cmd::Opened { number, cancel, result } => {
                let Some(o) = self.opening.take_if(|o| o.number == number) else {
                    cancel.cancel();
                    return;
                };
                match result {
                    Ok(opened) if o.next => {
                        if let Some(p) = &self.player {
                            p.append(opened.source);
                            self.next = Some((number, opened.title, cancel));
                        }
                    }
                    Ok(opened) => {
                        if let Err(e) = self.play(opened, cancel, &o) {
                            self.fail(number, e);
                        }
                    }
                    // A next song that cannot be opened is reported when its turn comes.
                    Err(_) if o.next => {}
                    Err(e) => self.fail(number, e),
                }
            }
            Cmd::Play => {
                if let Some(p) = &self.player {
                    p.play();
                    self.set_status(Status::Playing);
                }
            }
            Cmd::Pause => {
                if let Some(p) = &self.player {
                    p.pause();
                    self.set_status(Status::Paused);
                }
            }
            Cmd::Seek(to) => {
                if let Some(p) = &self.player
                    && p.try_seek(to).is_ok()
                {
                    lock(&self.state).position = to;
                }
            }
            Cmd::Volume(v) => {
                self.volume = v;
                if let Some(p) = &self.player {
                    // Loudness is heard on a curve: half way along the slider sounds half as loud.
                    p.set_volume(v * v);
                }
            }
            Cmd::Stop => {
                self.drop_player();
                let mut s = lock(&self.state);
                *s = State { current: s.current, ..State::default() };
            }
        }
    }

    /// Opens a song on another thread, which answers with `Opened`.
    fn begin(&mut self, input: Input, o: Opening) {
        let (tx, cancel, number) = (self.tx.clone(), o.cancel.clone(), o.number);
        self.opening = Some(o);
        let spawned = std::thread::Builder::new().name("open".to_owned()).spawn(move || {
            let result = open(&input, &cancel);
            // The audio thread ends only with the program.
            tx.send(Cmd::Opened { number, cancel, result }).ok();
        });
        if let Err(e) = spawned {
            self.opening = None;
            self.fail(number, e.to_string());
        }
    }

    fn play(&mut self, opened: Opened, cancel: stream::Cancel, o: &Opening) -> Result<(), String> {
        let (start, paused) = (o.start, o.paused);
        if self.device.is_none() {
            let (mut device, health) = open_device()?;
            device.log_on_drop(false);
            self.device = Some(device);
            self.health = health;
        }
        let Some(device) = &self.device else { return Err("no sound device".to_owned()) };
        let player = rodio::Player::connect_new(device.mixer());
        player.pause();
        player.set_volume(self.volume * self.volume);
        player.append(opened.source);
        if !start.is_zero() {
            player.play();
            // A stream that cannot seek starts from its beginning.
            player.try_seek(start).ok();
        }
        if paused {
            player.pause();
        } else {
            player.play();
        }
        self.title = opened.title;
        self.cancel = cancel;
        self.player = Some(player);
        self.set_status(if paused { Status::Paused } else { Status::Playing });
        Ok(())
    }

    fn watch(&mut self) {
        if self.device.is_some() && self.health.broken() {
            self.lose_device();
        }
        let Some(p) = &self.player else { return };
        if let Some((number, title, cancel)) = self.next.take() {
            if p.len() <= 1 {
                self.current = number;
                self.title = title;
                self.cancel = cancel;
                let mut s = lock(&self.state);
                s.current = number;
                s.stream_title.clear();
            } else {
                self.next = Some((number, title, cancel));
            }
        }
        let mut s = lock(&self.state);
        if s.current != self.current {
            return;
        }
        s.position = p.get_pos();
        let title = lock(&self.title);
        if *title != s.stream_title {
            s.stream_title.clone_from(&title);
        }
        if p.empty() && s.status == Status::Playing {
            s.status = Status::Ended;
        }
    }
}

impl Thread {
    /// Lets a broken device go, so that its stream stops and the next play opens it again.
    fn lose_device(&mut self) {
        self.drop_player();
        self.device = None;
        self.health = health::Health::default();
        let mut s = lock(&self.state);
        // A song asked for but not yet started is left to its own load.
        if s.current == self.current && matches!(s.status, Status::Playing | Status::Paused | Status::Loading) {
            s.status = Status::Failed;
            s.error = "the sound device stopped working; play the song again to retry".to_owned();
        }
    }
}

/// The default output device, or else another one that works, with our own error callback.
/// Each try has a count of its own, so that what a failed try reported is not held against the
/// device that opens.
fn open_device() -> Result<(rodio::stream::MixerDeviceSink, health::Health), String> {
    use rodio::DeviceTrait;
    use rodio::cpal::traits::HostTrait;
    let attempt = |builder: rodio::stream::DeviceSinkBuilder| {
        let health = health::Health::default();
        builder.with_error_callback(health.callback()).open_sink_or_fallback().map(|sink| (sink, health))
    };
    let first = rodio::stream::DeviceSinkBuilder::from_default_device().and_then(attempt);
    first.or_else(|original| {
        let devices = rodio::cpal::default_host().output_devices().map_err(|_| original.to_string())?;
        devices
            .filter(|d| d.description().is_ok_and(|desc| desc.driver().is_some_and(|driver| driver != "null")))
            .find_map(|d| rodio::stream::DeviceSinkBuilder::from_device(d).and_then(attempt).ok())
            .ok_or_else(|| original.to_string())
    })
}

fn open(input: &Input, cancel: &stream::Cancel) -> Result<Opened, String> {
    match input {
        Input::File(path) => {
            let file = std::fs::File::open(path).map_err(|e| e.to_string())?;
            let len = file.metadata().map_err(|e| e.to_string())?.len();
            let mut b = DecoderBuilder::new()
                .with_data(std::io::BufReader::with_capacity(128 * 1024, file))
                .with_byte_len(len)
                .with_seekable(true)
                .with_gapless(true);
            if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
                b = b.with_hint(&ext.to_ascii_lowercase());
            }
            let source = b.build().map_err(|e| e.to_string())?;
            Ok(Opened { source: Box::new(source), title: Arc::default() })
        }
        Input::Url { url, live } => {
            let s = stream::open(url, *live, cancel)?;
            let mut b = DecoderBuilder::new().with_data(s.reader).with_seekable(!live).with_gapless(true);
            if let Some(len) = s.len.filter(|_| !live) {
                b = b.with_byte_len(len);
            }
            if !s.mime.is_empty() {
                b = b.with_mime_type(&s.mime);
            }
            let source = b.build().map_err(|e| {
                cancel.cancel();
                e.to_string()
            })?;
            s.started.store(true, std::sync::atomic::Ordering::Relaxed);
            Ok(Opened { source: Box::new(source), title: s.title })
        }
    }
}
