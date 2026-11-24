//! Network audio. A download thread fills a bounded buffer, the decoder reads from it.
//!
//! A song from a server is kept whole in memory, so it can be rewound. A live stream keeps only
//! what has not been played. Titles inside live streams follow the public description of the
//! ICY metadata convention (`icy-metaint`, `StreamTitle='…';`).

use std::collections::VecDeque;
use std::io::{Read, Seek, SeekFrom};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

/// The largest song held in memory.
const MAX_SONG: usize = 512 * 1024 * 1024;
/// How much of a live stream is buffered ahead.
const MAX_LIVE: usize = 4 * 1024 * 1024;
/// How much must arrive before the decoder starts.
const READY: usize = 64 * 1024;
const WAIT: Duration = Duration::from_secs(20);

#[derive(Debug, Clone, Default)]
pub struct Cancel(Arc<AtomicBool>);

impl Cancel {
    pub fn cancel(&self) {
        self.0.store(true, Ordering::Relaxed);
    }

    fn is_set(&self) -> bool {
        self.0.load(Ordering::Relaxed)
    }
}

#[derive(Default)]
struct Data {
    bytes: VecDeque<u8>,
    /// Position of the first byte of `bytes` in the stream.
    base: u64,
    done: bool,
}

#[derive(Default)]
struct Shared {
    data: Mutex<Data>,
    changed: Condvar,
}

impl Shared {
    fn lock(&self) -> MutexGuard<'_, Data> {
        self.data.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// An open stream.
pub struct Stream {
    pub reader: Reader,
    pub len: Option<u64>,
    pub mime: String,
    pub title: Arc<Mutex<String>>,
    /// Set once the decoder has recognised the stream; until then reading ends at a deadline.
    pub started: Arc<AtomicBool>,
}

/// Reads a stream as it arrives.
pub struct Reader {
    shared: Arc<Shared>,
    cancel: Cancel,
    pos: u64,
    len: Option<u64>,
    live: bool,
    started: Arc<AtomicBool>,
    deadline: Instant,
}

/// Opens `url` and waits for its first bytes. `cancel` ends it at any time.
pub fn open(url: &str, live: bool, cancel: &Cancel) -> Result<Stream, String> {
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_connect(Some(Duration::from_secs(10)))
        .timeout_recv_response(Some(Duration::from_secs(15)))
        .user_agent(concat!("MusicCraft/", env!("CARGO_PKG_VERSION")))
        .build()
        .into();
    let mut request = agent.get(url);
    if live {
        request = request.header("Icy-MetaData", "1");
    }
    let response = request.call().map_err(reason)?;
    let header = |name: &str| response.headers().get(name).and_then(|v| v.to_str().ok()).unwrap_or("").to_owned();
    let mime = header("content-type").split(';').next().unwrap_or("").trim().to_ascii_lowercase();
    // Other names stations give to the same formats.
    let mime = match mime.as_str() {
        "audio/aacp" | "audio/x-aac" => "audio/aac".to_owned(),
        "audio/mp3" | "audio/x-mpeg" => "audio/mpeg".to_owned(),
        _ => mime,
    };
    let meta_every = header("icy-metaint").parse::<usize>().ok().filter(|n| (1..=1 << 20).contains(n));
    let len = header("content-length").parse::<u64>().ok().filter(|n| *n > 0 && !live);
    if mime.starts_with("text/") || mime.contains("json") || mime.contains("xml") {
        return Err(format!("the server sent {mime} instead of audio"));
    }
    let shared = Arc::new(Shared::default());
    let cancel = cancel.clone();
    let title = Arc::new(Mutex::new(String::new()));
    let (s, c, t) = (shared.clone(), cancel.clone(), title.clone());
    let body = response.into_body().into_reader();
    std::thread::Builder::new()
        .name("stream".to_owned())
        .spawn(move || {
            match meta_every {
                Some(n) => download(Icy::new(body, n, t), &s, &c, live),
                None => download(body, &s, &c, live),
            }
            s.lock().done = true;
            s.changed.notify_all();
        })
        .map_err(|e| e.to_string())?;
    let until = Instant::now() + WAIT;
    let mut data = shared.lock();
    while data.bytes.len() < READY && !data.done {
        if cancel.is_set() || Instant::now() > until {
            cancel.cancel();
            return Err("the stream did not start".to_owned());
        }
        data = shared.changed.wait_timeout(data, Duration::from_millis(100)).unwrap_or_else(PoisonError::into_inner).0;
    }
    if data.bytes.is_empty() {
        return Err("the stream is empty".to_owned());
    }
    drop(data);
    let started = Arc::new(AtomicBool::new(false));
    let reader = Reader { shared, cancel, pos: 0, len, live, started: started.clone(), deadline: until };
    Ok(Stream { reader, len, mime, title, started })
}

/// What went wrong, without the address, which may hold a token.
fn reason(e: ureq::Error) -> String {
    match e {
        ureq::Error::StatusCode(code) => format!("the server answered {code}"),
        ureq::Error::HostNotFound => "the server was not found".to_owned(),
        ureq::Error::Timeout(_) => "no answer in time".to_owned(),
        ureq::Error::Io(e) => e.to_string(),
        _ => "the connection failed".to_owned(),
    }
}

fn download(mut body: impl Read, shared: &Shared, cancel: &Cancel, live: bool) {
    let mut chunk = vec![0u8; 16 * 1024];
    let max = if live { MAX_LIVE } else { MAX_SONG };
    loop {
        if cancel.is_set() {
            return;
        }
        let n = match body.read(&mut chunk) {
            Ok(0) => return,
            Ok(n) => n,
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(_) => return,
        };
        let mut data = shared.lock();
        while live && data.bytes.len() + n > max && !cancel.is_set() {
            data =
                shared.changed.wait_timeout(data, Duration::from_millis(200)).unwrap_or_else(PoisonError::into_inner).0;
        }
        if data.bytes.len() + n > max {
            return;
        }
        data.bytes.extend(chunk.iter().take(n));
        drop(data);
        shared.changed.notify_all();
    }
}

impl Read for Reader {
    fn read(&mut self, out: &mut [u8]) -> std::io::Result<usize> {
        if out.is_empty() {
            return Ok(0);
        }
        let until = Instant::now() + WAIT;
        let mut data = self.shared.lock();
        loop {
            let late = !self.started.load(Ordering::Relaxed) && Instant::now() > self.deadline;
            if late || self.cancel.is_set() || self.pos < data.base {
                return Ok(0);
            }
            let start = usize::try_from(self.pos - data.base).unwrap_or(usize::MAX);
            let mut n = 0;
            for (slot, b) in out.iter_mut().zip(data.bytes.iter().skip(start)) {
                *slot = *b;
                n += 1;
            }
            if n > 0 {
                self.pos += n as u64;
                if self.live {
                    // Played bytes are dropped; a little is kept for the decoder's probing.
                    let played = usize::try_from(self.pos - data.base).unwrap_or(0);
                    if played > READY * 4 {
                        let drop_n = (played - READY).min(data.bytes.len());
                        data.bytes.drain(..drop_n);
                        data.base += drop_n as u64;
                    }
                }
                drop(data);
                self.shared.changed.notify_all();
                return Ok(n);
            }
            if data.done || Instant::now() > until {
                return Ok(0);
            }
            data = self
                .shared
                .changed
                .wait_timeout(data, Duration::from_millis(100))
                .unwrap_or_else(PoisonError::into_inner)
                .0;
        }
    }
}

impl Seek for Reader {
    fn seek(&mut self, to: SeekFrom) -> std::io::Result<u64> {
        let bad = || std::io::Error::new(std::io::ErrorKind::Unsupported, "cannot seek there");
        let pos = match to {
            SeekFrom::Start(n) => Some(n),
            SeekFrom::Current(d) => self.pos.checked_add_signed(d),
            SeekFrom::End(d) => {
                let len = match self.len {
                    Some(l) => Some(l),
                    None => {
                        let data = self.shared.lock();
                        data.done.then_some(data.base + data.bytes.len() as u64)
                    }
                };
                len.and_then(|l| l.checked_add_signed(d))
            }
        };
        let pos = pos.ok_or_else(bad)?;
        if pos < self.shared.lock().base {
            return Err(bad());
        }
        self.pos = pos;
        Ok(pos)
    }
}

/// Takes the metadata blocks out of a live stream.
struct Icy<R> {
    inner: R,
    every: usize,
    left: usize,
    title: Arc<Mutex<String>>,
}

impl<R: Read> Icy<R> {
    fn new(inner: R, every: usize, title: Arc<Mutex<String>>) -> Icy<R> {
        Icy { inner, every, left: every, title }
    }
}

impl<R: Read> Read for Icy<R> {
    fn read(&mut self, out: &mut [u8]) -> std::io::Result<usize> {
        if out.is_empty() {
            return Ok(0);
        }
        if self.left == 0 {
            let mut len = [0u8];
            self.inner.read_exact(&mut len)?;
            let mut block = vec![0u8; usize::from(len[0]) * 16];
            self.inner.read_exact(&mut block)?;
            if let Some(t) = stream_title(&block) {
                *self.title.lock().unwrap_or_else(PoisonError::into_inner) = t;
            }
            self.left = self.every;
        }
        let max = out.len().min(self.left);
        let n = self.inner.read(out.get_mut(..max).unwrap_or_default())?;
        self.left -= n;
        Ok(n)
    }
}

fn stream_title(block: &[u8]) -> Option<String> {
    let text = match std::str::from_utf8(block) {
        Ok(s) => s.to_owned(),
        Err(_) => block.iter().map(|b| char::from(*b)).collect(),
    };
    let rest = text.split_once("StreamTitle='")?.1;
    let end = rest.find("';").unwrap_or_else(|| rest.trim_end_matches('\0').len());
    Some(rest.get(..end)?.trim().to_owned())
}

#[cfg(test)]
mod tests {
    use super::Icy;
    use std::io::Read;
    use std::sync::{Arc, Mutex};

    /// The audio comes out whole, the title is taken from the blocks between, and a title with
    /// an apostrophe is not cut at it.
    #[test]
    fn metadata_is_removed_from_the_audio() {
        let title = Arc::new(Mutex::new(String::new()));
        let mut meta = b"StreamTitle='L'alba - Uno';StreamUrl='x';".to_vec();
        meta.resize(48, 0);
        let mut stream = b"abcd".to_vec();
        stream.push(3);
        stream.extend(meta);
        stream.extend(b"efgh\0ij");
        let mut out = Vec::new();
        Icy::new(&stream[..], 4, title.clone()).read_to_end(&mut out).unwrap();
        assert_eq!(out, b"abcdefghij");
        assert_eq!(*title.lock().unwrap(), "L'alba - Uno");
    }

    /// A stream cut inside a block ends with an error instead of hanging or crashing.
    #[test]
    fn a_cut_stream_is_an_error() {
        let mut out = Vec::new();
        assert!(Icy::new(&b"abcd\x05xx"[..], 4, Arc::default()).read_to_end(&mut out).is_err());
    }
}
