# ChopMedia

A simple video trimmer/clipper with a live preview, waveform, timeline
thumbnails, and crop support. Built with Rust + [egui](https://github.com/emilk/egui)/[eframe](https://github.com/emilk/egui).

This is a Rust rewrite of the original C#/MonoGame/ImGui.NET prototype in
[`old_code/`](old_code); see [`plan.md`](plan.md) for the migration design.

## Requirements

- **`ffmpeg` and `ffprobe` must be installed and available on your `PATH`.**
  ChopMedia shells out to them for video probing, frame decoding, and
  exporting - it does not bundle them. On Windows, [scoop](https://scoop.sh)
  (`scoop install ffmpeg`) or a manual install from https://ffmpeg.org both
  work fine.
- A recent Rust toolchain (edition 2024, so Rust 1.85+).

## Running

```sh
cargo run --release
```

## Prebuilt packages (GitHub Actions)

The `Build desktop packages` workflow publishes artifacts for:

- **Windows** (`ChopMedia-windows-thin`, `ChopMedia-windows-fat`)
- **macOS** (`ChopMedia-macos-thin`, `ChopMedia-macos-fat`) as a proper
  `ChopMedia.app` bundle
- **Linux** (`ChopMedia-linux-thin`, `ChopMedia-linux-fat`)

Package variants:

- **thin**: only the app
- **fat**: app + bundled `ffmpeg` and `ffprobe`

## Dual-mode binary

The same executable doubles as a CLI:

- **Launched with no terminal attached** (double-clicked): the GUI opens.
- **Launched from a terminal**: the CLI runs. Use `ChopMedia gui` to open
  the GUI from a terminal anyway.

```sh
ChopMedia help                     # full usage
ChopMedia info <input>            # duration / resolution / fps
ChopMedia caps                    # probed ffmpeg capabilities
ChopMedia export in.mp4 -o cut.mp4 --start 2 --end 6
ChopMedia export in.mp4 -o clip.mp4 --crop 640:360:100:50 \
    --codec h264 --crf 20 --speed veryfast --audio aac:192
```

The export command shares the GUI's ffmpeg pipeline: stream copy or
re-encode (crop/scale/fps force a re-encode), container/codec/audio
choices validated against the probed ffmpeg build, live progress, and
`Ctrl+C` cancels the running ffmpeg cleanly.

## Features

- A bold, dark custom theme with hand-drawn Lucide-style vector icons (no
  emoji or raster icon assets).
- Load a video (`mp4`/`mkv`/`mov`/`avi`) via a native file dialog.
- Scrub a timeline with generated thumbnails, dimmed out-of-range regions,
  a capped playhead, and chunky color-coded start (teal) / end (coral) trim
  handles with drag tooltips.
- Blender-style transport bar: jump to clip start, play/pause, jump to clip
  end, and a loop toggle - playback loops the trimmed range by default.
  VLC-style layout: timecode on the left, transport in the center, and a
  volume slider with a mute toggle on the right.
- Preview audio kept in sync via the audio playback position, with a
  mirrored-bar waveform strip.
- Crop overlay with a dimmed mask, rule-of-thirds guides, eight resize
  handles (corners + edges), an aspect-ratio lock (Free / 1:1 / 16:9 / 9:16 /
  4:3), and a live pixel-dimension readout while dragging. Cropping forces
  re-encoding on export.
- Trim start/end as editable timecodes, plus keyboard shortcuts:
  - `Space` - play/pause
  - `←` / `→` - step one frame
  - `Home` / `End` - jump to clip start/end
  - `I` / `O` - set trim start/end at the playhead
  - `L` - toggle loop playback
  - `M` - mute/unmute preview audio
  - `Esc` - quit
- An export settings modal with container (MP4/MKV/MOV/WebM), stream-copy
  vs. re-encode mode, video codec (H.264/H.265/VP9/AV1), CRF quality, speed
  preset, audio (Copy/AAC/Opus/none), resolution scaling, and frame-rate
  override - plus a live output summary. The offered containers, codecs,
  speed presets, and CRF ranges are auto-detected by probing the local
  `ffmpeg` build (`-muxers`, `-encoders`, and `-h encoder=...`) at
  startup. Export runs via `ffmpeg` with a progress modal and cancel
  support.

## Known limitations

- Frame seeking is keyframe-granular (fast `-ss` seeks), not always
  frame-exact, while scrubbing/previewing. Export is unaffected.
- No gamepad support (the old prototype's Xbox-controller "Back to quit"
  was dropped as out of scope for a desktop tool).
