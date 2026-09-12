//! Continuous frame streaming used during real-time playback.
//!
//! A single long-lived `ffmpeg` process streams raw RGBA frames sequentially
//! over one pipe (much cheaper than spawning a process per frame). A reader
//! thread paces delivery against a [`PlaybackClock`] and publishes the
//! latest decoded frame through a single-slot "mailbox" - the UI only ever
//! cares about the most recent frame, never a backlog.

use std::io::Read;
use std::path::PathBuf;
use std::process::{Child, Stdio};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use anyhow::Context;

use crate::ffmpeg_util;

/// The clock used to pace frame delivery to "real time", expressed as the
/// current absolute playback timestamp (in seconds) within the source
/// video.
pub enum PlaybackClock {
    /// Driven by the audio playback cursor (sample-accurate).
    Audio {
        position: Arc<AtomicUsize>,
        sample_rate: u32,
    },
    /// Wall-clock fallback for videos with no audio track.
    Wall {
        start_instant: Instant,
        start_secs: f64,
    },
}

impl PlaybackClock {
    pub fn wall(start_secs: f64) -> Self {
        PlaybackClock::Wall {
            start_instant: Instant::now(),
            start_secs,
        }
    }

    fn current_secs(&self) -> f64 {
        match self {
            PlaybackClock::Audio {
                position,
                sample_rate,
            } => position.load(Ordering::Relaxed) as f64 / *sample_rate as f64,
            PlaybackClock::Wall {
                start_instant,
                start_secs,
            } => start_secs + start_instant.elapsed().as_secs_f64(),
        }
    }
}

type SharedChild = Arc<Mutex<Option<Child>>>;
type FrameMailbox = Arc<Mutex<Option<(i64, Vec<u8>)>>>;

fn kill_child(child: &SharedChild) {
    if let Some(mut c) = child.lock().unwrap().take() {
        let _ = c.kill();
        let _ = c.wait();
    }
}

/// Manages one continuous-playback ffmpeg pipe.
pub struct PlaybackController {
    mailbox: FrameMailbox,
    stop: Arc<AtomicBool>,
    finished: Arc<AtomicBool>,
    child: SharedChild,
}

impl PlaybackController {
    pub fn start(
        path: PathBuf,
        start_frame: i64,
        end_frame: i64,
        fps: f64,
        native_size: (u32, u32),
        clock: PlaybackClock,
    ) -> anyhow::Result<Self> {
        let (width, height) = native_size;
        let frame_size = width as usize * height as usize * 4;
        let start_secs = start_frame as f64 / fps.max(0.001);
        let frame_count = (end_frame - start_frame + 1).max(1);

        let mut child = ffmpeg_util::command("ffmpeg")
            .args(["-v", "error", "-ss", &format!("{start_secs:.3}"), "-i"])
            .arg(&path)
            .args([
                "-frames:v",
                &frame_count.to_string(),
                "-f",
                "rawvideo",
                "-pix_fmt",
                "rgba",
                "-",
            ])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .context("failed to spawn ffmpeg for playback")?;

        let mut stdout = child.stdout.take().context("ffmpeg produced no stdout")?;

        let mailbox = Arc::new(Mutex::new(None));
        let stop = Arc::new(AtomicBool::new(false));
        let finished = Arc::new(AtomicBool::new(false));
        let child_handle = Arc::new(Mutex::new(Some(child)));

        let mailbox_thread = mailbox.clone();
        let stop_thread = stop.clone();
        let finished_thread = finished.clone();
        let child_thread = child_handle.clone();

        thread::spawn(move || {
            let mut buf = vec![0u8; frame_size];
            let mut frame_number = start_frame;

            'frames: loop {
                if stop_thread.load(Ordering::Relaxed) {
                    break;
                }
                if stdout.read_exact(&mut buf).is_err() {
                    break; // EOF or the process was killed.
                }

                // Pace delivery against the clock.
                loop {
                    if stop_thread.load(Ordering::Relaxed) {
                        break 'frames;
                    }
                    let target = frame_number as f64 / fps.max(0.001);
                    if clock.current_secs() >= target {
                        break;
                    }
                    thread::sleep(Duration::from_millis(2));
                }

                *mailbox_thread.lock().unwrap() = Some((frame_number, buf.clone()));

                frame_number += 1;
                if frame_number > end_frame {
                    break;
                }
            }

            finished_thread.store(true, Ordering::Relaxed);
            kill_child(&child_thread);
        });

        Ok(Self {
            mailbox,
            stop,
            finished,
            child: child_handle,
        })
    }

    /// Takes the most recently decoded frame, if a new one has arrived
    /// since the last call.
    pub fn take_latest(&self) -> Option<(i64, Vec<u8>)> {
        self.mailbox.lock().unwrap().take()
    }

    /// True once playback has reached the end frame or the pipe closed.
    pub fn is_finished(&self) -> bool {
        self.finished.load(Ordering::Relaxed)
    }
}

impl Drop for PlaybackController {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        kill_child(&self.child);
    }
}
