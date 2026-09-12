//! Shared helpers for locating and invoking `ffmpeg`/`ffprobe`.

use std::collections::HashMap;
use std::process::{Command, Stdio};

#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// Creates a [`Command`] for the given program, configured to not pop up a
/// console window on Windows.
pub fn command(program: &str) -> Command {
    let mut cmd = Command::new(program);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    cmd
}

/// Checks whether both `ffmpeg` and `ffprobe` are reachable on `PATH`.
///
/// Returns `Ok(())` if both are found, otherwise an error describing which
/// tool(s) are missing.
pub fn check_available() -> Result<(), String> {
    let mut missing = Vec::new();
    for tool in ["ffmpeg", "ffprobe"] {
        let ok = command(tool)
            .arg("-version")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_ok();
        if !ok {
            missing.push(tool);
        }
    }

    if missing.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "{} not found on PATH. Please install FFmpeg (https://ffmpeg.org/) and make sure `{}` are available in your PATH.",
            missing.join(" and "),
            missing.join(", ")
        ))
    }
}

/// Detailed options of a single encoder, parsed from
/// `ffmpeg -h encoder=<name>`. All fields are best-effort; callers fall
/// back to sensible static defaults when empty.
#[derive(Clone, Default)]
pub struct EncoderInfo {
    /// Named choices of the `-preset` option (x264/x265), from the
    /// `...value:` continuation line. Empty for numeric presets.
    pub presets: Vec<String>,
    /// Numeric range of `-preset` (SVT-AV1).
    pub preset_range: Option<(i64, i64)>,
    /// ffmpeg's default numeric `-preset` value, if printed.
    pub preset_default: Option<i64>,
    /// Numeric range of `-cpu-used` (VP9).
    pub cpu_used_range: Option<(i64, i64)>,
    /// ffmpeg's default `-cpu-used` value, if printed.
    pub cpu_used_default: Option<i64>,
    /// Numeric range of `-crf`.
    pub crf_range: Option<(f64, f64)>,
}

/// Capabilities of the installed ffmpeg build, probed once at startup by
/// listing its encoders and muxers (and the option help of the encoders
/// we care about).
#[derive(Clone, Default)]
pub struct FfmpegCaps {
    /// False when probing failed (e.g. ffmpeg missing); capability checks
    /// are permissive in that case so every option stays available.
    probed: bool,
    encoders: Vec<String>,
    muxers: Vec<String>,
    encoder_info: HashMap<String, EncoderInfo>,
}

/// The video encoders whose option help gets probed for presets/CRF ranges.
const PROBED_VIDEO_ENCODERS: [&str; 4] = ["libx264", "libx265", "libvpx-vp9", "libsvtav1"];

impl FfmpegCaps {
    pub fn was_probed(&self) -> bool {
        self.probed
    }

    /// All encoder names listed by the ffmpeg build (empty if not probed).
    pub fn encoders(&self) -> &[String] {
        &self.encoders
    }

    /// All muxer names listed by the ffmpeg build (empty if not probed).
    pub fn muxers(&self) -> &[String] {
        &self.muxers
    }

    pub fn has_encoder(&self, name: &str) -> bool {
        !self.probed || self.encoders.iter().any(|e| e == name)
    }

    pub fn has_muxer(&self, name: &str) -> bool {
        !self.probed || self.muxers.iter().any(|m| m == name)
    }

    pub fn encoder_info(&self, name: &str) -> Option<&EncoderInfo> {
        self.encoder_info.get(name)
    }
}

/// Runs `ffmpeg -encoders` / `-muxers` and parses the available names,
/// then probes the option help of the relevant video encoders.
pub fn probe_caps() -> FfmpegCaps {
    let encoders = run_listing(&["-hide_banner", "-encoders"]);
    let muxers = run_listing(&["-hide_banner", "-muxers"]);
    match (encoders, muxers) {
        (Ok(e), Ok(m)) => {
            let mut caps = FfmpegCaps {
                probed: true,
                encoders: parse_encoder_names(&e),
                muxers: parse_muxer_names(&m),
                encoder_info: HashMap::new(),
            };
            for name in PROBED_VIDEO_ENCODERS {
                if !caps.encoders.iter().any(|n| n == name) {
                    continue;
                }
                let help_arg = format!("encoder={name}");
                if let Ok(help) = run_listing(&["-hide_banner", "-h", help_arg.as_str()]) {
                    caps.encoder_info
                        .insert(name.to_string(), parse_encoder_info(&help));
                }
            }
            caps
        }
        _ => FfmpegCaps::default(),
    }
}

fn run_listing(args: &[&str]) -> std::io::Result<String> {
    let out = command("ffmpeg")
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()?;
    // ffmpeg prints these listings to stdout on current builds, but older
    // builds printed them to stderr - accept either.
    let stdout = String::from_utf8_lossy(&out.stdout);
    Ok(if stdout.trim().is_empty() {
        String::from_utf8_lossy(&out.stderr).into_owned()
    } else {
        stdout.into_owned()
    })
}

/// Parses `ffmpeg -encoders` output; lines look like
/// ` V....D libx264   libx264 H.264 / AVC / ...`.
fn parse_encoder_names(output: &str) -> Vec<String> {
    output
        .lines()
        .filter_map(|line| {
            let mut tokens = line.split_whitespace();
            let flags = tokens.next()?;
            let name = tokens.next()?;
            let is_flag_col =
                flags.len() == 6 && flags.chars().all(|c| c.is_ascii_uppercase() || c == '.');
            if is_flag_col && name != "=" {
                Some(name.to_string())
            } else {
                None
            }
        })
        .collect()
}

/// Parses `ffmpeg -h encoder=<name>` output, extracting the option data
/// the export dialog cares about: named preset choices, numeric preset /
/// cpu-used ranges and defaults, and the CRF range.
fn parse_encoder_info(output: &str) -> EncoderInfo {
    let mut info = EncoderInfo::default();
    let mut current: Option<String> = None;

    for line in output.lines() {
        let trimmed = line.trim_start();
        if let Some(rest) = trimmed.strip_prefix('-') {
            let name: String = rest
                .chars()
                .take_while(|c| c.is_alphanumeric() || *c == '-' || *c == '_')
                .collect();
            if !name.is_empty() {
                current = Some(name.clone());
                match name.as_str() {
                    "preset" | "cpu-used" => {
                        if let Some((lo, hi)) = parse_range(line) {
                            let range = (lo as i64, hi as i64);
                            if name == "preset" {
                                info.preset_range = Some(range);
                            } else {
                                info.cpu_used_range = Some(range);
                            }
                        }
                        if let Some(default) = parse_numeric_default(line) {
                            if name == "preset" {
                                info.preset_default = Some(default);
                            } else {
                                info.cpu_used_default = Some(default);
                            }
                        }
                    }
                    "crf" => {
                        info.crf_range = parse_range(line);
                    }
                    _ => {}
                }
                continue;
            }
        }

        // Continuation line listing the option's named choices, e.g.
        // `     ...value: ultrafast superfast ...`.
        if let Some(idx) = line.find("...value:")
            && current.as_deref() == Some("preset")
        {
            let list = &line[idx + "...value:".len()..];
            info.presets = list
                .split(|c: char| c == ',' || c.is_whitespace())
                .filter(|s| !s.is_empty())
                .map(str::to_string)
                .collect();
        }
    }
    info
}

/// Extracts `(lo, hi)` from an option line's `(from A to B)` suffix.
fn parse_range(line: &str) -> Option<(f64, f64)> {
    let start = line.find("(from ")?;
    let rest = &line[start + 6..];
    let mid = rest.find(" to ")?;
    let lo: f64 = rest[..mid].trim().parse().ok()?;
    let rest = &rest[mid + 4..];
    let end = rest.find(')')?;
    let hi: f64 = rest[..end].trim().parse().ok()?;
    Some((lo, hi))
}

/// Extracts ffmpeg's default from an option line's `(default N)` suffix;
/// returns `None` for non-numeric defaults like `(default: medium)`.
fn parse_numeric_default(line: &str) -> Option<i64> {
    let start = line.find("(default ")?;
    let rest = &line[start + 9..];
    let end = rest.find(')')?;
    rest[..end].trim_end_matches(':').trim().parse().ok()
}

/// Parses `ffmpeg -muxers` output; lines look like ` E mp4   MP4`.
/// The flags column must contain `E` (muxing supported).
fn parse_muxer_names(output: &str) -> Vec<String> {
    output
        .lines()
        .filter_map(|line| {
            let mut tokens = line.split_whitespace();
            let flags = tokens.next()?;
            let name = tokens.next()?;
            let is_flag_col = !flags.is_empty()
                && flags.len() <= 2
                && flags.chars().all(|c| matches!(c, 'D' | 'E'))
                && flags.contains('E');
            if is_flag_col && name != "=" {
                Some(name.to_string())
            } else {
                None
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const ENCODERS_SAMPLE: &str = "Encoders:\n \
 V..... = Video\n \
 A..... = Audio\n \
 ------\n \
 V....D libx264              libx264 H.264 / AVC / MPEG-4 AVC / MPEG-4 part 10 (codec h264)\n \
 V....D libx265              libx265 H.265 / HEVC (codec hevc)\n \
 A....D aac                  AAC (Advanced Audio Coding)\n \
 A....D libopus              libopus Opus (codec opus)\n";

    #[test]
    fn parses_encoder_names() {
        let names = parse_encoder_names(ENCODERS_SAMPLE);
        assert_eq!(names, ["libx264", "libx265", "aac", "libopus"]);
    }

    const MUXERS_SAMPLE: &str = "Muxers:\n \
 D. = Demuxing supported\n \
 .E = Muxing supported\n \
 --\n \
 D  3dostr          3DO STR\n \
  E mp4             MP4 (MPEG-4 Part 14)\n \
 DE matroska        Matroska\n";

    #[test]
    fn parses_muxer_names() {
        let names = parse_muxer_names(MUXERS_SAMPLE);
        assert_eq!(names, ["mp4", "matroska"]);
    }

    const X264_HELP_SAMPLE: &str = "Encoder libx264 [libx264 H.264 / AVC (codec h264)]:\n \
    General capabilities: delay small\n \
libx264 AVOptions:\n \
  -preset            <string>     E. . V. . Set the encoding preset [source] (default: medium)\n \
     ...value: ultrafast  superfast  veryfast  faster  fast  medium  slow  slower  veryslow  placebo\n \
  -tune              <string>     E. . V. . Tune the encoding params [source] (default: film)\n \
     ...value: film animation grain stillimage psnr\n \
  -crf               <float>      E. . V. . Select the quality for constant quality mode (from -1 to 51) (default -1)\n";

    #[test]
    fn parses_encoder_info_x264() {
        let info = parse_encoder_info(X264_HELP_SAMPLE);
        assert_eq!(
            info.presets,
            [
                "ultrafast",
                "superfast",
                "veryfast",
                "faster",
                "fast",
                "medium",
                "slow",
                "slower",
                "veryslow",
                "placebo"
            ]
        );
        assert_eq!(info.crf_range, Some((-1.0, 51.0)));
        assert_eq!(info.preset_range, None);
        assert_eq!(info.preset_default, None);
    }

    const SVTAV1_HELP_SAMPLE: &str = "libsvtav1 AVOptions:\n \
  -preset            <int>        E. . V. . Encoding preset. (from -1 to 13) (default 6)\n \
  -crf               <int>        E. . V. . Constant Rate Factor value (from 0 to 63) (default -1)\n";

    #[test]
    fn parses_encoder_info_svtav1() {
        let info = parse_encoder_info(SVTAV1_HELP_SAMPLE);
        assert_eq!(info.preset_range, Some((-1, 13)));
        assert_eq!(info.preset_default, Some(6));
        assert_eq!(info.crf_range, Some((0.0, 63.0)));
        assert!(info.presets.is_empty());
    }
}
