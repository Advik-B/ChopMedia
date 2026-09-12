//! ChopMedia: a video trimmer/clipper with an egui GUI.
//!
//! The binary is dual-mode: launched with no terminal attached (e.g.
//! double-clicked) it opens the GUI; launched from a terminal it behaves
//! as a CLI around the same ffmpeg pipeline. The mode is chosen in `main`
//! via `cli::terminal_launch` (the isatty equivalent) and an explicit
//! subcommand always wins.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod audio;
mod cli;
mod clipper;
mod export;
mod ffmpeg_util;
mod icons;
mod messages;
mod theme;
mod timecode;
mod video;

use eframe::egui;
use std::io::Write;

use app::ChopMediaApp;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();

    // GUI unless a CLI subcommand is present or we were launched from a
    // terminal (`gui` forces the GUI even from a terminal).
    let first = args.first().map(String::as_str);
    let cli_command = matches!(
        first,
        Some("info" | "caps" | "export" | "help" | "--help" | "-h" | "--version" | "-V")
    );
    if first != Some("gui") && (cli_command || cli::terminal_launch()) {
        std::process::exit(cli::run(&args));
    }

    if let Err(err) = run_gui() {
        // There may be no console to print to (double-clicked release
        // build), so write defensively.
        let _ = writeln!(std::io::stderr(), "error: {err:#}");
        std::process::exit(1);
    }
}

fn run_gui() -> eframe::Result<()> {
    let mut viewport = egui::ViewportBuilder::default()
        .with_inner_size([1280.0, 720.0])
        .with_resizable(true);

    if let Some(icon) = load_icon() {
        viewport = viewport.with_icon(icon);
    }

    let native_options = eframe::NativeOptions {
        viewport,
        ..Default::default()
    };

    eframe::run_native(
        "ChopMedia",
        native_options,
        Box::new(|cc| Ok(Box::new(ChopMediaApp::new(cc)))),
    )
}

fn load_icon() -> Option<egui::IconData> {
    let bytes = include_bytes!("../assets/icons/App.png");
    let image = image::load_from_memory(bytes).ok()?.into_rgba8();
    let (width, height) = image.dimensions();
    Some(egui::IconData {
        rgba: image.into_raw(),
        width,
        height,
    })
}
