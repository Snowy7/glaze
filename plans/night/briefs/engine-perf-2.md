# Brief: engine-perf round 2 (hidden = free)

Read `plans/native/LIVE.md`, the engine sections of `docs/performance.md`
and `docs/engine-notes.md`, and round 1's commits (`git log -- crates/wallpaper crates/glass-ipc
crates/liquid-rust`). Same ownership as round 1 (`briefs/engine-perf.md`).

## Where it is

Round 1 found the live 27 % was an endless Studio glass-preview request loop (~6,700 "no frame"
replies a second), and fixed it. At 05:15 the engine measured 4.2 % of one core, 7.6 % GPU 3D,
173 MB private on rain-city. It also added the style knobs for the bars.

## Decision (made by the orchestrator for the user: RAM counts as much as CPU)

**When every screen has been fully covered (maximized or fullscreen apps) for 10 s, release the GPU
work and memory.** Keep showing the theme's still frame on the desktop, which the engine already
has. Resume animation when any screen is uncovered again. The still covers the few hundred ms of
restart, so nothing ever goes blank or flashes. Quick alt-tabs inside the 10 s grace must not
thrash. Release as much as possible: swapchains, kept frames, render targets, pipelines, and the
device itself if resuming stays under ~500 ms. Measure what each step frees and what resuming
costs, and pick the deepest release that resumes cleanly. Glass over apps (overlays) still has
to work while released: if overlays need the device, keep only what they need, or bring it up on
demand.

## Then keep climbing

- Rain CPU 4.2 % → <= 3 %: per-thread profile; the remaining wakes, present path, plate traffic.
- Memory while visible: 173 MB private. What's left per item (swapchains, kept frames, baked rain,
  descriptor heaps, adapter probing). Patching wgpu-hal is out of scope tonight; everything else
  is in.
- GPU per screen toward 2 ms without visual change.

## How / stop

As round 1: interleaved measurements, one change per commit with numbers, quality proven
(lab frames or captures), deploy live when a win is real, desktop lock one action at a time.
Stop when done, or after 3 hours. Report as round 1.
