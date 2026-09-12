//! `VideoClipper`: the main application state + UI orchestration, ported
//! from the old `VideoClipper.cs`.

mod controls;
mod export_dialog;
mod export_modal;
mod header;
mod preview;
mod timeline;
mod transport;
mod waveform;

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, Sender};
use std::time::Duration;

use eframe::egui;

use crate::audio::AudioPlayback;
use crate::export::{self, CropPixels, ExportHandle, ExportOptions, ExportParams};
use crate::ffmpeg_util::FfmpegCaps;
use crate::messages::ClipperMsg;
use crate::theme;
use crate::timecode;
use crate::video::{self, PlaybackClock, PlaybackController, PreviewFetcher, VideoInfo};

/// Normalized (0..1) crop rectangle over the video frame.
#[derive(Clone, Copy)]
pub(crate) struct CropRect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl Default for CropRect {
    fn default() -> Self {
        Self {
            x: 0.0,
            y: 0.0,
            w: 1.0,
            h: 1.0,
        }
    }
}

impl CropRect {
    /// Clamps size first, then position. This preserves the rect's size
    /// when it is dragged against an edge, and the position clamp range is
    /// always valid (`1.0 - size >= 0.0`), so it can never panic.
    pub fn clamp(&mut self) {
        self.w = self.w.clamp(0.05, 1.0);
        self.h = self.h.clamp(0.05, 1.0);
        self.x = self.x.clamp(0.0, 1.0 - self.w);
        self.y = self.y.clamp(0.0, 1.0 - self.h);
    }
}

/// Optional aspect-ratio constraint applied while resizing the crop rect.
/// The ratio is width/height in *pixels* (not normalized coordinates).
#[derive(Clone, Copy, PartialEq)]
pub(crate) enum AspectLock {
    Free,
    Ratio(f32),
}

impl AspectLock {
    pub const PRESETS: [(&'static str, AspectLock); 5] = [
        ("Free", AspectLock::Free),
        ("1:1", AspectLock::Ratio(1.0)),
        ("16:9", AspectLock::Ratio(16.0 / 9.0)),
        ("9:16", AspectLock::Ratio(9.0 / 16.0)),
        ("4:3", AspectLock::Ratio(4.0 / 3.0)),
    ];
}

pub struct VideoClipper {
    // Loaded video.
    video_path: Option<PathBuf>,
    info: Option<VideoInfo>,

    // Trim / playhead state, in frame indices.
    start_frame: i64,
    end_frame: i64,
    current_frame: i64,
    is_playing: bool,
    start_time_str: String,
    end_time_str: String,

    // Crop.
    is_cropping: bool,
    crop_rect: CropRect,
    crop_aspect: AspectLock,

    // Preview.
    preview_texture: Option<egui::TextureHandle>,
    preview_fetcher: Option<PreviewFetcher>,
    playback_ctrl: Option<PlaybackController>,

    // Timeline thumbnails.
    thumbnail_textures: Vec<Option<egui::TextureHandle>>,
    thumbnail_cancel: Option<Arc<AtomicBool>>,

    // Audio.
    audio: Option<AudioPlayback>,
    volume: f32,
    muted: bool,
    waveform_samples: Option<Vec<f32>>,

    // Playback looping.
    loop_playback: bool,

    // Export.
    export_options: ExportOptions,
    show_export_modal: bool,
    export_handle: Option<ExportHandle>,
    export_progress: f32,
    export_message: String,
    export_done: bool,

    // Status bar.
    status_message: String,
    status_progress: Option<f32>,

    // Capabilities of the installed ffmpeg build (probed async at startup).
    ffmpeg_caps: FfmpegCaps,

    // Background -> UI messaging.
    msg_tx: Sender<ClipperMsg>,
    msg_rx: Receiver<ClipperMsg>,
}

impl VideoClipper {
    const NUM_THUMBNAILS: usize = 40;
    const THUMBNAIL_SIZE: (u32, u32) = (160, 90);
    const WAVEFORM_BUCKETS: usize = 512;

    pub fn new() -> Self {
        let (msg_tx, msg_rx) = std::sync::mpsc::channel();

        let status_message = match crate::ffmpeg_util::check_available() {
            Ok(()) => String::new(),
            Err(err) => err,
        };

        let clipper = Self {
            video_path: None,
            info: None,
            start_frame: 0,
            end_frame: 0,
            current_frame: 0,
            is_playing: false,
            start_time_str: "00:00:00.000".to_string(),
            end_time_str: "00:00:00.000".to_string(),
            is_cropping: false,
            crop_rect: CropRect::default(),
            crop_aspect: AspectLock::Free,
            preview_texture: None,
            preview_fetcher: None,
            playback_ctrl: None,
            thumbnail_textures: Vec::new(),
            thumbnail_cancel: None,
            audio: None,
            volume: 1.0,
            muted: false,
            waveform_samples: None,
            loop_playback: true,
            export_options: ExportOptions::default(),
            show_export_modal: false,
            export_handle: None,
            export_progress: 0.0,
            export_message: String::new(),
            export_done: false,
            status_message,
            status_progress: None,
            ffmpeg_caps: FfmpegCaps::default(),
            msg_tx,
            msg_rx,
        };

        // Probe the ffmpeg build's encoders/muxers in the background so the
        // export dialog can offer exactly what's available.
        let caps_tx = clipper.msg_tx.clone();
        std::thread::spawn(move || {
            let caps = crate::ffmpeg_util::probe_caps();
            let _ = caps_tx.send(ClipperMsg::FfmpegCaps(caps));
        });

        clipper
    }

    pub fn ui(&mut self, ctx: &egui::Context) {
        self.drain_messages(ctx);
        self.handle_keyboard(ctx);
        self.tick_playback(ctx);

        let panel_frame = egui::Frame::new()
            .fill(theme::colors::panel())
            .inner_margin(egui::Margin::symmetric(14, 10));
        let card_frame = egui::Frame::new()
            .fill(theme::colors::background())
            .inner_margin(egui::Margin::symmetric(16, 12));

        egui::TopBottomPanel::top("header_panel")
            .frame(panel_frame)
            .exact_height(52.0)
            .show(ctx, |ui| self.draw_header(ui));

        egui::TopBottomPanel::bottom("transport_panel")
            .frame(panel_frame)
            .exact_height(56.0)
            .show(ctx, |ui| self.draw_transport(ui));

        egui::TopBottomPanel::bottom("waveform_panel")
            .frame(card_frame)
            .exact_height(64.0)
            .show(ctx, |ui| self.draw_waveform(ui));

        egui::TopBottomPanel::bottom("timeline_panel")
            .frame(card_frame)
            .exact_height(96.0)
            .show(ctx, |ui| self.draw_timeline(ui));

        egui::SidePanel::right("controls_panel")
            .frame(panel_frame)
            .exact_width(300.0)
            .resizable(false)
            .show(ctx, |ui| self.draw_controls(ui));

        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(theme::colors::background()))
            .show(ctx, |ui| self.draw_preview(ui));

        self.draw_export_dialog(ctx);
        self.draw_export_modal(ctx);

        let busy =
            self.is_playing || self.status_progress.is_some() || self.export_handle.is_some();
        if busy {
            ctx.request_repaint_after(Duration::from_millis(16));
        }
    }

    // ---- Messaging -------------------------------------------------

    fn drain_messages(&mut self, ctx: &egui::Context) {
        while let Ok(msg) = self.msg_rx.try_recv() {
            match msg {
                ClipperMsg::FrameReady {
                    frame_index,
                    rgba,
                    width,
                    height,
                } => {
                    self.set_preview_image(ctx, &rgba, width, height);
                    if !self.is_playing {
                        self.current_frame = frame_index;
                    }
                }
                ClipperMsg::ThumbnailReady {
                    index,
                    rgba,
                    width,
                    height,
                    done,
                    total,
                } => {
                    let image = egui::ColorImage::from_rgba_unmultiplied(
                        [width as usize, height as usize],
                        &rgba,
                    );
                    let tex = ctx.load_texture(
                        format!("thumb_{index}"),
                        image,
                        egui::TextureOptions::LINEAR,
                    );
                    if index < self.thumbnail_textures.len() {
                        self.thumbnail_textures[index] = Some(tex);
                    }
                    self.report_thumbnail_progress(done, total);
                }
                ClipperMsg::AudioReady {
                    samples,
                    channels,
                    sample_rate,
                    waveform,
                } => {
                    self.waveform_samples = Some(waveform);
                    match AudioPlayback::new(samples, channels, sample_rate, self.volume) {
                        Ok(audio) => self.audio = Some(audio),
                        Err(err) => {
                            self.status_message = format!("Audio playback unavailable: {err}");
                        }
                    }
                }
                ClipperMsg::AudioUnavailable(reason) => {
                    self.waveform_samples = None;
                    self.audio = None;
                    self.status_message = format!("No audio: {reason}");
                }
                ClipperMsg::ExportProgress { fraction, message } => {
                    if let Some(f) = fraction {
                        self.export_progress = f;
                    }
                    self.export_message = message;
                }
                ClipperMsg::ExportFinished { outcome } => {
                    self.export_message = outcome.message();
                    self.export_done = true;
                }
                ClipperMsg::Status { message, progress } => {
                    self.status_message = message;
                    self.status_progress = progress;
                }
                ClipperMsg::FfmpegCaps(caps) => {
                    self.ffmpeg_caps = caps;
                }
            }
        }
    }

    fn report_thumbnail_progress(&mut self, done: usize, total: usize) {
        if done >= total {
            self.status_message = "Thumbnails loaded.".to_string();
            self.status_progress = None;
            self.thumbnail_cancel = None;
        } else {
            self.status_message = format!("Loading thumbnails... ({done}/{total})");
            self.status_progress = Some(done as f32 / total.max(1) as f32);
        }
    }

    fn set_preview_image(&mut self, ctx: &egui::Context, rgba: &[u8], width: u32, height: u32) {
        let size = [width as usize, height as usize];
        let image = egui::ColorImage::from_rgba_unmultiplied(size, rgba);
        match &mut self.preview_texture {
            Some(tex) if tex.size() == size => tex.set(image, egui::TextureOptions::LINEAR),
            _ => {
                self.preview_texture =
                    Some(ctx.load_texture("preview", image, egui::TextureOptions::LINEAR));
            }
        }
    }

    // ---- Loading -----------------------------------------------------

    fn load_video(&mut self, path: PathBuf) {
        let info = match video::probe(&path) {
            Ok(info) => info,
            Err(err) => {
                self.status_message = format!("Failed to open video: {err}");
                return;
            }
        };

        self.reset_state();

        self.video_path = Some(path.clone());
        self.start_frame = 0;
        self.end_frame = (info.frame_count - 1).max(0);
        self.current_frame = 0;
        self.thumbnail_textures = vec![None; Self::NUM_THUMBNAILS];

        self.preview_fetcher = Some(PreviewFetcher::new(
            path.clone(),
            (info.width, info.height),
            self.msg_tx.clone(),
        ));
        self.info = Some(info.clone());

        self.update_timestamps();
        self.show_frame(0);

        self.status_message = "Loading thumbnails...".to_string();
        self.status_progress = Some(0.0);
        self.thumbnail_cancel = Some(video::spawn_thumbnail_generation(
            path.clone(),
            info.duration,
            Self::NUM_THUMBNAILS,
            Self::THUMBNAIL_SIZE,
            self.msg_tx.clone(),
        ));

        crate::audio::extract_async(path, Self::WAVEFORM_BUCKETS, self.msg_tx.clone());
    }

    fn reset_state(&mut self) {
        self.is_playing = false;
        self.playback_ctrl = None;
        self.preview_fetcher = None;

        if let Some(cancel) = self.thumbnail_cancel.take() {
            cancel.store(true, Ordering::Relaxed);
        }
        self.thumbnail_textures.clear();
        self.preview_texture = None;

        self.audio = None;
        self.waveform_samples = None;

        self.is_cropping = false;
        self.crop_rect = CropRect::default();
        self.crop_aspect = AspectLock::Free;

        if let Some(handle) = self.export_handle.take() {
            handle.cancel();
        }
        self.show_export_modal = false;
        self.export_progress = 0.0;
        self.export_message.clear();
        self.export_done = false;

        self.status_message.clear();
        self.status_progress = None;

        self.info = None;
        self.video_path = None;
    }

    // ---- Playback / seeking -------------------------------------------

    fn show_frame(&mut self, frame: i64) {
        let Some(info) = &self.info else { return };
        let frame = frame.clamp(0, (info.frame_count - 1).max(0));
        self.current_frame = frame;
        if let Some(fetcher) = &self.preview_fetcher {
            fetcher.request(frame, frame as f64 / info.fps.max(0.001));
        }
    }

    fn toggle_play(&mut self) {
        if self.is_playing {
            self.stop_playback();
        } else {
            self.start_playback();
        }
    }

    /// Blender-style transport: relocate the playhead, continuing playback
    /// if it was already running.
    fn jump_to(&mut self, frame: i64) {
        if self.is_playing {
            self.stop_playback();
            self.current_frame = frame;
            self.start_playback();
        } else {
            self.show_frame(frame);
        }
    }

    fn jump_to_start(&mut self) {
        self.jump_to(self.start_frame);
    }

    fn jump_to_end(&mut self) {
        self.jump_to(self.end_frame);
    }

    fn start_playback(&mut self) {
        let Some(path) = self.video_path.clone() else {
            return;
        };
        let Some(info) = self.info.clone() else {
            return;
        };
        if self.end_frame <= self.start_frame {
            return;
        }

        let play_from = self.current_frame.clamp(self.start_frame, self.end_frame);
        let native_size = (info.width, info.height);

        let clock = if let Some(audio) = &self.audio {
            audio.seek_to_seconds(play_from as f64 / info.fps);
            audio.play();
            PlaybackClock::Audio {
                position: audio.position_handle(),
                sample_rate: audio.sample_rate(),
            }
        } else {
            PlaybackClock::wall(play_from as f64 / info.fps)
        };

        match PlaybackController::start(
            path,
            play_from,
            self.end_frame,
            info.fps,
            native_size,
            clock,
        ) {
            Ok(ctrl) => {
                self.playback_ctrl = Some(ctrl);
                self.is_playing = true;
            }
            Err(err) => {
                self.status_message = format!("Failed to start playback: {err}");
            }
        }
    }

    fn stop_playback(&mut self) {
        self.is_playing = false;
        self.playback_ctrl = None;
        if let Some(audio) = &self.audio {
            audio.pause();
        }
    }

    fn tick_playback(&mut self, ctx: &egui::Context) {
        if !self.is_playing {
            return;
        }

        let latest = self.playback_ctrl.as_ref().and_then(|c| c.take_latest());
        let finished = self
            .playback_ctrl
            .as_ref()
            .map(|c| c.is_finished())
            .unwrap_or(false);

        if let Some((frame_index, rgba)) = latest {
            if let Some(info) = &self.info {
                let (width, height) = (info.width, info.height);
                self.set_preview_image(ctx, &rgba, width, height);
            }
            self.current_frame = frame_index;
        }

        if finished {
            if self.loop_playback {
                self.current_frame = self.start_frame;
                self.stop_playback();
                self.start_playback();
            } else {
                self.stop_playback();
            }
        }
    }

    // ---- Timecodes ------------------------------------------------------

    fn update_timestamps(&mut self) {
        if let Some(info) = &self.info
            && info.fps > 0.0
        {
            self.start_time_str = timecode::seconds_to_timecode(self.start_frame as f64 / info.fps);
            self.end_time_str = timecode::seconds_to_timecode(self.end_frame as f64 / info.fps);
        }
    }

    fn update_from_text(&mut self) {
        let Some(info) = &self.info else { return };
        if let Some(secs) = timecode::timecode_to_seconds(&self.start_time_str) {
            self.start_frame = ((secs * info.fps) as i64).max(0);
        }
        if let Some(secs) = timecode::timecode_to_seconds(&self.end_time_str) {
            self.end_frame = ((secs * info.fps) as i64).min((info.frame_count - 1).max(0));
        }
    }

    // ---- Keyboard ---------------------------------------------------

    fn handle_keyboard(&mut self, ctx: &egui::Context) {
        if ctx.wants_keyboard_input() {
            return;
        }

        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            return;
        }

        let Some(info) = self.info.clone() else {
            return;
        };

        if ctx.input(|i| i.key_pressed(egui::Key::Space)) {
            self.toggle_play();
        }
        if ctx.input(|i| i.key_pressed(egui::Key::ArrowLeft)) {
            self.show_frame((self.current_frame - 1).max(0));
        }
        if ctx.input(|i| i.key_pressed(egui::Key::ArrowRight)) {
            self.show_frame((self.current_frame + 1).min((info.frame_count - 1).max(0)));
        }
        if ctx.input(|i| i.key_pressed(egui::Key::Home)) {
            self.jump_to_start();
        }
        if ctx.input(|i| i.key_pressed(egui::Key::End)) {
            self.jump_to_end();
        }
        if ctx.input(|i| i.key_pressed(egui::Key::L)) {
            self.loop_playback = !self.loop_playback;
        }
        if ctx.input(|i| i.key_pressed(egui::Key::M)) {
            self.muted = !self.muted;
            self.apply_volume();
        }
        if ctx.input(|i| i.key_down(egui::Key::I)) {
            self.start_frame = self.current_frame;
            self.update_timestamps();
        }
        if ctx.input(|i| i.key_down(egui::Key::O)) {
            self.end_frame = self.current_frame;
            self.update_timestamps();
        }
    }

    /// Pushes the current volume/mute state into the audio player.
    pub(crate) fn apply_volume(&self) {
        if let Some(audio) = &self.audio {
            audio.set_volume(if self.muted { 0.0 } else { self.volume });
        }
    }

    // ---- File dialogs / export kickoff -----------------------------------

    fn show_open_file_dialog(&mut self) {
        let mut dialog = rfd::FileDialog::new().add_filter("Video", &["mp4", "mkv", "mov", "avi"]);
        if let Some(dir) = dirs::video_dir() {
            dialog = dialog.set_directory(dir);
        }
        if let Some(path) = dialog.pick_file() {
            self.load_video(path);
        }
    }

    fn show_save_file_dialog(&mut self) {
        let container = self.export_options.container;
        let mut dialog =
            rfd::FileDialog::new().add_filter(container.label(), &[container.extension()]);
        if let Some(dir) = dirs::video_dir() {
            dialog = dialog.set_directory(dir);
        }
        if let Some(mut path) = dialog.save_file() {
            let has_ext = path
                .extension()
                .map(|e| e.eq_ignore_ascii_case(container.extension()))
                .unwrap_or(false);
            if !has_ext {
                path.set_extension(container.extension());
            }
            self.start_export(path);
        }
    }

    fn start_export(&mut self, output_path: PathBuf) {
        if self.export_handle.is_some() && !self.export_done {
            return;
        }
        let Some(video_path) = self.video_path.clone() else {
            return;
        };
        let Some(info) = self.info.clone() else {
            return;
        };

        let crop = if self.is_cropping {
            Some(CropPixels {
                x: (self.crop_rect.x * info.width as f32).round() as i64,
                y: (self.crop_rect.y * info.height as f32).round() as i64,
                w: (self.crop_rect.w * info.width as f32).round() as i64,
                h: (self.crop_rect.h * info.height as f32).round() as i64,
            })
        } else {
            None
        };

        let params = ExportParams {
            video_path,
            output_path,
            start_secs: self.start_frame as f64 / info.fps,
            end_secs: self.end_frame as f64 / info.fps,
            crop,
            options: self.export_options.clone(),
        };

        self.export_progress = 0.0;
        self.export_message = "Starting export...".to_string();
        self.export_done = false;
        self.export_handle = Some(export::start_export(params, self.msg_tx.clone()));
    }
}
