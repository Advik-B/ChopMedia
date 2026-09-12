//! Top-level `eframe::App` implementation, ported from the old `Application.cs`.

use std::sync::Arc;

use eframe::egui;

use crate::clipper::VideoClipper;
use crate::theme;

pub struct ChopMediaApp {
    clipper: VideoClipper,
}

impl ChopMediaApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        cc.egui_ctx.set_visuals(theme::visuals());
        cc.egui_ctx.style_mut(|style| {
            style.spacing = theme::spacing();
        });
        setup_fonts(&cc.egui_ctx);

        Self {
            clipper: VideoClipper::new(),
        }
    }
}

impl eframe::App for ChopMediaApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.clipper.ui(ctx);
    }
}

fn setup_fonts(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();
    fonts.font_data.insert(
        "inter_semibold".to_owned(),
        Arc::new(egui::FontData::from_static(include_bytes!(
            "../assets/fonts/Inter-SemiBold.ttf"
        ))),
    );

    fonts
        .families
        .entry(egui::FontFamily::Proportional)
        .or_default()
        .insert(0, "inter_semibold".to_owned());
    fonts
        .families
        .entry(egui::FontFamily::Monospace)
        .or_default()
        .push("inter_semibold".to_owned());

    ctx.set_fonts(fonts);
}
