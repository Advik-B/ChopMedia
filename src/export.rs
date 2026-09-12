//! Clip export via `ffmpeg`, ported from the old `ExportClip`/`RunExport`
//! and extended with configurable containers, codecs, quality, audio,
//! scaling, and frame rate.

use std::io::{BufRead, BufReader};
use std::path::PathBuf;
use std::process::{Child, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use crate::ffmpeg_util::{self, FfmpegCaps};
use crate::messages::{ClipperMsg, ExportOutcome};

#[derive(Clone)]
pub struct CropPixels {
    pub x: i64,
    pub y: i64,
    pub w: i64,
    pub h: i64,
}

/// Output container format.
#[derive(Clone, Copy, PartialEq)]
pub enum Container {
    Mp4,
    Mkv,
    Mov,
    WebM,
}

impl Container {
    pub const ALL: [Container; 4] = [
        Container::Mp4,
        Container::Mkv,
        Container::Mov,
        Container::WebM,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Container::Mp4 => "MP4",
            Container::Mkv => "MKV",
            Container::Mov => "MOV",
            Container::WebM => "WebM",
        }
    }

    pub fn extension(self) -> &'static str {
        match self {
            Container::Mp4 => "mp4",
            Container::Mkv => "mkv",
            Container::Mov => "mov",
            Container::WebM => "webm",
        }
    }

    /// The name of the ffmpeg muxer backing this container.
    pub fn muxer_name(self) -> &'static str {
        match self {
            Container::Mp4 => "mp4",
            Container::Mkv => "matroska",
            Container::Mov => "mov",
            Container::WebM => "webm",
        }
    }
}

/// Stream copy (fast, keyframe-bound) vs. full re-encode (frame-accurate).
#[derive(Clone, Copy, PartialEq)]
pub enum ExportMode {
    StreamCopy,
    Reencode,
}

/// Video encoder used in re-encode mode.
#[derive(Clone, Copy, PartialEq)]
pub enum VideoCodec {
    H264,
    H265,
    Vp9,
    Av1,
}

impl VideoCodec {
    pub const ALL: [VideoCodec; 4] = [
        VideoCodec::H264,
        VideoCodec::H265,
        VideoCodec::Vp9,
        VideoCodec::Av1,
    ];

    pub fn label(self) -> &'static str {
        match self {
            VideoCodec::H264 => "H.264",
            VideoCodec::H265 => "H.265 (HEVC)",
            VideoCodec::Vp9 => "VP9",
            VideoCodec::Av1 => "AV1",
        }
    }

    pub fn encoder(self) -> &'static str {
        match self {
            VideoCodec::H264 => "libx264",
            VideoCodec::H265 => "libx265",
            VideoCodec::Vp9 => "libvpx-vp9",
            VideoCodec::Av1 => "libsvtav1",
        }
    }

    pub fn available_in(self, container: Container) -> bool {
        match container {
            Container::Mp4 => matches!(self, VideoCodec::H264 | VideoCodec::H265 | VideoCodec::Av1),
            Container::Mkv => true,
            Container::Mov => matches!(self, VideoCodec::H264 | VideoCodec::H265),
            Container::WebM => matches!(self, VideoCodec::Vp9 | VideoCodec::Av1),
        }
    }

    /// Container-compatible *and* present in the probed ffmpeg build.
    pub fn is_usable(self, container: Container, caps: &FfmpegCaps) -> bool {
        self.available_in(container) && caps.has_encoder(self.encoder())
    }

    pub fn default_for(container: Container, caps: &FfmpegCaps) -> VideoCodec {
        let preferred = match container {
            Container::WebM => VideoCodec::Vp9,
            _ => VideoCodec::H264,
        };
        if preferred.is_usable(container, caps) {
            return preferred;
        }
        VideoCodec::ALL
            .iter()
            .copied()
            .find(|c| c.is_usable(container, caps))
            .unwrap_or(preferred)
    }

    /// Valid CRF range for this encoder.
    pub fn crf_range(self) -> std::ops::RangeInclusive<u8> {
        match self {
            VideoCodec::H264 | VideoCodec::H265 => 0..=51,
            VideoCodec::Vp9 | VideoCodec::Av1 => 0..=63,
        }
    }
}

/// Encoder speed setting, validated by the export dialog against the
/// probed encoder options.
#[derive(Clone, PartialEq)]
pub enum EncoderSpeed {
    /// Named `-preset` choice (x264/x265), e.g. "medium".
    Named(String),
    /// Numeric `-preset` (SVT-AV1) or `-cpu-used` (VP9) value.
    Numeric(i64),
}

/// Audio handling for the exported clip.
#[derive(Clone, Copy, PartialEq)]
pub enum AudioMode {
    Copy,
    Aac(u32),
    Opus(u32),
    Remove,
}

impl AudioMode {
    /// Label/option pairs valid for the given container and ffmpeg build
    /// (WebM can't hold AAC so it gets Opus instead; codecs missing from
    /// the probed build are omitted).
    pub fn presets_for(container: Container, caps: &FfmpegCaps) -> Vec<(&'static str, AudioMode)> {
        let mut presets = vec![("Copy", AudioMode::Copy)];
        match container {
            Container::WebM => {
                if caps.has_encoder("libopus") {
                    presets.push(("Opus 96k", AudioMode::Opus(96)));
                    presets.push(("Opus 160k", AudioMode::Opus(160)));
                }
            }
            _ => {
                if caps.has_encoder("aac") {
                    presets.push(("AAC 128k", AudioMode::Aac(128)));
                    presets.push(("AAC 192k", AudioMode::Aac(192)));
                    presets.push(("AAC 256k", AudioMode::Aac(256)));
                }
            }
        }
        presets.push(("No audio", AudioMode::Remove));
        presets
    }

    pub fn label(self) -> String {
        match self {
            AudioMode::Copy => "Copy".to_string(),
            AudioMode::Aac(k) => format!("AAC {k}k"),
            AudioMode::Opus(k) => format!("Opus {k}k"),
            AudioMode::Remove => "No audio".to_string(),
        }
    }
}

/// Everything the export modal lets the user tweak.
#[derive(Clone)]
pub struct ExportOptions {
    pub container: Container,
    pub mode: ExportMode,
    pub video_codec: VideoCodec,
    /// Quality factor, clamped by the dialog into the encoder's probed CRF
    /// range (or a static fallback). Lower is better.
    pub crf: u8,
    pub speed: EncoderSpeed,
    pub audio: AudioMode,
    /// Output scale factor (1.0 = original).
    pub scale: f32,
    /// Output frame rate override (`None` = original).
    pub fps: Option<f64>,
}

impl Default for ExportOptions {
    fn default() -> Self {
        Self {
            container: Container::Mp4,
            mode: ExportMode::StreamCopy,
            video_codec: VideoCodec::H264,
            crf: 20,
            speed: EncoderSpeed::Named("medium".to_string()),
            audio: AudioMode::Copy,
            scale: 1.0,
            fps: None,
        }
    }
}

impl ExportOptions {
    /// Cropping, scaling, or changing the frame rate always requires a
    /// re-encode, regardless of the selected mode.
    pub fn needs_reencode(&self, has_crop: bool) -> bool {
        matches!(self.mode, ExportMode::Reencode)
            || has_crop
            || self.scale != 1.0
            || self.fps.is_some()
    }

    fn video_args(&self) -> Vec<String> {
        let mut args = vec![
            "-c:v".to_string(),
            self.video_codec.encoder().to_string(),
            "-crf".to_string(),
            self.crf.to_string(),
        ];
        if matches!(self.video_codec, VideoCodec::Vp9) {
            // VP9 needs an explicit bitrate cap of 0 for CRF-only mode.
            args.push("-b:v".to_string());
            args.push("0".to_string());
        }
        match (&self.speed, self.video_codec) {
            (EncoderSpeed::Named(preset), VideoCodec::H264 | VideoCodec::H265) => {
                args.push("-preset".to_string());
                args.push(preset.clone());
            }
            (EncoderSpeed::Numeric(n), VideoCodec::Av1) => {
                args.push("-preset".to_string());
                args.push(n.to_string());
            }
            (EncoderSpeed::Numeric(n), VideoCodec::Vp9) => {
                args.push("-cpu-used".to_string());
                args.push(n.to_string());
                args.push("-row-mt".to_string());
                args.push("1".to_string());
            }
            // Mismatched speed kind (shouldn't happen; the dialog validates).
            _ => {}
        }
        args
    }

    fn audio_args(&self) -> Vec<String> {
        match self.audio {
            AudioMode::Copy => vec!["-c:a".to_string(), "copy".to_string()],
            AudioMode::Aac(k) => vec![
                "-c:a".to_string(),
                "aac".to_string(),
                "-b:a".to_string(),
                format!("{k}k"),
            ],
            AudioMode::Opus(k) => vec![
                "-c:a".to_string(),
                "libopus".to_string(),
                "-b:a".to_string(),
                format!("{k}k"),
            ],
            AudioMode::Remove => vec!["-an".to_string()],
        }
    }
}

pub struct ExportParams {
    pub video_path: PathBuf,
    pub output_path: PathBuf,
    pub start_secs: f64,
    pub end_secs: f64,
    pub crop: Option<CropPixels>,
    pub options: ExportOptions,
}

fn kill_child(child: &Arc<Mutex<Option<Child>>>) {
    let child = child.lock().unwrap().take();
    if let Some(mut child) = child {
        let _ = child.kill();
        let _ = child.wait();
    }
}

#[derive(Default)]
struct FfmpegProgressRecord {
    out_time_us: Option<i64>,
    out_time_secs: Option<f64>,
}

#[derive(Debug, PartialEq)]
enum FfmpegProgressEvent {
    Continue(Option<f32>),
    End,
}

impl FfmpegProgressRecord {
    fn consume_line(&mut self, line: &str, duration_total: f64) -> Option<FfmpegProgressEvent> {
        let (key, value) = line.split_once('=')?;
        let key = key.trim();
        let value = value.trim();

        match key {
            "out_time_us" => self.out_time_us = value.parse().ok(),
            "out_time" => self.out_time_secs = parse_out_time(value),
            "progress" => {
                let event = match value {
                    "continue" => {
                        Some(FfmpegProgressEvent::Continue(self.fraction(duration_total)))
                    }
                    "end" => Some(FfmpegProgressEvent::End),
                    _ => None,
                };
                self.reset();
                return event;
            }
            _ => {}
        }

        None
    }

    fn fraction(&self, duration_total: f64) -> Option<f32> {
        if !duration_total.is_finite() || duration_total <= 0.0 {
            return None;
        }

        let elapsed = self
            .out_time_us
            .map(|out_time_us| out_time_us as f64 / 1_000_000.0)
            .or(self.out_time_secs)?;
        if !elapsed.is_finite() {
            return None;
        }

        Some((elapsed / duration_total).clamp(0.0, 1.0) as f32)
    }

    fn reset(&mut self) {
        self.out_time_us = None;
        self.out_time_secs = None;
    }
}

fn parse_out_time(value: &str) -> Option<f64> {
    let value = value.trim();
    let (sign, value) = if let Some(value) = value.strip_prefix('-') {
        (-1.0, value)
    } else if let Some(value) = value.strip_prefix('+') {
        (1.0, value)
    } else {
        (1.0, value)
    };

    let mut components = value.split(':');
    let hours = components.next()?.parse::<f64>().ok()?;
    let minutes = components.next()?.parse::<f64>().ok()?;
    let seconds = components.next()?.parse::<f64>().ok()?;
    if components.next().is_some()
        || !hours.is_finite()
        || !minutes.is_finite()
        || !seconds.is_finite()
        || hours < 0.0
        || !(0.0..60.0).contains(&minutes)
        || !(0.0..60.0).contains(&seconds)
    {
        return None;
    }

    let total = hours * 3_600.0 + minutes * 60.0 + seconds;
    total.is_finite().then_some(sign * total)
}

fn wait_for_child(
    child_handle: &Arc<Mutex<Option<Child>>>,
    cancel: &Arc<AtomicBool>,
) -> anyhow::Result<Option<std::process::ExitStatus>> {
    loop {
        if cancel.load(Ordering::Relaxed) {
            kill_child(child_handle);
            return Ok(None);
        }

        let status = {
            let mut child = child_handle.lock().unwrap();
            match child.as_mut() {
                Some(child) => child.try_wait()?,
                None => return Ok(None),
            }
        };

        if let Some(status) = status {
            let _ = child_handle.lock().unwrap().take();
            return Ok(Some(status));
        }

        thread::sleep(Duration::from_millis(10));
    }
}

/// Handle to a running export; drop or call `cancel()` to abort it.
pub struct ExportHandle {
    cancel: Arc<AtomicBool>,
    child: Arc<Mutex<Option<Child>>>,
}

impl ExportHandle {
    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::Relaxed);
        kill_child(&self.child);
    }
}

/// Starts an export on a background thread. Progress/completion are
/// reported through `sender` as `ClipperMsg::ExportProgress` /
/// `ClipperMsg::ExportFinished`.
pub fn start_export(params: ExportParams, sender: Sender<ClipperMsg>) -> ExportHandle {
    let cancel = Arc::new(AtomicBool::new(false));
    let child_handle: Arc<Mutex<Option<Child>>> = Arc::new(Mutex::new(None));

    let cancel_thread = cancel.clone();
    let child_thread = child_handle.clone();

    thread::spawn(move || {
        let result = run_export(&params, &cancel_thread, &child_thread, &sender);
        let outcome = match result {
            Ok(()) if cancel_thread.load(Ordering::Relaxed) => ExportOutcome::Canceled,
            Ok(()) => ExportOutcome::Success(params.output_path.clone()),
            Err(err) => ExportOutcome::Failed(format!("Export failed: {err}")),
        };
        let _ = sender.send(ClipperMsg::ExportFinished { outcome });
    });

    ExportHandle {
        cancel,
        child: child_handle,
    }
}

fn run_export(
    params: &ExportParams,
    cancel: &Arc<AtomicBool>,
    child_handle: &Arc<Mutex<Option<Child>>>,
    sender: &Sender<ClipperMsg>,
) -> anyhow::Result<()> {
    if params.end_secs <= params.start_secs {
        anyhow::bail!("invalid start/end time");
    }
    let duration_total = params.end_secs - params.start_secs;
    let reencode = params.options.needs_reencode(params.crop.is_some());

    let mut cmd = ffmpeg_util::command("ffmpeg");
    cmd.arg("-y");
    cmd.args(["-progress", "pipe:1", "-nostats"]);

    if reencode {
        cmd.arg("-i").arg(&params.video_path);
        cmd.args(["-ss", &format!("{:.3}", params.start_secs)]);
    } else {
        cmd.args(["-ss", &format!("{:.3}", params.start_secs)]);
        cmd.arg("-i").arg(&params.video_path);
    }

    // Cut by duration. `-to` is wrong here in both branches: with the
    // input-side `-ss` used for stream copy, ffmpeg resets timestamps so
    // `-to end` would count from the seek point and overrun the clip.
    cmd.args(["-t", &format!("{duration_total:.3}")]);

    // Filter chain: crop, then scale (rounding to even dimensions, which
    // yuv420p encoders require).
    let mut filters: Vec<String> = Vec::new();
    if let Some(crop) = &params.crop {
        filters.push(format!("crop={}:{}:{}:{}", crop.w, crop.h, crop.x, crop.y));
    }
    if params.options.scale != 1.0 {
        let s = params.options.scale;
        filters.push(format!("scale=trunc(iw*{s}/2)*2:trunc(ih*{s}/2)*2"));
    } else if !filters.is_empty() {
        filters.push("scale=trunc(iw/2)*2:trunc(ih/2)*2".to_string());
    }
    if !filters.is_empty() {
        cmd.args(["-vf", &filters.join(",")]);
    }

    if reencode {
        for arg in params.options.video_args() {
            cmd.arg(arg);
        }
        if let Some(fps) = params.options.fps {
            cmd.args(["-r", &format!("{fps}")]);
        }
        for arg in params.options.audio_args() {
            cmd.arg(arg);
        }
    } else {
        cmd.args(["-c", "copy"]);
        if matches!(params.options.audio, AudioMode::Remove) {
            cmd.arg("-an");
        }
    }

    cmd.arg(&params.output_path);
    cmd.stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    let mut child = cmd.spawn()?;
    let stdout = child.stdout.take();
    let stderr = child.stderr.take();
    *child_handle.lock().unwrap() = Some(child);

    let stderr_thread = stderr.map(|stderr| {
        let sender = sender.clone();
        thread::spawn(move || {
            for line in BufReader::new(stderr).lines() {
                let Ok(line) = line else { break };
                let _ = sender.send(ClipperMsg::ExportProgress {
                    fraction: None,
                    message: line,
                });
            }
        })
    });

    let _ = sender.send(ClipperMsg::ExportProgress {
        fraction: Some(0.0),
        message: "Starting export...".to_string(),
    });

    let mut canceled = cancel.load(Ordering::Relaxed);
    if canceled {
        kill_child(child_handle);
    } else if let Some(stdout) = stdout {
        let mut progress = FfmpegProgressRecord::default();
        for line in BufReader::new(stdout).lines() {
            if cancel.load(Ordering::Relaxed) {
                canceled = true;
                kill_child(child_handle);
                break;
            }
            let Ok(line) = line else { break };

            if let Some(FfmpegProgressEvent::Continue(Some(fraction))) =
                progress.consume_line(&line, duration_total)
            {
                let _ = sender.send(ClipperMsg::ExportProgress {
                    fraction: Some(fraction),
                    message: "Exporting...".to_string(),
                });
            }
        }
    }

    if cancel.load(Ordering::Relaxed) {
        canceled = true;
        kill_child(child_handle);
    }

    let status_result = if canceled {
        Ok(None)
    } else {
        wait_for_child(child_handle, cancel)
    };
    if status_result.is_err() {
        kill_child(child_handle);
    }
    if let Some(stderr_thread) = stderr_thread {
        let _ = stderr_thread.join();
    }
    let status = status_result?;

    if canceled || cancel.load(Ordering::Relaxed) {
        return Ok(());
    }
    let Some(status) = status else {
        return Ok(()); // already killed/cancelled
    };

    if !status.success() {
        anyhow::bail!("ffmpeg exited with status {status}");
    }

    let _ = sender.send(ClipperMsg::ExportProgress {
        fraction: Some(1.0),
        message: "Done.".to_string(),
    });

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn progress_prefers_out_time_us() {
        let mut record = FfmpegProgressRecord::default();
        assert_eq!(record.consume_line("out_time_us=2500000", 10.0), None);
        assert_eq!(record.consume_line("out_time=00:00:08.000000", 10.0), None);
        assert_eq!(
            record.consume_line("progress=continue", 10.0),
            Some(FfmpegProgressEvent::Continue(Some(0.25)))
        );
    }

    #[test]
    fn progress_falls_back_to_out_time() {
        let mut record = FfmpegProgressRecord::default();
        assert_eq!(
            record.consume_line("out_time=01:02:03.456789", 4_000.0),
            None
        );

        let Some(FfmpegProgressEvent::Continue(Some(fraction))) =
            record.consume_line("progress=continue", 4_000.0)
        else {
            panic!("expected a progress fraction");
        };
        assert!((fraction - (3_723.456_789 / 4_000.0) as f32).abs() < f32::EPSILON);
    }

    #[test]
    fn progress_clamps_continue_updates_and_discards_end() {
        let mut record = FfmpegProgressRecord::default();
        assert_eq!(record.consume_line("out_time_us=15000000", 10.0), None);
        assert_eq!(
            record.consume_line("progress=continue", 10.0),
            Some(FfmpegProgressEvent::Continue(Some(1.0)))
        );

        assert_eq!(record.consume_line("out_time_us=10000000", 10.0), None);
        assert_eq!(
            record.consume_line("progress=end", 10.0),
            Some(FfmpegProgressEvent::End)
        );
        assert_eq!(
            record.consume_line("progress=continue", 10.0),
            Some(FfmpegProgressEvent::Continue(None))
        );
    }

    #[test]
    fn out_time_parser_rejects_invalid_clock_values() {
        assert_eq!(parse_out_time("not-a-time"), None);
        assert_eq!(parse_out_time("00:60:00"), None);
        assert_eq!(parse_out_time("00:00:60"), None);
    }
}
