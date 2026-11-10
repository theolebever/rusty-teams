# Rune spike: call view with live video tiles

The question was whether Rune (rev `7ed827a`) can show call video at 30 fps. It can. The spike draws 4 synthetic 1280x720 feeds, a self view, the Teams control bar and an incoming call card. See `meeting.png` and `ringing.png` (headless, 1600x900).

## Measurements

Windowed run, `--scene meeting`, 20 s, one Windows machine (GPU not recorded):

| Metric | 5 s | 20 s |
|---|---|---|
| Presented fps per tile | 30 | 30 (single samples of 29 or 31) |
| CPU, share of 12 logical cores | 10.3% | 10.3% |
| Working set | 158.9 MB | 144.9 MB |
| Private memory | 258.6 MB | 244.6 MB |

The CPU figure includes frame generation, colour swizzle, upload and drawing. The split was not profiled. Memory did not grow.

## How frames reach the GPU

- `NativeSurface::push_bytes` keeps only the latest frame and wakes the UI through the waker from `AppSpec::setup`. It accepts BGRA or NV12, not RGBA, so the spike swizzles in the producer.
- On Windows each pushed frame creates a new `wgpu` texture and uploads into it. The old texture is dropped, and the 20 s run showed no accumulation. Reusing one texture per tile would need a change in Rune.
- The `NativeSurface` observer reports each completed draw by frame id. It is the only public hook for counting presented frames.
- `Pixels::set` is the other path. It takes RGBA, reuses a CPU atlas and uploads only changed rows. It has no per-draw callback, so it was not measured.

## Fit with `crates/calling`

`calling` already hands the UI BGRA `VideoPicture`s at 640 px and 30 fps, keeping only the latest frame through `VideoHub`. That maps to `push_bytes` with `Format::Bgra` and needs no swizzle. Only the `calling` code was read; it was not run against Rune.

## Other call features

- Mini window and pop-out: `rune::run_windows` and overlays exist. Not built.
- Screen share: `push_shared` takes Windows D3D shared textures. Not built.
- The incoming call card is drawn inside the window. The current app uses a topmost OS-level ring window, which the port has to keep.
- Chat, People, Raise, React, View, More and Share are display-only. GUARDRAILS.md forbids dead clicks, so they cannot ship like this.

## Build

`cargo build --release` passes with no warnings. The link step needs the same MSVC `PATH`/`LIB` fix as `spikes/rune-teams`, plus the UCRT lib path in `LIB`.
