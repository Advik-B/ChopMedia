# ChopMedia: C#/MonoGame/ImGui → Rust/egui Migration Plan

## 1. Summary of the existing app

`old_code/ChopMedia` is a MonoGame + ImGui.NET desktop app ("ChopMedia") that lets a user:

- Load a video file (`mp4/mkv/mov/avi`) via a native file dialog (NativeFileDialogSharp).
- Decode/seek frames with OpenCvSharp (OpenCV) and show a live preview texture.
- Scrub a timeline with 40 generated thumbnails, a playhead, and draggable start/end trim handles.
- Play/pause a preview with audio (NAudio `MediaFoundationReader` + `WaveOutEvent`), roughly synced to the displayed frame.
- Draw a waveform strip (amplitude envelope) below the timeline.
- Crop via a draggable/resizable overlay rectangle on the preview.
- Enter exact start/end timecodes as text, with keyboard shortcuts: `Space` play/pause, `←/→` step frame, `I`/`O` set start/end at playhead, `Esc` quit.
- Export the trimmed (and optionally cropped) clip by shelling out to `ffmpeg`, either stream-copying (`-c copy`) or re-encoding (`libx264`/`aac`, forced when cropping), with a progress modal parsed from ffmpeg's stderr, and a Cancel button that kills the process.
- Custom dark/blue ImGui visual theme.

`ffmpeg` is already a required external dependency (used for export), and is present on this machine via scoop (`ffmpeg`, `ffprobe` on `PATH`).

## 2. Target architecture

**GUI:** `egui` + `eframe` (immediate-mode, closest possible 1:1 port of the ImGui code — panels, `Painter` drawing, `interact()`-based drag handles map directly to the old ImGui `InvisibleButton`/`IsItemActive`/`GetMouseDragDelta` patterns).

**Video decode:** CLI-based, shelling out to `ffmpeg`/`ffprobe` (already a runtime dependency of this app). No native library linking (no OpenCV/FFmpeg dev headers, no vcpkg/bindgen setup) — matches the user's decision to keep the build simple. Tradeoffs vs. in-process decoding (e.g. `ffmpeg-next`) are called out in §7.

**Audio:** Extract the audio track once per loaded video via `ffmpeg` into an in-memory PCM buffer (via a temp WAV file + `hound`), then play it back with `rodio` using a small custom `Source` that supports instant sample-accurate seeking (needed for scrubbing/looping within the trim range). This avoids requiring Windows Media Foundation codecs and avoids relying on `rodio`'s built-in decoders supporting every possible container/codec.

**File dialogs:** `rfd` (native dialogs on Windows/macOS/Linux), replacing NativeFileDialogSharp.

**Export:** Same approach as today — spawn `ffmpeg` as a child process, stream its stderr, regex-parse `time=` progress, support cancel-by-kill. Ported near-verbatim into Rust.

## 3. Dependencies (Cargo.toml)

| Crate | Purpose | Notes |
|---|---|---|
| `eframe` | App shell / windowing / rendering | default (glow/wgpu) backend |
| `egui` | Immediate-mode UI + `Painter` | pulled in by eframe |
| `rfd` | Native open/save file dialogs | replaces NativeFileDialogSharp |
| `image` | Decode PNG for the window icon | small, already needed |
| `rodio` | Audio output (`OutputStream`/`Sink`) | `default-features = false` — we supply our own `Source`, don't need its format decoders |
| `hound` | Read the WAV produced by `ffmpeg` audio extraction | simple, no native deps |
| `serde` / `serde_json` | Parse `ffprobe -of json` output | |
| `regex` | Parse `time=hh:mm:ss.cc` from ffmpeg export stderr | matches old code's approach |
| `anyhow` | Ergonomic error handling in background threads | |
| `dirs` | Resolve the platform "Videos" folder for dialog default dir | |

No async runtime (tokio) is needed — background work uses plain `std::thread` + `std::sync::mpsc`, which is simpler and sufficient here (mirrors the old `Task.Run` + `ConcurrentQueue<Action>` pattern).

## 4. Module layout

```
src/
  main.rs                 – eframe::run_native setup, icon, fonts
  app.rs                  – ChopMediaApp (eframe::App impl), global keyboard shortcuts, Esc-to-quit
  theme.rs                – custom egui::Visuals (dark/blue), ported from ApplyCustomStyle
  timecode.rs             – seconds <-> "hh:mm:ss.fff" helpers
  ffmpeg_util.rs          – shared helpers: locate ffmpeg/ffprobe on PATH, run + capture, error type
  video/
    mod.rs                – VideoInfo (probe result), VideoHandle (loaded-video state/lifecycle)
    probe.rs               – ffprobe wrapper -> VideoInfo
    frame_extract.rs       – single-frame extraction ("mailbox" latest-request pattern), used for scrubbing + thumbnails
    playback.rs            – continuous ffmpeg raw-frame pipe + frame-pump thread, used during Play
  audio/
    mod.rs                 – extraction (ffmpeg -> temp wav -> hound -> Vec<f32>), waveform bucketing
    source.rs              – PcmSource: custom rodio::Source over the in-memory buffer with atomic seek position
  export.rs                – argument building + child process + stderr progress parsing (ExportClip/RunExport port)
  clipper/
    mod.rs                 – VideoClipper state struct + top-level layout (DrawUI port)
    controls.rs             – right-hand control panel (DrawControls port)
    preview.rs               – video preview + crop overlay (DrawVideoPreview/DrawCropOverlay/ClampCropRect port)
    timeline.rs               – timeline + thumbnails + handles (DrawTimeline port)
    waveform.rs                – waveform strip (DrawWaveform port)
    export_dialog.rs            – export progress modal (DrawExportPopup port)
assets/
  fonts/Inter-SemiBold.ttf   – copied from old_code (only weight actually used)
  icons/App.png              – copied from old_code, decoded at startup for the window icon
```

## 5. Feature-by-feature mapping

- **Layout** — old code manually computes pixel heights/columns inside one big ImGui window. New code uses idiomatic egui panels: `SidePanel::right` (controls, 320px), `TopBottomPanel::bottom` ×2 (timeline 80px, waveform 60px), `CentralPanel` (video preview). Same visual arrangement, cleaner code.
- **Video preview texture** — `Texture2D`/`BindTexture` → `egui::TextureHandle` created via `ctx.load_texture`, updated in place via `TextureHandle::set(ColorImage, ..)` each time a new frame arrives (no manual bind/unbind bookkeeping needed).
- **Thumbnails (40)** — instead of decoding via OpenCV per-thumbnail, each is extracted with a single `ffmpeg -ss <t> -i in -frames:v 1 -vf scale=160:90 -f rawvideo -pix_fmt rgba -` call. A small fixed-size thread pool (a handful of `std::thread`s, no extra crate) processes the 40 requests concurrently and reports progress through an `mpsc` channel — same UX as today's progress bar/status text.
- **Timeline / crop overlay / handles** — direct port using `egui::Painter` (`rect_filled`, `rect_stroke`, `line_segment`, `circle_filled`) for drawing, and `ui.interact(rect, id, Sense::drag())` + `response.drag_delta()` for the invisible drag regions — a very close analogue of ImGui's `InvisibleButton`/`GetMouseDragDelta`.
- **Waveform** — extracted once from the ffmpeg-decoded WAV (see audio below) instead of NAudio; drawn identically with `Painter::line_segment`.
- **Trim start/end text fields** — `egui::TextEdit::singleline` bound to the timecode strings, same parse-on-edit behavior (`UpdateFromText`/`UpdateTimestamps` ports 1:1).
- **Keyboard shortcuts** — `ctx.input(|i| i.key_pressed(Key::Space/ArrowLeft/ArrowRight))`, `I`/`O`, guarded by `!ctx.wants_keyboard_input()` (direct analogue of `io.WantCaptureKeyboard`). `Esc` closes the app via `ctx.send_viewport_cmd(ViewportCommand::Close)`. Gamepad "Back" button support is dropped (Xbox-controller-specific nicety, not essential for a desktop tool) — flagging this as an intentional scope cut.
- **Play/Pause** — see §6 (Audio & playback) below; behavior-equivalent, but internally cleaner (see below).
- **Export** — `export.rs` ports `ExportClip`/`RunExport` argument-building logic verbatim (fast-seek vs. accurate-seek ordering, `-t` vs `-to`, `-vf crop=...`, `-c copy` vs `-c:v libx264 -c:a aac`), spawns `ffmpeg` with piped stderr read on a background thread, regex-parses `time=` for progress, and supports Cancel by killing the child process. The progress modal (`export_dialog.rs`) is a straightforward `egui::Window`/modal port of `DrawExportPopup`.
- **Custom theme** — `theme.rs` maps the old color palette (window/frame/button backgrounds, hover/active blue accent, checkmark/slider color, rounding) onto the closest `egui::Visuals` fields. egui groups colors by widget-state (`noninteractive`/`inactive`/`hovered`/`active`) rather than ImGui's flat per-role array, so this won't be byte-identical, but will preserve the same dark background + blue accent look.
- **File dialogs** — `rfd::FileDialog` with video extension filters, defaulting to `dirs::video_dir()`, replacing `Dialog.FileOpen`/`Dialog.FileSave`.
- **Main-thread action queue** — the old `ConcurrentQueue<Action>` pattern becomes `std::sync::mpsc::channel::<ClipperMsg>()`, drained once per `update()` call, where `ClipperMsg` is an enum (`FrameReady`, `ThumbnailReady{index, image}`, `AudioReady{...}`, `ExportProgress{..}`, `ExportFinished{..}`, etc.).

## 6. Audio & playback design (biggest behavioral change vs. old code)

On `LoadVideo`:
1. `ffprobe` runs synchronously (fast, <1s) to populate `VideoInfo` (fps, frame count — falling back to `round(duration * fps)` if the container doesn't report `nb_frames`, e.g. some `.mkv` files; this is a small robustness improvement over relying solely on OpenCV's frame-count property).
2. Thumbnail generation and audio extraction are kicked off on background threads; the UI shows a status message/progress bar immediately, exactly as before.
3. Audio extraction: `ffmpeg -y -i <path> -vn -ac 2 -ar 44100 -f wav <temp.wav>` into a temp file (path built from `std::env::temp_dir()` + a unique name; no `tempfile` crate needed). If the video has no audio track (or ffmpeg fails), we treat it as silent — same fallback behavior as the old code's try/catch around `MediaFoundationReader`.
4. The WAV is parsed with `hound` into an in-memory `Vec<f32>` (interleaved), which is used for **both**:
   - Waveform bucketing (mono-mixed amplitude envelope, same algorithm as `GenerateWaveform`), and
   - Playback, via a custom `rodio::Source` (`audio/source.rs`) wrapping the same buffer with an `Arc<AtomicUsize>` sample-position cursor. This gives **instant, sample-accurate seeking** (just write the atomic) instead of relying on a decoder's seek support — needed for scrubbing and for looping within `[start, end]`.

During playback, the shared atomic sample position becomes the single source of truth for "where we are": a frame-pump thread reads it every loop iteration, converts it to a target video frame (`position / sample_rate * fps`), and requests that frame from the video pipeline (see below) — replacing the old `Stopwatch`-based clock. If a video has no audio, playback falls back to an `Instant`-based clock exactly like the old code's `Stopwatch`.

**Video frame delivery has two modes**, addressing the main weakness of a CLI-only decoder (process-spawn overhead per frame):
- **Scrubbing (paused)** — a single dedicated "mailbox" thread holds only the *latest* requested frame/timestamp (via `Mutex<Option<i64>> + Condvar`, no extra crate); dragging the timeline/handles rapidly only ever triggers the most recent request, discarding stale ones, and extracts via one-shot `ffmpeg -ss <t> -frames:v 1 ...` calls (fast keyframe-ish seek, matching the old code's own seek precision).
- **Playing** — a single long-lived `ffmpeg -ss start -i in -t (end-start) -f rawvideo -pix_fmt rgba -vf scale=w:h -` process streams raw frames sequentially over one pipe; a reader thread pulls fixed-size chunks and pushes them through a small bounded channel to the UI thread, which displays them paced against the audio-position clock (dropping/waiting as needed). This avoids per-frame process-spawn cost during real-time playback and gives noticeably smoother preview than naive "spawn ffmpeg per frame". The process is killed on pause/seek/stop.

This is a deliberate improvement over a literal port (which would spawn-per-frame during playback) while staying entirely within the CLI-only constraint.

**Known limitation to flag explicitly:** frame seeking via `ffmpeg -ss` before decode is fast but keyframe-granular, not always exact-frame; this is comparable to (arguably better than) the old app's own precision (which re-opened a fresh `VideoCapture` per seek). Sample-accurate export is unaffected — export always re-decodes precisely when frame-accurate mode is on, same as before.

## 7. Explicitly out of scope / intentional differences

- No native OpenCV/FFmpeg-dev-library linking (per your choice) — `ffmpeg.exe`/`ffprobe.exe` must be present on `PATH` at runtime, same as the old app's export path already required. We add a startup check that surfaces a clear status-bar error if either is missing, instead of only failing at export time.
- Gamepad "Back to quit" is dropped; `Esc` remains.
- The old app's dark/blue ImGui theme is approximated in egui's theming model, not byte-identical.
- `old_code/` is left untouched during migration; we can delete or archive it once you're happy with the new app.

## 8. Implementation phases (each independently buildable/testable)

1. **Scaffold** — Cargo deps, copy font/icon assets, empty eframe window with theme + font + icon wired up, boots successfully.
2. **Load + probe + single-frame preview** — `rfd` open dialog, `ffprobe` wrapper, single-frame extraction, preview panel shows frame 0.
3. **Timeline + thumbnails** — thumbnail thread pool, timeline widget with handles/playhead/click-seek.
4. **Trim controls + keyboard shortcuts** — timecode text fields, `I`/`O`/`Space`/arrows/`Esc`.
5. **Crop overlay** — draggable/resizable rect, reset button, enable checkbox, clamped.
6. **Audio + waveform + playback** — ffmpeg audio extraction, waveform draw, rodio `PcmSource`, Play/Pause wired to the two-mode frame delivery, volume slider.
7. **Export** — argument building, background process, progress modal, cancel.
8. **Polish** — missing-ffmpeg error UX, temp file cleanup, resizing, final visual pass.

## 9. Validation

- `cargo check`/`cargo build` after each phase.
- Manual smoke test after each phase (this is a GUI app; there's no automated UI test harness planned). I'll launch it briefly via terminal to confirm it starts without panicking, but real verification of look/feel and interaction (drag handles, playback smoothness, export correctness) will need you to try it.
- No unit tests are planned for the UI itself; `timecode.rs` (seconds↔timecode) and the ffprobe JSON parsing are small, pure functions worth a couple of unit tests.
