//! What the sound device's errors mean for playback, and how much of them reaches the log.
//!
//! A broken output stream can report the same error thousands of times a second and never
//! recover. Its errors are counted here: the first is logged, then one line now and then says
//! how many followed, and a stream that keeps failing is declared broken, so that the player
//! lets the device go and opens it again at the next play.

use std::io::Write;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// How an error of the stream bears on playback.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Weight {
    /// A glitch the stream gets over by itself, such as a buffer underrun.
    Passing,
    /// An error that may or may not last.
    Doubtful,
    /// The device or the stream is gone: nothing more will play through it.
    Fatal,
}

impl Weight {
    pub(crate) fn of(kind: rodio::CpalErrorKind) -> Weight {
        use rodio::CpalErrorKind as K;
        match kind {
            K::Xrun | K::DeviceChanged | K::RealtimeDenied => Weight::Passing,
            K::DeviceNotAvailable | K::HostUnavailable | K::StreamInvalidated => Weight::Fatal,
            _ => Weight::Doubtful,
        }
    }
}

/// Errors in this span of time…
const WINDOW: Duration = Duration::from_secs(1);
/// …past this many make the stream broken. Underruns come a few at a time, not by the hundred.
const BROKEN_AFTER: u32 = 100;
/// The least time between two lines in the log about the same run of errors.
const LOG_EVERY: Duration = Duration::from_secs(10);

/// The count of a stream's errors, with the time passed in.
#[derive(Debug, Default)]
pub(crate) struct Count {
    window_start: Option<Instant>,
    in_window: u32,
    last_log: Option<Instant>,
    unlogged: u64,
    broken: bool,
}

impl Count {
    /// Counts an error; returns the line to log, if one is due.
    pub(crate) fn record(&mut self, weight: Weight, text: &str, now: Instant) -> Option<String> {
        match weight {
            Weight::Fatal => self.broken = true,
            // Only doubtful errors count towards a stream that keeps failing: underruns never do.
            Weight::Doubtful => {
                match self.window_start {
                    Some(start) if now.saturating_duration_since(start) < WINDOW => {
                        self.in_window = self.in_window.saturating_add(1);
                    }
                    _ => {
                        self.window_start = Some(now);
                        self.in_window = 1;
                    }
                }
                if self.in_window >= BROKEN_AFTER {
                    self.broken = true;
                }
            }
            Weight::Passing => {}
        }
        let due = self.last_log.is_none_or(|last| now.saturating_duration_since(last) >= LOG_EVERY);
        if !due {
            self.unlogged = self.unlogged.saturating_add(1);
            return None;
        }
        self.last_log = Some(now);
        let line = match std::mem::take(&mut self.unlogged) {
            0 => format!("audio stream error: {text}"),
            n => format!("audio stream error: {text} (and {n} more since the last report)"),
        };
        Some(line)
    }

    pub(crate) fn broken(&self) -> bool {
        self.broken
    }
}

/// The count shared between the stream's error callback and the audio thread.
#[derive(Debug, Clone, Default)]
pub(crate) struct Health(Arc<Mutex<Count>>);

impl Health {
    /// The callback the output stream reports its errors to.
    pub(crate) fn callback(&self) -> impl FnMut(rodio::CpalError) + Send + Clone + 'static {
        let health = self.clone();
        move |err: rodio::CpalError| {
            let line = health.count().record(Weight::of(err.kind()), &err.to_string(), Instant::now());
            if let Some(line) = line {
                // A closed stderr is no reason to stop: unlike `eprintln!`, this never panics.
                writeln!(std::io::stderr(), "{line}").ok();
            }
        }
    }

    pub(crate) fn broken(&self) -> bool {
        self.count().broken()
    }

    fn count(&self) -> std::sync::MutexGuard<'_, Count> {
        self.0.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

#[cfg(test)]
mod tests {
    use super::{BROKEN_AFTER, Count, LOG_EVERY, Weight};
    use std::time::{Duration, Instant};

    #[test]
    fn a_flood_of_errors_logs_a_few_lines_and_breaks_the_stream() {
        let mut count = Count::default();
        let start = Instant::now();
        let mut lines = Vec::new();
        // A million errors over a minute, as a dead ALSA stream sends them.
        for i in 0..1_000_000u64 {
            let now = start + Duration::from_micros(i * 60);
            lines.extend(count.record(Weight::Doubtful, "I/O error", now));
        }
        assert!(count.broken());
        assert!(lines.len() <= 8, "{} lines", lines.len());
        assert!(lines.iter().skip(1).all(|l| l.contains("more since the last report")));
    }

    #[test]
    fn underruns_now_and_then_leave_the_stream_alone() {
        let mut count = Count::default();
        let start = Instant::now();
        // Bursts of underruns, the most a minute, for an hour.
        for minute in 0..60u64 {
            for i in 0..u64::from(BROKEN_AFTER) * 2 {
                let now = start + Duration::from_secs(minute * 60) + Duration::from_millis(i);
                count.record(Weight::Passing, "underrun", now);
            }
        }
        assert!(!count.broken());
        // A doubtful error now and then does not break it either.
        for s in 0..120u64 {
            count.record(Weight::Doubtful, "busy", start + Duration::from_secs(3600 + s * 5));
        }
        assert!(!count.broken());
    }

    #[test]
    fn underruns_do_not_push_a_single_doubtful_error_over_the_limit() {
        let mut count = Count::default();
        let start = Instant::now();
        for i in 0..u64::from(BROKEN_AFTER) {
            count.record(Weight::Passing, "underrun", start + Duration::from_micros(i));
        }
        count.record(Weight::Doubtful, "busy", start + Duration::from_millis(1));
        assert!(!count.broken());
    }

    #[test]
    fn a_lost_device_breaks_the_stream_at_once() {
        let mut count = Count::default();
        let line = count.record(Weight::Fatal, "device gone", Instant::now());
        assert!(count.broken());
        assert!(line.is_some_and(|l| l.contains("device gone")));
    }

    #[test]
    fn a_quiet_spell_lets_the_next_error_be_logged() {
        let mut count = Count::default();
        let start = Instant::now();
        assert!(count.record(Weight::Passing, "underrun", start).is_some());
        assert!(count.record(Weight::Passing, "underrun", start + Duration::from_secs(1)).is_none());
        let later = count.record(Weight::Passing, "underrun", start + LOG_EVERY + Duration::from_secs(1));
        assert!(later.is_some_and(|l| l.contains("1 more")));
    }
}
