//! Bottom transport bar, VLC-style: timecode on the left, Blender-style
//! transport controls (jump to start/end, play/pause, loop) centered, and a
//! volume control with mute toggle on the right.

use eframe::egui;

use super::VideoClipper;
use crate::icons::{self, Icon};
use crate::theme;
use crate::timecode;

impl VideoClipper {
    pub(super) fn draw_transport(&mut self, ui: &mut egui::Ui) {
        let Some(info) = self.info.clone() else {
            ui.centered_and_justified(|ui| {
                ui.label(
                    egui::RichText::new("Load a video to enable playback controls")
                        .color(theme::colors::muted_text())
                        .size(12.5),
                );
            });
            return;
        };

        let current = timecode::seconds_to_timecode(self.current_frame as f64 / info.fps);
        let end = timecode::seconds_to_timecode(self.end_frame as f64 / info.fps);

        ui.columns(3, |cols| {
            // ---- Left: timecode readout ----
            cols[0].horizontal_centered(|ui| {
                ui.add_space(4.0);
                ui.label(
                    egui::RichText::new(current)
                        .monospace()
                        .size(13.0)
                        .color(theme::colors::text()),
                );
                ui.label(
                    egui::RichText::new(format!("/ {end}"))
                        .monospace()
                        .size(12.0)
                        .color(theme::colors::muted_text()),
                );
            });

            // ---- Center: transport controls ----
            cols[1].horizontal_centered(|ui| {
                if icons::icon_button(ui, Icon::SkipBack, 30.0, false)
                    .on_hover_text("Jump to clip start (Home)")
                    .clicked()
                {
                    self.jump_to_start();
                }

                ui.add_space(4.0);

                let (play_icon, play_tip) = if self.is_playing {
                    (Icon::Pause, "Pause (Space)")
                } else {
                    (Icon::Play, "Play (Space)")
                };
                if icons::primary_icon_button(ui, play_icon, 38.0)
                    .on_hover_text(play_tip)
                    .clicked()
                {
                    self.toggle_play();
                }

                ui.add_space(4.0);

                if icons::icon_button(ui, Icon::SkipForward, 30.0, false)
                    .on_hover_text("Jump to clip end (End)")
                    .clicked()
                {
                    self.jump_to_end();
                }

                ui.add_space(8.0);

                if icons::icon_button(ui, Icon::Repeat, 30.0, self.loop_playback)
                    .on_hover_text("Loop playback (L)")
                    .clicked()
                {
                    self.loop_playback = !self.loop_playback;
                }
            });

            // ---- Right: volume ----
            cols[2].with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.add_space(4.0);
                let volume_icon = if self.muted || self.volume <= 0.001 {
                    Icon::VolumeX
                } else {
                    Icon::Volume2
                };
                if icons::icon_button(ui, volume_icon, 28.0, false)
                    .on_hover_text(if self.muted { "Unmute (M)" } else { "Mute (M)" })
                    .clicked()
                {
                    self.muted = !self.muted;
                    self.apply_volume();
                }

                ui.add_space(6.0);

                let slider = ui.add_sized(
                    egui::vec2(100.0, 20.0),
                    egui::Slider::new(&mut self.volume, 0.0..=1.0).show_value(false),
                );
                if slider.changed() {
                    // Dragging the slider unmutes, like VLC.
                    self.muted = false;
                    self.apply_volume();
                }

                ui.add_space(6.0);
                ui.label(
                    egui::RichText::new(format!("{:.0}%", self.volume * 100.0))
                        .size(11.5)
                        .color(theme::colors::muted_text()),
                );
            });
        });
    }
}
