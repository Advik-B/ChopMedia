//! Audio extraction (via `ffmpeg` -> temp WAV -> in-memory PCM), waveform
//! bucketing, and playback control.

mod source;

use std::path::PathBuf;
use std::process::Stdio;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::Sender;
use std::thread;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::Context;
use rodio::{OutputStream, OutputStreamHandle, Sink};

use crate::ffmpeg_util;
use crate::messages::ClipperMsg;
use source::PcmSource;

/// Kicks off audio extraction + waveform computation on a background
/// thread. Sends `ClipperMsg::AudioReady` on success or
/// `ClipperMsg::AudioUnavailable` if the video has no usable audio track.
pub fn extract_async(path: PathBuf, waveform_buckets: usize, sender: Sender<ClipperMsg>) {
    thread::spawn(move || match extract(&path, waveform_buckets) {
        Ok((samples, channels, sample_rate, waveform)) => {
            let _ = sender.send(ClipperMsg::AudioReady {
                samples: Arc::new(samples),
                channels,
                sample_rate,
                waveform,
            });
        }
        Err(err) => {
            let _ = sender.send(ClipperMsg::AudioUnavailable(err.to_string()));
        }
    });
}

fn temp_wav_path() -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    std::env::temp_dir().join(format!("chopmedia_audio_{nanos}.wav"))
}

fn extract(
    path: &std::path::Path,
    waveform_buckets: usize,
) -> anyhow::Result<(Vec<f32>, u16, u32, Vec<f32>)> {
    let temp_path = temp_wav_path();

    let status = ffmpeg_util::command("ffmpeg")
        .args(["-y", "-v", "error", "-i"])
        .arg(path)
        .args([
            "-vn",
            "-ac",
            "2",
            "-ar",
            "44100",
            "-c:a",
            "pcm_s16le",
            "-f",
            "wav",
        ])
        .arg(&temp_path)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .output()
        .context("failed to spawn ffmpeg for audio extraction")?;

    if !status.status.success() {
        let _ = std::fs::remove_file(&temp_path);
        anyhow::bail!(
            "no audio track (ffmpeg: {})",
            String::from_utf8_lossy(&status.stderr).trim()
        );
    }

    let result = (|| {
        let mut reader = hound::WavReader::open(&temp_path)?;
        let spec = reader.spec();
        let samples: Vec<f32> = reader
            .samples::<i16>()
            .map(|s| s.unwrap_or(0) as f32 / i16::MAX as f32)
            .collect();
        anyhow::Ok((samples, spec.channels, spec.sample_rate))
    })();

    let _ = std::fs::remove_file(&temp_path);
    let (samples, channels, sample_rate) = result.context("failed to read extracted audio")?;

    if samples.is_empty() {
        anyhow::bail!("extracted audio track was empty");
    }

    let waveform = compute_waveform(&samples, channels, waveform_buckets);
    Ok((samples, channels, sample_rate, waveform))
}

/// Mono amplitude envelope, bucketed into `bucket_count` values in `[0, 1]`.
fn compute_waveform(samples: &[f32], channels: u16, bucket_count: usize) -> Vec<f32> {
    let channels = channels.max(1) as usize;
    let total_frames = samples.len() / channels;
    if total_frames == 0 || bucket_count == 0 {
        return Vec::new();
    }

    let frames_per_bucket = total_frames as f64 / bucket_count as f64;
    let mut waveform = Vec::with_capacity(bucket_count);

    for i in 0..bucket_count {
        let start_frame = (i as f64 * frames_per_bucket) as usize;
        let end_frame = (((i + 1) as f64) * frames_per_bucket).ceil() as usize;
        let end_frame = end_frame.clamp(start_frame + 1, total_frames);

        let mut sum = 0f32;
        let mut count = 0usize;
        for f in start_frame..end_frame {
            sum += samples[f * channels].abs();
            count += 1;
        }
        waveform.push(if count > 0 { sum / count as f32 } else { 0.0 });
    }

    waveform
}

/// Owns the audio output device + sink for the currently loaded video, and
/// exposes an instantly-seekable playback cursor.
pub struct AudioPlayback {
    _stream: OutputStream,
    _stream_handle: OutputStreamHandle,
    sink: Sink,
    position: Arc<AtomicUsize>,
    #[allow(dead_code)]
    channels: u16,
    sample_rate: u32,
    total_frames: usize,
}

impl AudioPlayback {
    pub fn new(
        samples: Arc<Vec<f32>>,
        channels: u16,
        sample_rate: u32,
        volume: f32,
    ) -> anyhow::Result<Self> {
        let (stream, stream_handle) =
            OutputStream::try_default().context("no audio output device")?;
        let sink = Sink::try_new(&stream_handle).context("failed to create audio sink")?;
        sink.set_volume(volume);

        let total_frames = samples.len() / channels.max(1) as usize;
        let position = Arc::new(AtomicUsize::new(0));
        let source = PcmSource::new(samples, channels, sample_rate, position.clone());
        sink.append(source);
        sink.pause();

        Ok(Self {
            _stream: stream,
            _stream_handle: stream_handle,
            sink,
            position,
            channels,
            sample_rate,
            total_frames,
        })
    }

    pub fn play(&self) {
        self.sink.play();
    }

    pub fn pause(&self) {
        self.sink.pause();
    }

    pub fn set_volume(&self, volume: f32) {
        self.sink.set_volume(volume);
    }

    /// Seeks the playback cursor to the given position in seconds (clamped
    /// to the buffer's length).
    pub fn seek_to_seconds(&self, seconds: f64) {
        let frame = (seconds.max(0.0) * self.sample_rate as f64).round() as usize;
        self.position
            .store(frame.min(self.total_frames), Ordering::Relaxed);
    }

    /// Current playback position, in seconds. Exposed for callers that
    /// want to display an audio-driven clock (e.g. a future time readout).
    #[allow(dead_code)]
    pub fn current_seconds(&self) -> f64 {
        self.position.load(Ordering::Relaxed) as f64 / self.sample_rate as f64
    }

    #[allow(dead_code)]
    pub fn channels(&self) -> u16 {
        self.channels
    }

    pub fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    /// Returns a clone of the shared playback-position handle (in sample
    /// frames), suitable for driving a [`crate::video::PlaybackClock`].
    pub fn position_handle(&self) -> Arc<AtomicUsize> {
        self.position.clone()
    }
}
