//! `ffprobe`-based video metadata probing.

use std::path::Path;
use std::process::Stdio;

use anyhow::Context;
use serde::Deserialize;

use crate::ffmpeg_util;

#[derive(Debug, Clone)]
pub struct VideoInfo {
    pub width: u32,
    pub height: u32,
    pub fps: f64,
    pub frame_count: i64,
    pub duration: f64,
}

#[derive(Deserialize)]
struct ProbeOutput {
    #[serde(default)]
    streams: Vec<ProbeStream>,
    format: ProbeFormat,
}

#[derive(Deserialize)]
struct ProbeStream {
    width: Option<u32>,
    height: Option<u32>,
    r_frame_rate: Option<String>,
    avg_frame_rate: Option<String>,
    nb_frames: Option<String>,
}

#[derive(Deserialize)]
struct ProbeFormat {
    duration: Option<String>,
}

pub fn probe(path: &Path) -> anyhow::Result<VideoInfo> {
    let output = ffmpeg_util::command("ffprobe")
        .args([
            "-v",
            "error",
            "-select_streams",
            "v:0",
            "-show_entries",
            "format=duration:stream=width,height,r_frame_rate,avg_frame_rate,nb_frames",
            "-of",
            "json",
        ])
        .arg(path)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .context("failed to spawn ffprobe")?;

    if !output.status.success() {
        anyhow::bail!(
            "ffprobe failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }

    let parsed: ProbeOutput =
        serde_json::from_slice(&output.stdout).context("failed to parse ffprobe output")?;

    let stream = parsed.streams.first().context("no video stream found")?;
    let width = stream.width.context("missing video width")?;
    let height = stream.height.context("missing video height")?;

    let fps = stream
        .r_frame_rate
        .as_deref()
        .and_then(parse_fraction)
        .filter(|f| *f > 0.0)
        .or_else(|| stream.avg_frame_rate.as_deref().and_then(parse_fraction))
        .filter(|f| *f > 0.0)
        .unwrap_or(30.0);

    let duration: f64 = parsed
        .format
        .duration
        .as_deref()
        .and_then(|s| s.parse().ok())
        .unwrap_or(0.0);

    let frame_count = stream
        .nb_frames
        .as_deref()
        .and_then(|s| s.parse::<i64>().ok())
        .filter(|&n| n > 0)
        .unwrap_or_else(|| (duration * fps).round() as i64);

    Ok(VideoInfo {
        width,
        height,
        fps,
        frame_count: frame_count.max(1),
        duration,
    })
}

fn parse_fraction(s: &str) -> Option<f64> {
    let (num, den) = s.split_once('/')?;
    let num: f64 = num.parse().ok()?;
    let den: f64 = den.parse().ok()?;
    if den == 0.0 { None } else { Some(num / den) }
}
