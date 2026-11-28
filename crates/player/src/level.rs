//! Even volume: each song is played louder or softer by the factor its tags give, while the
//! switch shared with the player is on. The switch is read at every sample, so turning it on or
//! off reaches the song that is playing and the one prepared to follow it at once.

use rodio::source::SeekError;
use rodio::{ChannelCount, Sample, SampleRate, Source};
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

/// How a song's factor is found: from its file, on the thread that opens it, so that a slow disk
/// never holds up the window.
pub type Reader = fn(&Path) -> Option<f32>;

/// The largest factor the player applies; anything else becomes 1.
const MOST: f32 = 4.0;

/// A factor the player can use: finite, above 0 and at most `MOST`, else 1.
pub(crate) fn clean(gain: f32) -> f32 {
    if gain.is_finite() && gain > 0.0 && gain <= MOST { gain } else { 1.0 }
}

/// A song with its own factor, applied while `on` is set.
pub(crate) struct Leveled<I> {
    input: I,
    gain: Sample,
    on: Arc<AtomicBool>,
}

impl<I> Leveled<I> {
    pub(crate) fn new(input: I, gain: f32, on: Arc<AtomicBool>) -> Leveled<I> {
        Leveled { input, gain: clean(gain), on }
    }
}

impl<I: Source> Iterator for Leveled<I> {
    type Item = Sample;

    fn next(&mut self) -> Option<Sample> {
        let v = self.input.next()?;
        Some(if self.on.load(Ordering::Relaxed) { v * self.gain } else { v })
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        self.input.size_hint()
    }
}

impl<I: Source> Source for Leveled<I> {
    fn current_span_len(&self) -> Option<usize> {
        self.input.current_span_len()
    }

    fn channels(&self) -> ChannelCount {
        self.input.channels()
    }

    fn sample_rate(&self) -> SampleRate {
        self.input.sample_rate()
    }

    fn total_duration(&self) -> Option<Duration> {
        self.input.total_duration()
    }

    fn try_seek(&mut self, pos: Duration) -> Result<(), SeekError> {
        self.input.try_seek(pos)
    }
}

#[cfg(test)]
mod tests {
    use super::{Leveled, clean};
    use rodio::buffer::SamplesBuffer;
    use std::num::NonZero;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};

    /// The switch reaches a song already opened: on, its samples follow its factor; off, they are
    /// as they came.
    #[test]
    fn the_switch_reaches_a_song_already_opened() {
        let on = Arc::new(AtomicBool::new(false));
        let one = NonZero::new(1u16).unwrap();
        let rate = NonZero::new(8000u32).unwrap();
        let mut song = Leveled::new(SamplesBuffer::new(one, rate, vec![0.5f32; 4]), 0.5, on.clone());
        let before = song.next().unwrap();
        on.store(true, Ordering::Relaxed);
        let during = song.next().unwrap();
        on.store(false, Ordering::Relaxed);
        let after = song.next().unwrap();
        assert!(during < before);
        assert!((after - before).abs() < 1e-6);
    }

    #[test]
    fn a_factor_out_of_range_leaves_the_song_alone() {
        for bad in [0.0, -1.0, 5.0, f32::NAN, f32::INFINITY] {
            assert!((clean(bad) - 1.0).abs() < 1e-6);
        }
        assert!(clean(2.0) > 1.0 && clean(0.5) < 1.0);
    }
}
