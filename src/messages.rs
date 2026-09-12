//! Messages sent from background worker threads back to the UI thread.
//!
//! This replaces the old C# `ConcurrentQueue<Action>` "main thread action"
//! pattern: workers never touch UI state directly, they just enqueue a
//! `ClipperMsg`, which `VideoClipper::drain_messages` applies once per frame.

use std::path::PathBuf;
use std::sync::Arc;

use crate::ffmpeg_util::FfmpegCaps;

/// Final result of an export run, shared by the GUI (status text) and the
/// CLI (exit code).
pub enum ExportOutcome {
    /// Saved to the given path.
    Success(PathBuf),
    /// Canceled by the user (Cancel in the GUI, Ctrl+C in the CLI).
    Canceled,
    /// Failed with an already user-facing message.
    Failed(String),
}

impl ExportOutcome {
    pub fn message(&self) -> String {
        match self {
            ExportOutcome::Success(path) => {
                format!("Export finished! Saved to {}", path.display())
            }
            ExportOutcome::Canceled => "Export canceled.".to_string(),
            ExportOutcome::Failed(msg) => msg.clone(),
        }
    }
}

pub enum ClipperMsg {
    /// A single decoded frame is ready to be shown in the preview.
    FrameReady {
        frame_index: i64,
        rgba: Vec<u8>,
        width: u32,
        height: u32,
    },
    /// One of the timeline thumbnails finished decoding.
    ThumbnailReady {
        index: usize,
        rgba: Vec<u8>,
        width: u32,
        height: u32,
        done: usize,
        total: usize,
    },
    /// The audio track finished extracting/decoding and is ready to play.
    AudioReady {
        samples: Arc<Vec<f32>>,
        channels: u16,
        sample_rate: u32,
        waveform: Vec<f32>,
    },
    /// The video has no usable audio track (or extraction failed) - treated
    /// as silent, exactly like the old code's try/catch around the audio
    /// reader.
    AudioUnavailable(String),
    /// Progress update while exporting.
    ExportProgress {
        fraction: Option<f32>,
        message: String,
    },
    /// Export finished (successfully, with an error, or cancelled).
    ExportFinished { outcome: ExportOutcome },
    /// A generic status message + optional progress bar update (used for
    /// thumbnail loading, etc. outside of the dedicated variants above).
    Status {
        message: String,
        progress: Option<f32>,
    },
    /// The startup ffmpeg capability probe finished.
    FfmpegCaps(FfmpegCaps),
}
