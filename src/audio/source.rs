//! A `rodio::Source` over an in-memory interleaved PCM buffer with an
//! externally-controlled, instantly-seekable playback cursor.
//!
//! The cursor (`position`) is expressed in *sample frames* (i.e. one unit
//! per sample across all channels, not per raw interleaved sample). It's
//! shared via an `Arc<AtomicUsize>` so the rest of the app can both read
//! "where is playback right now" (to drive the video preview) and write
//! "jump to here" (to seek) without going through rodio at all.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use rodio::Source;

pub struct PcmSource {
    samples: Arc<Vec<f32>>,
    channels: u16,
    sample_rate: u32,
    /// Position in the interleaved `samples` buffer (i.e. `frame * channels`).
    position: Arc<AtomicUsize>,
    frame_cursor: usize,
}

impl PcmSource {
    /// `position` holds the current playback position in *sample frames*.
    pub fn new(
        samples: Arc<Vec<f32>>,
        channels: u16,
        sample_rate: u32,
        position: Arc<AtomicUsize>,
    ) -> Self {
        Self {
            samples,
            channels,
            sample_rate,
            position,
            frame_cursor: 0,
        }
    }
}

impl Iterator for PcmSource {
    type Item = f32;

    fn next(&mut self) -> Option<f32> {
        let channels = self.channels as usize;
        let frame = self.position.load(Ordering::Relaxed);
        let index = frame * channels + self.frame_cursor;

        let sample = self.samples.get(index).copied().unwrap_or(0.0);

        self.frame_cursor += 1;
        if self.frame_cursor >= channels {
            self.frame_cursor = 0;
            self.position.fetch_add(1, Ordering::Relaxed);
        }

        // Never-ending stream of silence past the end of the buffer; the app
        // is responsible for pausing playback once it reaches the trim end.
        Some(sample)
    }
}

impl Source for PcmSource {
    fn current_frame_len(&self) -> Option<usize> {
        None
    }

    fn channels(&self) -> u16 {
        self.channels
    }

    fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    fn total_duration(&self) -> Option<Duration> {
        None
    }
}
