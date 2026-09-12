//! Single-frame extraction via `ffmpeg`, plus two higher-level helpers built
//! on top of it:
//!
//! - [`PreviewFetcher`]: a "mailbox" background thread used while scrubbing.
//!   Only the *latest* requested frame is ever decoded; stale requests made
//!   while a previous one is still in flight are simply dropped.
//! - [`spawn_thumbnail_generation`]: a small fixed-size worker pool that
//!   decodes a batch of evenly-spaced thumbnails concurrently.

use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::Sender;
use std::sync::{Arc, Condvar, Mutex};
use std::thread;

use anyhow::Context;

use crate::ffmpeg_util;
use crate::messages::ClipperMsg;

/// Extracts a single frame at `timestamp_secs` as raw RGBA bytes.
///
/// If `scale` is given, the frame is resized by ffmpeg to that size;
/// otherwise it's returned at `native_size`. The returned buffer is always
/// exactly `width * height * 4` bytes.
pub fn extract_frame_rgba(
    path: &Path,
    timestamp_secs: f64,
    native_size: (u32, u32),
    scale: Option<(u32, u32)>,
) -> anyhow::Result<Vec<u8>> {
    let (out_w, out_h) = scale.unwrap_or(native_size);
    let ts = format!("{:.3}", timestamp_secs.max(0.0));

    let mut cmd = ffmpeg_util::command("ffmpeg");
    cmd.args(["-v", "error", "-ss", &ts, "-i"])
        .arg(path)
        .args(["-frames:v", "1"]);

    if let Some((w, h)) = scale {
        cmd.args(["-vf", &format!("scale={w}:{h}")]);
    }

    cmd.args(["-f", "rawvideo", "-pix_fmt", "rgba", "-"])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());

    let output = cmd.output().context("failed to spawn ffmpeg")?;

    let expected_len = out_w as usize * out_h as usize * 4;
    if output.stdout.len() < expected_len {
        anyhow::bail!(
            "ffmpeg produced {} bytes, expected {expected_len}",
            output.stdout.len()
        );
    }

    let mut data = output.stdout;
    data.truncate(expected_len);
    Ok(data)
}

struct Wanted {
    frame_index: i64,
    timestamp_secs: f64,
}

struct Inner {
    wanted: Option<Wanted>,
    stop: bool,
}

/// Background "mailbox" fetcher for scrubbing: cheap to call `request()`
/// repeatedly (e.g. every UI frame while dragging), only the most recent
/// request actually gets decoded.
pub struct PreviewFetcher {
    shared: Arc<(Mutex<Inner>, Condvar)>,
}

impl PreviewFetcher {
    pub fn new(path: PathBuf, native_size: (u32, u32), sender: Sender<ClipperMsg>) -> Self {
        let shared = Arc::new((
            Mutex::new(Inner {
                wanted: None,
                stop: false,
            }),
            Condvar::new(),
        ));
        let shared_thread = shared.clone();

        thread::spawn(move || {
            loop {
                let wanted = {
                    let (lock, cvar) = &*shared_thread;
                    let mut guard = lock.lock().unwrap();
                    while guard.wanted.is_none() && !guard.stop {
                        guard = cvar.wait(guard).unwrap();
                    }
                    if guard.stop {
                        return;
                    }
                    guard.wanted.take().unwrap()
                };

                if let Ok(rgba) =
                    extract_frame_rgba(&path, wanted.timestamp_secs, native_size, None)
                {
                    let _ = sender.send(ClipperMsg::FrameReady {
                        frame_index: wanted.frame_index,
                        rgba,
                        width: native_size.0,
                        height: native_size.1,
                    });
                }
            }
        });

        Self { shared }
    }

    /// Requests a frame; supersedes any not-yet-started previous request.
    pub fn request(&self, frame_index: i64, timestamp_secs: f64) {
        let (lock, cvar) = &*self.shared;
        let mut guard = lock.lock().unwrap();
        guard.wanted = Some(Wanted {
            frame_index,
            timestamp_secs,
        });
        cvar.notify_one();
    }
}

impl Drop for PreviewFetcher {
    fn drop(&mut self) {
        let (lock, cvar) = &*self.shared;
        let mut guard = lock.lock().unwrap();
        guard.stop = true;
        cvar.notify_one();
    }
}

const THUMBNAIL_WORKERS: usize = 4;

/// Spawns a small worker pool that decodes `count` evenly-spaced thumbnails
/// across `[0, duration)`. Returns a cancellation flag the caller can set
/// (e.g. when a new video is loaded) to stop in-flight work early.
pub fn spawn_thumbnail_generation(
    path: PathBuf,
    duration: f64,
    count: usize,
    thumb_size: (u32, u32),
    sender: Sender<ClipperMsg>,
) -> Arc<AtomicBool> {
    let cancel = Arc::new(AtomicBool::new(false));
    let queue: Arc<Mutex<VecDeque<usize>>> = Arc::new(Mutex::new((0..count).collect()));
    let completed = Arc::new(AtomicUsize::new(0));

    for _ in 0..THUMBNAIL_WORKERS {
        let path = path.clone();
        let queue = queue.clone();
        let cancel = cancel.clone();
        let completed = completed.clone();
        let sender = sender.clone();

        thread::spawn(move || {
            loop {
                if cancel.load(Ordering::Relaxed) {
                    break;
                }

                let index = {
                    let mut q = queue.lock().unwrap();
                    q.pop_front()
                };
                let Some(index) = index else { break };

                let timestamp = if count > 0 {
                    (index as f64 / count as f64) * duration
                } else {
                    0.0
                };

                let done = completed.fetch_add(1, Ordering::SeqCst) + 1;
                match extract_frame_rgba(&path, timestamp, thumb_size, Some(thumb_size)) {
                    Ok(rgba) => {
                        let _ = sender.send(ClipperMsg::ThumbnailReady {
                            index,
                            rgba,
                            width: thumb_size.0,
                            height: thumb_size.1,
                            done,
                            total: count,
                        });
                    }
                    Err(_) => {
                        let _ = sender.send(ClipperMsg::Status {
                            message: format!("Loading thumbnails... ({done}/{count})"),
                            progress: Some(done as f32 / count.max(1) as f32),
                        });
                    }
                }
            }
        });
    }

    cancel
}
