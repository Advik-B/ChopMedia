//! Export settings modal: container, mode, codec, quality, speed, audio,
//! resolution, and frame rate. Opened from the header's Export Clip button;
//! the actual file picking happens in `show_save_file_dialog`.

use eframe::egui;

use super::VideoClipper;
use crate::export::{AudioMode, Container, EncoderSpeed, ExportMode, VideoCodec};
use crate::ffmpeg_util::FfmpegCaps;
use crate::icons::{self, Icon};
use crate::theme::colors;
use crate::timecode;
use crate::video::VideoInfo;

const SCALE_OPTIONS: [(&str, f32); 4] = [
    ("Original", 1.0),
    ("75%", 0.75),
    ("50%", 0.5),
    ("25%", 0.25),
];

const FPS_OPTIONS: [(&str, Option<f64>); 4] = [
    ("Original", None),
    ("60 fps", Some(60.0)),
    ("30 fps", Some(30.0)),
    ("24 fps", Some(24.0)),
];

impl VideoClipper {
    pub(super) fn draw_export_modal(&mut self, ctx: &egui::Context) {
        if !self.show_export_modal {
            return;
        }
        let Some(info) = self.info.clone() else {
            self.show_export_modal = false;
            return;
        };

        let mut open = true;
        egui::Window::new("Export Clip")
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
            .open(&mut open)
            .show(ctx, |ui| {
                ui.set_min_width(400.0);
                self.export_modal_body(ui, &info);
            });
        if !open {
            self.show_export_modal = false;
        }
    }

    fn export_modal_body(&mut self, ui: &mut egui::Ui, info: &VideoInfo) {
        let has_crop = self.is_cropping;
        let crop_rect = self.crop_rect;
        let start_frame = self.start_frame;
        let end_frame = self.end_frame;
        let caps = self.ffmpeg_caps.clone();

        // Work on a local copy and write it back at the end, so the buttons
        // below can freely borrow `self`.
        let mut opts = self.export_options.clone();

        // ---- Container (only muxers present in the probed ffmpeg build) ----
        let containers: Vec<(&'static str, Container)> = Container::ALL
            .iter()
            .filter(|c| caps.has_muxer(c.muxer_name()))
            .map(|c| (c.label(), *c))
            .collect();
        if let Some((_, first)) = containers.first()
            && !containers.iter().any(|(_, c)| *c == opts.container)
        {
            opts.container = *first;
        }
        let prev_container = opts.container;
        combo_row(
            ui,
            "export_container",
            "Container",
            &mut opts.container,
            &containers,
        );
        if opts.container != prev_container {
            if !opts.video_codec.is_usable(opts.container, &caps) {
                opts.video_codec = VideoCodec::default_for(opts.container, &caps);
            }
            opts.audio = match (opts.audio, opts.container) {
                (AudioMode::Aac(_), Container::WebM) => AudioMode::Opus(160),
                (AudioMode::Opus(_), c) if c != Container::WebM => AudioMode::Aac(192),
                (audio, _) => audio,
            };
        }

        // ---- Resolution / frame rate ----
        combo_row(
            ui,
            "export_scale",
            "Resolution",
            &mut opts.scale,
            &SCALE_OPTIONS,
        );
        combo_row(ui, "export_fps", "Frame rate", &mut opts.fps, &FPS_OPTIONS);

        // ---- Mode ----
        let forced_reencode = has_crop || opts.scale != 1.0 || opts.fps.is_some();
        if forced_reencode {
            opts.mode = ExportMode::Reencode;
        }
        ui.horizontal(|ui| {
            row_label(ui, "Mode");
            if ui
                .add_enabled(
                    !forced_reencode,
                    egui::SelectableLabel::new(
                        matches!(opts.mode, ExportMode::StreamCopy),
                        "Stream copy (fast)",
                    ),
                )
                .on_hover_text("Copy packets without re-encoding (keyframe-bound cuts)")
                .clicked()
            {
                opts.mode = ExportMode::StreamCopy;
            }
            if ui
                .selectable_label(matches!(opts.mode, ExportMode::Reencode), "Re-encode")
                .on_hover_text("Frame-accurate cut, re-encoded with the settings below")
                .clicked()
            {
                opts.mode = ExportMode::Reencode;
            }
        });
        if forced_reencode {
            ui.label(
                egui::RichText::new("Crop, scale, or fps changes require re-encoding.")
                    .size(11.0)
                    .color(colors::warning()),
            );
        }

        // ---- Codec / quality / speed (re-encode only) ----
        // Codecs are filtered to what the probed ffmpeg build supports.
        let reencode = opts.needs_reencode(has_crop);
        if reencode {
            let codecs: Vec<(&'static str, VideoCodec)> = VideoCodec::ALL
                .iter()
                .filter(|c| c.is_usable(opts.container, &caps))
                .map(|c| (c.label(), *c))
                .collect();
            if let Some((_, first)) = codecs.first()
                && !codecs.iter().any(|(_, c)| *c == opts.video_codec)
            {
                opts.video_codec = *first;
            }
            combo_row(ui, "export_codec", "Codec", &mut opts.video_codec, &codecs);

            // Quality: CRF range probed from the encoder's own help.
            let crf_range = caps
                .encoder_info(opts.video_codec.encoder())
                .and_then(|i| i.crf_range)
                .map(|(lo, hi)| (lo.max(0.0).ceil() as u8)..=(hi.floor() as u8))
                .unwrap_or_else(|| opts.video_codec.crf_range());
            opts.crf = opts.crf.clamp(*crf_range.start(), *crf_range.end());
            ui.horizontal(|ui| {
                row_label(ui, "Quality");
                ui.add(egui::Slider::new(&mut opts.crf, crf_range));
            });
            ui.label(
                egui::RichText::new("CRF: lower is better (0 is lossless)")
                    .size(11.0)
                    .color(colors::muted_text()),
            );

            // Speed: named presets for x264/x265; numeric values for
            // SVT-AV1 (-preset) and VP9 (-cpu-used), all probed.
            let speed_options = speed_options(opts.video_codec, &caps);
            if !speed_options.iter().any(|(_, s)| *s == opts.speed) {
                opts.speed = default_speed(opts.video_codec, &caps);
                if !speed_options.iter().any(|(_, s)| *s == opts.speed)
                    && let Some((_, first)) = speed_options.first()
                {
                    opts.speed = first.clone();
                }
            }
            combo_row(ui, "export_speed", "Speed", &mut opts.speed, &speed_options);
            if matches!(opts.speed, EncoderSpeed::Numeric(_)) {
                ui.label(
                    egui::RichText::new("Lower numbers are slower / better quality.")
                        .size(11.0)
                        .color(colors::muted_text()),
                );
            }
        }

        // ---- Audio ----
        let audio_options = AudioMode::presets_for(opts.container, &caps);
        if !audio_options.iter().any(|(_, a)| *a == opts.audio) {
            opts.audio = AudioMode::Copy;
        }
        combo_row(ui, "export_audio", "Audio", &mut opts.audio, &audio_options);

        if caps.was_probed() {
            ui.label(
                egui::RichText::new("Options auto-detected from your ffmpeg build.")
                    .size(10.5)
                    .color(colors::muted_text()),
            );
        }

        // ---- Summary ----
        ui.add_space(6.0);
        ui.separator();
        ui.add_space(6.0);

        let (base_w, base_h) = if has_crop {
            (
                (crop_rect.w * info.width as f32).round() as i64,
                (crop_rect.h * info.height as f32).round() as i64,
            )
        } else {
            (info.width as i64, info.height as i64)
        };
        let out_w = ((base_w as f32 * opts.scale).round() as i64 / 2) * 2;
        let out_h = ((base_h as f32 * opts.scale).round() as i64 / 2) * 2;
        let clip_len =
            timecode::seconds_to_timecode((end_frame - start_frame).max(0) as f64 / info.fps);
        let mode_desc = if reencode {
            format!(
                "{} · CRF {} · speed {}",
                opts.video_codec.label(),
                opts.crf,
                speed_label(&opts.speed)
            )
        } else {
            "Stream copy".to_string()
        };
        ui.label(
            egui::RichText::new(format!(
                "Output: {out_w} x {out_h}  ·  {clip_len}  ·  {mode_desc}  ·  {}",
                opts.audio.label(),
            ))
            .size(11.5)
            .color(colors::muted_text()),
        );

        self.export_options = opts;

        // ---- Actions ----
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            if icons::icon_text_button(ui, Icon::Download, "Export...", true).clicked() {
                self.show_export_modal = false;
                self.show_save_file_dialog();
            }
            if ui.button("Cancel").clicked() {
                self.show_export_modal = false;
            }
        });
    }
}

fn row_label(ui: &mut egui::Ui, text: &str) {
    ui.add_sized(
        egui::vec2(90.0, 20.0),
        egui::Label::new(
            egui::RichText::new(text)
                .size(12.5)
                .color(colors::muted_text()),
        ),
    );
}

fn combo_row<T: PartialEq + Clone, S: AsRef<str>>(
    ui: &mut egui::Ui,
    id: &str,
    label: &str,
    current: &mut T,
    options: &[(S, T)],
) {
    ui.horizontal(|ui| {
        row_label(ui, label);
        let selected = options
            .iter()
            .find(|(_, v)| v == current)
            .map(|(l, _)| l.as_ref().to_owned())
            .unwrap_or_else(|| "?".to_owned());
        egui::ComboBox::from_id_salt(id)
            .selected_text(selected)
            .show_ui(ui, |ui| {
                for (label, value) in options {
                    if ui
                        .selectable_label(*current == *value, label.as_ref())
                        .clicked()
                    {
                        *current = value.clone();
                    }
                }
            });
    });
}

/// Named `-preset` choices for x264/x265, probed from the encoder's help
/// with a static fallback.
fn named_presets(caps: &FfmpegCaps, encoder: &str) -> Vec<String> {
    const FALLBACK: [&str; 9] = [
        "ultrafast",
        "superfast",
        "veryfast",
        "faster",
        "fast",
        "medium",
        "slow",
        "slower",
        "veryslow",
    ];
    caps.encoder_info(encoder)
        .map(|i| i.presets.clone())
        .filter(|p| !p.is_empty())
        .unwrap_or_else(|| FALLBACK.iter().map(|s| s.to_string()).collect())
}

/// Speed choices offered for the codec: named presets or probed numeric
/// ranges, with static fallbacks.
fn speed_options(codec: VideoCodec, caps: &FfmpegCaps) -> Vec<(String, EncoderSpeed)> {
    let info = caps.encoder_info(codec.encoder());
    match codec {
        VideoCodec::H264 | VideoCodec::H265 => named_presets(caps, codec.encoder())
            .into_iter()
            .map(|p| (p.clone(), EncoderSpeed::Named(p)))
            .collect(),
        VideoCodec::Av1 => {
            let (lo, hi) = info
                .and_then(|i| i.preset_range)
                .map(|(lo, hi)| (lo.max(0), hi))
                .unwrap_or((0, 13));
            (lo..=hi)
                .map(|n| (n.to_string(), EncoderSpeed::Numeric(n)))
                .collect()
        }
        VideoCodec::Vp9 => {
            let (lo, hi) = info
                .and_then(|i| i.cpu_used_range)
                .map(|(lo, hi)| (lo.max(0), hi))
                .unwrap_or((0, 8));
            (lo..=hi)
                .map(|n| (n.to_string(), EncoderSpeed::Numeric(n)))
                .collect()
        }
    }
}

/// The codec's default speed, preferring ffmpeg's own probed default.
fn default_speed(codec: VideoCodec, caps: &FfmpegCaps) -> EncoderSpeed {
    let info = caps.encoder_info(codec.encoder());
    match codec {
        VideoCodec::H264 | VideoCodec::H265 => {
            let presets = named_presets(caps, codec.encoder());
            let name = presets
                .iter()
                .find(|p| p.as_str() == "medium")
                .or_else(|| presets.first())
                .cloned()
                .unwrap_or_else(|| "medium".to_string());
            EncoderSpeed::Named(name)
        }
        VideoCodec::Av1 => {
            // Negative values are "unset" sentinels; use the encoder's
            // documented default instead.
            EncoderSpeed::Numeric(
                info.and_then(|i| i.preset_default)
                    .filter(|n| *n >= 0)
                    .unwrap_or(6),
            )
        }
        VideoCodec::Vp9 => EncoderSpeed::Numeric(
            info.and_then(|i| i.cpu_used_default)
                .filter(|n| *n >= 0)
                .unwrap_or(2),
        ),
    }
}

fn speed_label(speed: &EncoderSpeed) -> String {
    match speed {
        EncoderSpeed::Named(preset) => preset.clone(),
        EncoderSpeed::Numeric(n) => n.to_string(),
    }
}
