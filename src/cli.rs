//! Terminal mode.
//!
//! ChopMedia is a GUI app, but the same executable doubles as a CLI when it
//! is launched with a terminal attached: inspecting videos, printing the
//! probed ffmpeg capabilities, and exporting clips without opening a
//! window. `main` picks the mode via [`terminal_launch`], the "isatty"
//! equivalent for a GUI-subsystem Windows binary.

use std::collections::VecDeque;
use std::io::{IsTerminal, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, mpsc};

use clap::{Args, CommandFactory, Parser, Subcommand};

use crate::export::{
    self, AudioMode, Container, CropPixels, EncoderSpeed, ExportMode, ExportOptions, ExportParams,
    VideoCodec,
};
use crate::ffmpeg_util;
use crate::messages::{ClipperMsg, ExportOutcome};
use crate::timecode::{seconds_to_timecode, timecode_to_seconds};
use crate::video::probe;

/// How many ffmpeg stderr lines to keep around for failure diagnostics.
const TAIL_LINES: usize = 8;
/// Width of the interactive progress bar.
const BAR_WIDTH: usize = 30;

#[cfg(windows)]
mod windows_console {
    //! Minimal kernel32 console bindings. A GUI-subsystem Windows binary
    //! has no console of its own, so terminal detection has to work with
    //! the raw API instead of a plain isatty.

    use std::ffi::c_void;

    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn AttachConsole(dwProcessId: usize) -> i32;
        fn CreateFileW(
            lpFileName: *const u16,
            dwDesiredAccess: u32,
            dwShareMode: u32,
            lpSecurityAttributes: *mut c_void,
            dwCreationDisposition: u32,
            dwFlagsAndAttributes: u32,
            hTemplateFile: *mut c_void,
        ) -> *mut c_void;
        fn GetConsoleProcessList(lpdwProcessList: *mut u32, dwProcessCount: u32) -> u32;
        fn GetConsoleWindow() -> *mut c_void;
        fn GetStdHandle(nStdHandle: u32) -> *mut c_void;
        fn SetStdHandle(nStdHandle: u32, hHandle: *mut c_void) -> i32;
    }

    const ATTACH_PARENT_PROCESS: usize = usize::MAX;
    const GENERIC_WRITE: u32 = 0x4000_0000;
    const FILE_SHARE_READ: u32 = 0x0000_0001;
    const FILE_SHARE_WRITE: u32 = 0x0000_0002;
    const OPEN_EXISTING: u32 = 3;
    const STD_OUTPUT_HANDLE: u32 = 11;
    const STD_ERROR_HANDLE: u32 = 12;

    /// True when the process is attached to a console right now.
    pub fn is_attached() -> bool {
        unsafe { !GetConsoleWindow().is_null() }
    }

    /// True when other processes share our console, i.e. we were launched
    /// from a shell rather than getting a console window of our own
    /// (double-clicked console-subsystem binary).
    pub fn shared_console() -> bool {
        let mut list = [0u32; 64];
        let count = unsafe { GetConsoleProcessList(list.as_mut_ptr(), list.len() as u32) };
        count != 1
    }

    /// Attaches the parent process's console (the "launched from a
    /// terminal" case for a GUI-subsystem binary). Std handles are only
    /// redirected to the console buffer when they were unset, so redirected
    /// pipes (e.g. an MSYS pty) keep working.
    pub fn attach_parent_console() -> bool {
        unsafe {
            if AttachConsole(ATTACH_PARENT_PROCESS) == 0 {
                return false;
            }
            if GetStdHandle(STD_OUTPUT_HANDLE).is_null() {
                let name: Vec<u16> = "CONOUT$\0".encode_utf16().collect();
                let handle = CreateFileW(
                    name.as_ptr(),
                    GENERIC_WRITE,
                    FILE_SHARE_READ | FILE_SHARE_WRITE,
                    std::ptr::null_mut(),
                    OPEN_EXISTING,
                    0,
                    std::ptr::null_mut(),
                );
                if !handle.is_null() && handle as isize != -1 {
                    SetStdHandle(STD_OUTPUT_HANDLE, handle);
                    SetStdHandle(STD_ERROR_HANDLE, handle);
                }
            }
            true
        }
    }
}

/// True when the process was launched from an interactive terminal.
///
/// On Windows this is more than an isatty: release builds are
/// GUI-subsystem binaries without a console, so after checking the std
/// handles we fall back to inspecting (and attaching to) the Windows
/// console itself. On other platforms it is a plain `isatty`.
#[cfg(windows)]
pub fn terminal_launch() -> bool {
    // Fast path: the shell handed us terminal std handles (cmd, ConPTY).
    if std::io::stdout().is_terminal() {
        return true;
    }
    if windows_console::is_attached() {
        return windows_console::shared_console();
    }
    windows_console::attach_parent_console()
}

#[cfg(not(windows))]
pub fn terminal_launch() -> bool {
    std::io::stdout().is_terminal()
}

/// Runs the CLI with the given arguments (argv without the program name)
/// and returns the process exit code.
pub fn run(args: &[String]) -> i32 {
    let cli = match Cli::try_parse_from(
        std::iter::once("ChopMedia").chain(args.iter().map(String::as_str)),
    ) {
        Ok(cli) => cli,
        Err(err) => {
            let code = err.exit_code();
            let _ = err.print();
            return code;
        }
    };

    match cli.command {
        None => {
            print_help();
            0
        }
        Some(CliCommand::Gui) => 0,
        Some(CliCommand::Info { input }) => cmd_info(&input),
        Some(CliCommand::Caps) => cmd_caps(),
        Some(CliCommand::Export(cfg)) => cmd_export(cfg),
    }
}

fn print_help() {
    let mut command = Cli::command();
    let _ = command.print_help();
    println!();
}

fn cmd_info(path: &Path) -> i32 {
    match probe::probe(path) {
        Ok(info) => {
            println!("File       {}", path.display());
            println!("Resolution {}x{}", info.width, info.height);
            println!("Frame rate {:.3} fps", info.fps);
            println!("Frames     {}", info.frame_count);
            println!(
                "Duration   {} ({:.3} s)",
                seconds_to_timecode(info.duration),
                info.duration
            );
            0
        }
        Err(err) => {
            eprintln!("error: {err:#}");
            1
        }
    }
}

fn cmd_caps() -> i32 {
    if let Err(msg) = ffmpeg_util::check_available() {
        eprintln!("error: {msg}");
        return 1;
    }
    let caps = ffmpeg_util::probe_caps();
    if !caps.was_probed() {
        eprintln!("error: failed to probe ffmpeg capabilities");
        return 1;
    }

    if let Ok(out) = ffmpeg_util::command("ffmpeg").arg("-version").output()
        && let Some(line) = String::from_utf8_lossy(&out.stdout).lines().next()
    {
        println!("{line}");
    }
    println!(
        "probed {} encoders and {} muxers",
        caps.encoders().len(),
        caps.muxers().len()
    );
    println!();
    println!("Containers:");
    for container in Container::ALL {
        println!(
            "  {:<10} {}",
            container.label(),
            yes_no(caps.has_muxer(container.muxer_name()))
        );
    }
    println!("Video encoders:");
    for codec in VideoCodec::ALL {
        let encoder = codec.encoder();
        println!("  {:<12} {}", encoder, yes_no(caps.has_encoder(encoder)));
        if let Some(info) = caps.encoder_info(encoder) {
            if !info.presets.is_empty() {
                println!("    presets  : {}", info.presets.join(", "));
            }
            if let Some((lo, hi)) = info.preset_range {
                let default = info
                    .preset_default
                    .map(|d| format!(" (default {d})"))
                    .unwrap_or_default();
                println!("    preset   : {lo} to {hi}{default}");
            }
            if let Some((lo, hi)) = info.cpu_used_range {
                let default = info
                    .cpu_used_default
                    .map(|d| format!(" (default {d})"))
                    .unwrap_or_default();
                println!("    cpu-used : {lo} to {hi}{default}");
            }
            if let Some((lo, hi)) = info.crf_range {
                println!("    crf      : {lo} to {hi}");
            }
        }
    }
    println!("Audio encoders:");
    for encoder in ["aac", "libopus"] {
        println!("  {:<12} {}", encoder, yes_no(caps.has_encoder(encoder)));
    }
    0
}

fn yes_no(ok: bool) -> &'static str {
    if ok { "yes" } else { "no" }
}

fn cmd_export(cfg: ExportArgs) -> i32 {
    let info = match probe::probe(&cfg.input) {
        Ok(info) => info,
        Err(err) => {
            eprintln!("error: {err:#}");
            return 1;
        }
    };

    let start = cfg.start.unwrap_or(0.0);
    if start < 0.0 {
        eprintln!("error: --start must be >= 0");
        return 2;
    }
    let end = cfg.end.unwrap_or(info.duration);
    if end <= start {
        eprintln!("error: --end must come after --start");
        return 2;
    }
    if end > info.duration + 0.001 {
        eprintln!(
            "warning: --end exceeds the video duration ({:.3} s); ffmpeg stops at the end of the input",
            info.duration
        );
    }

    // Container: explicit flag, else inferred from the output extension,
    // else MP4. The output extension drives ffmpeg's muxer choice.
    let container = match cfg.container {
        Some(container) => container,
        None => container_from_extension(&cfg.output).unwrap_or(Container::Mp4),
    };
    let mut output = cfg.output.clone();
    match output.extension().and_then(|e| e.to_str()) {
        Some(ext) if ext.eq_ignore_ascii_case(container.extension()) => {}
        None => {
            output.set_extension(container.extension());
            println!(
                "note: output had no extension; using .{} (the {} container's default)",
                container.extension(),
                container.label()
            );
        }
        Some(ext) => {
            eprintln!(
                "warning: output extension .{ext} does not match the {} container; ffmpeg picks the muxer from the file extension",
                container.label()
            );
        }
    }

    let caps = ffmpeg_util::probe_caps();
    if !caps.has_muxer(container.muxer_name()) {
        eprintln!(
            "warning: this ffmpeg build has no {} muxer; the export will likely fail",
            container.label()
        );
    }
    let codec = cfg
        .codec
        .unwrap_or_else(|| VideoCodec::default_for(container, &caps));
    if !codec.is_usable(container, &caps) {
        eprintln!(
            "warning: encoder {} is missing from this ffmpeg build; the export will likely fail",
            codec.encoder()
        );
    }

    let audio = cfg.audio.unwrap_or(AudioMode::Copy);
    if matches!(audio, AudioMode::Aac(_)) && container == Container::WebM {
        eprintln!("error: AAC audio is not supported in WebM; use --audio opus");
        return 2;
    }
    if matches!(audio, AudioMode::Aac(_)) && !caps.has_encoder("aac") {
        eprintln!("warning: aac encoder missing; the export will likely fail");
    }
    if matches!(audio, AudioMode::Opus(_)) && !caps.has_encoder("libopus") {
        eprintln!("warning: libopus encoder missing; the export will likely fail");
    }

    let crf = cfg
        .crf
        .unwrap_or(20)
        .clamp(*codec.crf_range().start(), *codec.crf_range().end());
    let speed = cfg.speed.unwrap_or_else(|| default_speed(codec, &caps));
    let speed_text = speed_label(&speed);
    match (&speed, codec) {
        (EncoderSpeed::Named(_), VideoCodec::Av1 | VideoCodec::Vp9)
        | (EncoderSpeed::Numeric(_), VideoCodec::H264 | VideoCodec::H265) => {
            eprintln!(
                "warning: {} does not take {speed_text}; it is ignored and the encoder default is used",
                codec.encoder()
            );
        }
        _ => {}
    }

    let options = ExportOptions {
        container,
        mode: cfg.mode,
        video_codec: codec,
        crf,
        speed,
        audio,
        scale: cfg.scale.unwrap_or(1.0),
        fps: cfg.fps,
    };

    let has_crop = cfg.crop.is_some();
    if options.needs_reencode(has_crop) {
        println!(
            "Encoding: {} / {} ({}), CRF {crf}, {speed_text} / audio: {}",
            container.label(),
            codec.label(),
            codec.encoder(),
            audio.label()
        );
    } else {
        println!(
            "Encoding: {} / stream copy / audio: {}",
            container.label(),
            audio.label()
        );
    }
    if matches!(cfg.mode, ExportMode::StreamCopy) && options.needs_reencode(has_crop) {
        println!(
            "note: crop/scale/fps require a re-encode; re-encoding with {} ({})",
            codec.label(),
            codec.encoder()
        );
    }
    println!(
        "Input     : {} ({}x{}, {:.3} s)",
        cfg.input.display(),
        info.width,
        info.height,
        info.duration
    );
    println!("Output    : {}", output.display());
    println!(
        "Range     : {} - {} ({:.3} s)",
        seconds_to_timecode(start),
        seconds_to_timecode(end),
        end - start
    );
    if let Some(crop) = &cfg.crop {
        println!(
            "Crop      : {}x{} at ({}, {})",
            crop.w, crop.h, crop.x, crop.y
        );
    }
    if options.scale != 1.0 {
        println!("Scale     : {}x", options.scale);
    }
    if let Some(fps) = options.fps {
        println!("Frame rate: {fps} fps");
    }

    let params = ExportParams {
        video_path: cfg.input.clone(),
        output_path: output,
        start_secs: start,
        end_secs: end,
        crop: cfg.crop,
        options,
    };

    let (tx, rx) = mpsc::channel::<ClipperMsg>();
    let handle = export::start_export(params, tx);

    // Ctrl+C cancels the running ffmpeg instead of orphaning it.
    let cancel_handle = Arc::new(Mutex::new(Some(handle)));
    let handler_handle = cancel_handle.clone();
    if ctrlc::set_handler(move || {
        if let Some(handle) = &*handler_handle.lock().unwrap() {
            handle.cancel();
        }
    })
    .is_err()
    {
        eprintln!("warning: Ctrl+C handler unavailable; export cannot be interrupted cleanly");
    }

    let interactive = std::io::stdout().is_terminal();
    let mut tail: VecDeque<String> = VecDeque::new();

    while let Ok(msg) = rx.recv() {
        match msg {
            ClipperMsg::ExportProgress { fraction, message } => {
                // Keep the last few real ffmpeg stderr lines for
                // diagnostics; skip the noisy per-frame progress lines.
                if !message.starts_with("frame=") {
                    tail.push_back(message);
                    while tail.len() > TAIL_LINES {
                        tail.pop_front();
                    }
                }
                if let Some(fraction) = fraction
                    && interactive
                {
                    draw_progress(fraction);
                }
            }
            ClipperMsg::ExportFinished { outcome } => {
                if interactive {
                    println!();
                }
                let code = match &outcome {
                    ExportOutcome::Success(_) => {
                        println!("{}", outcome.message());
                        0
                    }
                    ExportOutcome::Canceled => {
                        eprintln!("{}", outcome.message());
                        130
                    }
                    ExportOutcome::Failed(_) => {
                        eprintln!("{}", outcome.message());
                        if !tail.is_empty() {
                            eprintln!("last ffmpeg output:");
                            for line in &tail {
                                eprintln!("  {line}");
                            }
                        }
                        1
                    }
                };
                return code;
            }
            _ => {}
        }
    }

    eprintln!("error: export worker stopped unexpectedly");
    1
}

fn draw_progress(fraction: f32) {
    let fraction = fraction.clamp(0.0, 1.0);
    let filled = (fraction * BAR_WIDTH as f32).round() as usize;
    let bar: String = std::iter::repeat_n('#', filled)
        .chain(std::iter::repeat_n('-', BAR_WIDTH - filled))
        .collect();
    let pct = (fraction * 100.0).round() as i32;
    print!("\r  {pct:3}% [{bar}]");
    let _ = std::io::stdout().flush();
}

fn default_speed(codec: VideoCodec, caps: &ffmpeg_util::FfmpegCaps) -> EncoderSpeed {
    let info = caps.encoder_info(codec.encoder());
    match codec {
        VideoCodec::H264 | VideoCodec::H265 => EncoderSpeed::Named("medium".to_string()),
        VideoCodec::Av1 => EncoderSpeed::Numeric(
            info.and_then(|i| i.preset_default)
                .filter(|n| *n >= 0)
                .unwrap_or(6),
        ),
        VideoCodec::Vp9 => EncoderSpeed::Numeric(
            info.and_then(|i| i.cpu_used_default)
                .filter(|n| *n >= 0)
                .unwrap_or(1),
        ),
    }
}

fn speed_label(speed: &EncoderSpeed) -> String {
    match speed {
        EncoderSpeed::Named(preset) => format!("preset {preset}"),
        EncoderSpeed::Numeric(n) => format!("speed {n}"),
    }
}

fn container_from_extension(path: &Path) -> Option<Container> {
    let ext = path.extension()?.to_str()?.to_ascii_lowercase();
    match ext.as_str() {
        "mp4" | "m4v" => Some(Container::Mp4),
        "mkv" => Some(Container::Mkv),
        "mov" => Some(Container::Mov),
        "webm" => Some(Container::WebM),
        _ => None,
    }
}

#[derive(Parser)]
#[command(
    name = "ChopMedia",
    version,
    about = "Trim, crop, and export video clips",
    long_about = "Trim, crop, and export video clips.\n\nLaunched with no terminal attached (double-clicked), the GUI opens. Launched from a terminal, the CLI runs. Use `ChopMedia gui` to open the GUI from a terminal.",
    after_help = "EXAMPLES:\n    ChopMedia export in.mp4 -o cut.mp4 --start 2 --end 6\n    ChopMedia export in.mp4 -o clip.mp4 --crop 640:360:100:50 \\\n        --codec h264 --crf 20 --speed veryfast --audio aac:192"
)]
struct Cli {
    #[command(subcommand)]
    command: Option<CliCommand>,
}

#[derive(Subcommand)]
enum CliCommand {
    /// Open the GUI from a terminal.
    Gui,
    /// Print video information.
    Info {
        /// Input video file.
        input: PathBuf,
    },
    /// Print probed ffmpeg capabilities.
    Caps,
    /// Export a clip without opening the GUI.
    Export(ExportArgs),
}

#[derive(Args)]
#[command(
    after_help = "TIME accepts seconds or hh:mm:ss. CROP uses W:H:X:Y.\n\nCrop, scale, and FPS changes force re-encoding."
)]
struct ExportArgs {
    /// Input video file.
    input: PathBuf,
    /// Output file path.
    #[arg(short, long, value_name = "PATH")]
    output: PathBuf,
    /// Clip start: seconds or hh:mm:ss (default: 0).
    #[arg(short, long, value_name = "TIME", value_parser = parse_time)]
    start: Option<f64>,
    /// Clip end: seconds or hh:mm:ss (default: full duration).
    #[arg(short, long, value_name = "TIME", value_parser = parse_time)]
    end: Option<f64>,
    /// Crop rectangle in input pixels: W:H:X:Y.
    #[arg(long, value_name = "W:H:X:Y", value_parser = parse_crop)]
    crop: Option<CropPixels>,
    /// Output container: mp4, mkv, mov, or webm.
    #[arg(long, value_name = "FMT", value_parser = parse_container)]
    container: Option<Container>,
    /// Export mode: copy or reencode.
    #[arg(
        long,
        value_name = "MODE",
        default_value = "copy",
        value_parser = parse_mode
    )]
    mode: ExportMode,
    /// Video codec: h264, h265, vp9, or av1.
    #[arg(long, value_name = "CODEC", value_parser = parse_codec)]
    codec: Option<VideoCodec>,
    /// Constant quality value; lower is better.
    #[arg(long, value_name = "N")]
    crf: Option<u8>,
    /// Encoder preset name or numeric speed.
    #[arg(long, value_name = "SPEED", value_parser = parse_speed)]
    speed: Option<EncoderSpeed>,
    /// Audio mode: copy, none, aac[:KBPS], or opus[:KBPS].
    #[arg(long, value_name = "MODE", value_parser = parse_audio)]
    audio: Option<AudioMode>,
    /// Resolution scale factor, for example 0.5.
    #[arg(long, value_name = "FACTOR", value_parser = parse_positive_f32)]
    scale: Option<f32>,
    /// Output frame rate override.
    #[arg(long, value_name = "FPS", value_parser = parse_positive_f64)]
    fps: Option<f64>,
}

#[derive(Debug)]
struct ArgumentParseError(String);

impl std::fmt::Display for ArgumentParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for ArgumentParseError {}

type ArgumentParseResult<T> = Result<T, ArgumentParseError>;

fn invalid_value(message: impl Into<String>) -> ArgumentParseError {
    ArgumentParseError(message.into())
}

fn parse_time(text: &str) -> ArgumentParseResult<f64> {
    timecode_to_seconds(text)
        .filter(|seconds| seconds.is_finite())
        .ok_or_else(|| {
            invalid_value(format!(
                "invalid time {text:?}: expected seconds or hh:mm:ss"
            ))
        })
}

fn parse_crop(text: &str) -> ArgumentParseResult<CropPixels> {
    let nums: Vec<i64> = text
        .split(':')
        .map(|part| part.parse::<i64>())
        .collect::<Result<_, _>>()
        .map_err(|_| invalid_value(format!("invalid crop {text:?}: expected W:H:X:Y")))?;
    let [w, h, x, y] = nums.as_slice() else {
        return Err(invalid_value(format!(
            "invalid crop {text:?}: expected W:H:X:Y"
        )));
    };
    if *w <= 0 || *h <= 0 {
        return Err(invalid_value(format!(
            "crop size must be positive (got {w}x{h})"
        )));
    }
    Ok(CropPixels {
        w: *w,
        h: *h,
        x: *x,
        y: *y,
    })
}

fn parse_container(text: &str) -> ArgumentParseResult<Container> {
    match text.to_ascii_lowercase().as_str() {
        "mp4" => Ok(Container::Mp4),
        "mkv" | "matroska" => Ok(Container::Mkv),
        "mov" => Ok(Container::Mov),
        "webm" => Ok(Container::WebM),
        other => Err(invalid_value(format!(
            "unknown container {other:?} (expected mp4, mkv, mov, or webm)"
        ))),
    }
}

fn parse_mode(text: &str) -> ArgumentParseResult<ExportMode> {
    match text.to_ascii_lowercase().as_str() {
        "copy" | "streamcopy" => Ok(ExportMode::StreamCopy),
        "reencode" | "encode" => Ok(ExportMode::Reencode),
        other => Err(invalid_value(format!(
            "unknown mode {other:?} (expected copy or reencode)"
        ))),
    }
}

fn parse_codec(text: &str) -> ArgumentParseResult<VideoCodec> {
    match text.to_ascii_lowercase().as_str() {
        "h264" | "avc" => Ok(VideoCodec::H264),
        "h265" | "hevc" => Ok(VideoCodec::H265),
        "vp9" => Ok(VideoCodec::Vp9),
        "av1" => Ok(VideoCodec::Av1),
        other => Err(invalid_value(format!(
            "unknown codec {other:?} (expected h264, h265, vp9, or av1)"
        ))),
    }
}

fn parse_speed(text: &str) -> ArgumentParseResult<EncoderSpeed> {
    Ok(match text.parse::<i64>() {
        Ok(value) => EncoderSpeed::Numeric(value),
        Err(_) => EncoderSpeed::Named(text.to_string()),
    })
}

fn parse_audio(text: &str) -> ArgumentParseResult<AudioMode> {
    let (kind, bitrate) = match text.split_once(':') {
        Some((kind, bitrate)) => (kind, Some(bitrate)),
        None => (text, None),
    };
    match kind.to_ascii_lowercase().as_str() {
        "copy" => Ok(AudioMode::Copy),
        "none" | "no" | "off" => Ok(AudioMode::Remove),
        "aac" => {
            let kbps = parse_audio_bitrate(text, bitrate, 128)?;
            Ok(AudioMode::Aac(kbps))
        }
        "opus" => {
            let kbps = parse_audio_bitrate(text, bitrate, 96)?;
            Ok(AudioMode::Opus(kbps))
        }
        other => Err(invalid_value(format!(
            "unknown audio mode {other:?} (expected copy, none, aac[:kbps], or opus[:kbps])"
        ))),
    }
}

fn parse_audio_bitrate(
    text: &str,
    bitrate: Option<&str>,
    default: u32,
) -> ArgumentParseResult<u32> {
    bitrate
        .map(str::parse)
        .transpose()
        .map_err(|_| {
            invalid_value(format!(
                "invalid audio bitrate in {text:?}: expected an integer"
            ))
        })
        .map(|bitrate| bitrate.unwrap_or(default))
}

fn parse_positive_f32(text: &str) -> ArgumentParseResult<f32> {
    let value = text.parse::<f32>().map_err(|_| {
        invalid_value(format!(
            "invalid scale {text:?}: expected a positive number"
        ))
    })?;
    if !value.is_finite() || value <= 0.0 {
        return Err(invalid_value("scale must be a finite positive number"));
    }
    Ok(value)
}

fn parse_positive_f64(text: &str) -> ArgumentParseResult<f64> {
    let value = text
        .parse::<f64>()
        .map_err(|_| invalid_value(format!("invalid FPS {text:?}: expected a positive number")))?;
    if !value.is_finite() || value <= 0.0 {
        return Err(invalid_value("FPS must be a finite positive number"));
    }
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_export_options_and_compatibility_aliases() {
        let cli = Cli::try_parse_from([
            "ChopMedia",
            "export",
            "input.mp4",
            "--output",
            "clip.mkv",
            "--start",
            "01:01.5",
            "--end",
            "02:00",
            "--crop",
            "640:360:100:50",
            "--container",
            "matroska",
            "--mode",
            "streamcopy",
            "--codec",
            "avc",
            "--crf",
            "23",
            "--speed",
            "4",
            "--audio",
            "aac:192",
            "--scale",
            "0.5",
            "--fps",
            "29.97",
        ])
        .expect("arguments should parse");

        let Some(CliCommand::Export(args)) = cli.command else {
            panic!("expected export command");
        };
        assert_eq!(args.input, PathBuf::from("input.mp4"));
        assert_eq!(args.output, PathBuf::from("clip.mkv"));
        assert_eq!(args.start, Some(61.5));
        assert_eq!(args.end, Some(120.0));
        assert!(matches!(args.container, Some(Container::Mkv)));
        assert!(matches!(args.mode, ExportMode::StreamCopy));
        assert!(matches!(args.codec, Some(VideoCodec::H264)));
        assert_eq!(args.crf, Some(23));
        assert!(matches!(args.speed, Some(EncoderSpeed::Numeric(4))));
        assert!(matches!(args.audio, Some(AudioMode::Aac(192))));
        assert_eq!(args.scale, Some(0.5));
        assert_eq!(args.fps, Some(29.97));

        let crop = args.crop.expect("crop should parse");
        assert_eq!((crop.w, crop.h, crop.x, crop.y), (640, 360, 100, 50));
    }

    #[test]
    fn requires_an_export_output_path() {
        assert!(Cli::try_parse_from(["ChopMedia", "export", "input.mp4"]).is_err());
    }

    #[test]
    fn rejects_non_positive_numeric_export_options() {
        assert!(
            Cli::try_parse_from([
                "ChopMedia",
                "export",
                "input.mp4",
                "--output",
                "clip.mp4",
                "--scale",
                "0",
            ])
            .is_err()
        );
        assert!(
            Cli::try_parse_from([
                "ChopMedia",
                "export",
                "input.mp4",
                "--output",
                "clip.mp4",
                "--fps",
                "NaN",
            ])
            .is_err()
        );
    }
}
