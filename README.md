# mypaper

**Total Windows customization at close to zero cost.** Live wallpapers (shaders, video,
full 3D scenes), and eventually a full retheme of Windows itself: taskbar, Start,
Settings, File Explorer, title bars and icons. You could make Windows look like Linux, macOS or
something new, without your desktop paying for it.

> Status: **phases 1–2 done.** All five wallpaper types (shader, image, slideshow, hardware video,
> 3D glTF scenes) render behind the desktop icons, with smart pausing (covered, fullscreen games,
> battery, lock), live config, a tray icon, the Studio app, hot reload, a supervisor and a panic
> hotkey. Next up is native retheming; see the [Roadmap](#roadmap).

## Principles

1. **Lightness comes from scheduling.** Render only what's visible, and only when it
   changes. A wallpaper hidden behind a maximized window costs 0% CPU and GPU: the
   engine sleeps on the message queue until a window event wakes it.
2. **A small native core, with everything visual as content.** Themes, shaders and scenes are
   files that reload live. Rust only changes when the engine does.
3. **Separate processes.** If a wallpaper crashes, nothing else goes down. The supervisor
   restarts it with backoff.
4. **Always reversible.** The panic hotkey restores the stock desktop from any state.
5. **Injection only into Windows itself.** Never games, never third-party apps
   (see [Injection](#injection-windows-only)).

## Quick start

Requirements: Windows 10/11, [rustup](https://rustup.rs). The pinned nightly toolchain
installs itself on first build.

```sh
cargo build --release
target/release/mypaper-core       # behind your icons, supervised, with a tray icon

# Studio (gallery, new-theme wizard, settings): Tauri + React, needs pnpm
cd apps/studio && pnpm install && pnpm build && cd src-tauri && cargo build --release
# then: tray icon → Open Studio, or run apps/studio/src-tauri/target/release/mypaper-studio.exe

cargo paper --windowed            # dev: render a theme in a normal window
```

**Panic hotkey: `Ctrl+Alt+Shift+Backspace`.** It stops everything and restores your desktop.
`Ctrl+C` in the console does the same.

Themes are found by name in `%APPDATA%\mypaper\themes` and then in a `themes/` folder beside the
exe or any parent folder, so you can double-click `target\debug\mypaper-core.exe`. Debug builds
log to their console. Release builds (`cargo build --release`) open no console and log to
`%LOCALAPPDATA%\mypaper\mypaper.log`.

`mypaper-core` takes the theme from the [config](#config-tray-and-pausing). `mypaper-core aurora`
(or `mypaper-wallpaper aurora`) overrides it for that run, which is handy in development.

## The dev loop: three speeds

| You change | What happens | Measured |
|---|---|---|
| A theme file (`themes/*/`, `.wgsl`, `theme.json`) | Live reload in the running wallpaper. Errors print with exact line numbers, and the last good shader keeps running. | **25–70 ms** |
| Any Rust | Incremental rebuild with Cranelift, the parallel frontend and rust-lld | **1.3–2.6 s** |
| Engine Rust, hot-patched | [subsecond](https://crates.io/crates/subsecond) patches the running process | ⚠️ experimental, see below |

Most visual iteration happens in the first row. Rust rebuilds are fast enough that restarting
`cargo paper --windowed` is the normal loop.

**Rust hot-patching (experimental, not working yet).** The wiring is in place (the `hotpatch`
feature, plus `Engine::tick` routed through subsecond's jump table), and
[`scripts/hotpatch.cmd`](scripts/hotpatch.cmd) runs it with the setup `dx` needs on Windows
(the MSVC environment, `link.exe`, and LLVM instead of Cranelift). Patches build in about 2 s and
reach the app, but they don't take effect yet. This is tracked as an open issue. Requires
Dioxus CLI 0.7.10 (`cargo binstall dioxus-cli@0.7.10`).

### Why compiles are fast

- `rust-toolchain.toml` pins **nightly** with the **Cranelift** backend component.
- `.cargo/config.toml`:
  - Cranelift compiles our crates. Dependencies are built once with LLVM at `opt-level=2`,
    so wgpu and naga are fast at runtime and never rebuilt.
  - `-Zthreads=8` turns on the parallel frontend, and `-Zshare-generics` reuses
    monomorphizations across crates.
  - `rust-lld` replaces `link.exe`.
- `debug = "line-tables-only"` keeps backtraces and drops most debuginfo work.
- Many small crates, so an edit only recompiles the crate it touches.
- wgpu is built with **only the DX12 backend**: no Vulkan, GL or Metal to compile.
- Tip: put the repo on a [Dev Drive](https://learn.microsoft.com/windows/dev-drive/)
  (ReFS, with Defender in performance mode) for another large speedup.

## Architecture

```text
mypaper-studio      Tauri app: gallery, new-theme wizard, settings → writes config.json
mypaper-core        supervisor: spawn/restart, tray icon, panic hotkey, single instance
  └─ mypaper-wallpaper   one process, one GPU device, one surface per monitor
        ├─ sources           shader · image · slideshow · video · scene (one trait, shared blit/fit)
        ├─ desktop attach    behind the icons (24H2+ layered layout, plus the legacy WorkerW)
        ├─ pause logic       WinEvent hooks → "is this monitor covered?" → sleep
        ├─ hold reasons      user / battery / display off / locked / fullscreen app → sleep everywhere
        ├─ live config       config.json watcher → theme switch, pause settings, no restart
        ├─ frame pacing      high-res waitable timer, per-theme fps cap
        └─ hot reload        file watcher → WM_RELOAD → recompile
```

| Crate | What it owns |
|---|---|
| [`crates/core`](crates/core) | Supervisor process, restart policy (unit-tested), tray icon and menu, panic hotkey |
| [`crates/wallpaper`](crates/wallpaper) | Engine: wgpu renderer, frame loop, hold reasons, live config, shader prelude, dev window |
| [`crates/desktop`](crates/desktop) | Win32 plumbing: attaching to the desktop, monitors, occlusion, fullscreen detection, battery, config watcher, Ctrl+C |
| [`crates/config`](crates/config) | The user's settings file (`config.json`), shared by core, the engine and the studio |
| [`crates/theme`](crates/theme) | Theme format and validation |
| [`crates/inject`](crates/inject) | Injection allowlist policy (unit-tested). The injector itself is coming. |
| [`apps/studio`](apps/studio) | mypaper Studio: Tauri v2 + React + Tailwind control app |

### What keeps it light

Measured with release builds (Ryzen laptop, AMD iGPU and NVIDIA dGPU, two 1440p monitors):

| | CPU (whole machine) | Notes |
|---|---|---|
| Shader (Aurora), 60 fps | **0.23%** | engine binary 3.6 MB |
| Video, 1080p30 on a 1440p screen | **0.23%** | hardware decode, zero-copy D3D11→D3D12 |
| Image | **0%** after the first frame | drawn once per screen, then idle |
| Slideshow | **0%** between slides | next image decoded ahead on a worker thread |
| Everything covered, or a game fullscreen | **0.00%** | core: 0.36 MB binary, 8 MB RAM |

Memory is the open problem: the engine's working set is 150–290 MB (private ~210 MB). Almost all of
it belongs to the GPU drivers: AMD's shader compiler, plus NVIDIA's overlay/capture hooks (`nvspcap64`
and its D3D11 driver), which load into *every* Direct3D process on a machine with GeForce software,
whether or not the process uses the NVIDIA GPU. Restricting wgpu to the low-power GPU was tried and
measured at only about 20 MB saved, so it isn't worth it. Next to measure: a bare DX12 baseline, and
whether disabling the NVIDIA overlay (or a per-app GPU preference) removes the hooks.

- Event-driven throughout. No polling threads. Fully covered monitors are not rendered at all.
- The low-power (integrated) GPU is preferred, so a wallpaper never wakes your dGPU.
- `Mailbox` present plus our own timer: no vsync blocking, and frame latency is 1.
- Shader time stops while paused and resumes without a jump.
- Release builds use thin LTO, `panic=abort` and stripped binaries.

## Config, tray and pausing

Settings live in `%APPDATA%\mypaper\config.json`. Every field is optional, and unknown fields are
rejected so typos fail loudly:

```json
{ "theme": "aurora", "paused": false, "pause_on_battery": true, "pause_on_fullscreen": true }
```

The wallpaper watches the file and applies edits live, with no restart and no IPC (the studio, the
tray and a text editor all just write the file):

- **`theme`** (a name or a folder path) loads in-process and the theme file watcher follows the new
  folder. If the new theme fails to load, the error is logged and the current one keeps running.
- **`paused`, `pause_on_battery`, `pause_on_fullscreen`** update the pause state immediately.
- A config that doesn't parse is logged and ignored, so the last good settings stay in effect.

**Tray icon.** `mypaper-core` shows a tray icon (tooltip `mypaper: <theme name>`). Right-click for
**Pause / Resume**, **Themes** (every theme in your themes folder and the bundled ones, the active one
checked), **Open Studio** (greyed out if `mypaper-studio.exe` isn't beside core or in an
`apps/studio` dev build), **Restart wallpaper** and **Quit** (same cleanup as the panic hotkey).
The icon comes back by itself after an Explorer restart.

**Pausing.** Any one of these puts every screen to sleep at 0% CPU and GPU (each change is logged
with its reason):

| Reason | When |
|---|---|
| paused by user | `paused` is set (tray or studio) |
| on battery | `pause_on_battery`, and the PC runs on battery |
| display off | the monitors are off (always) |
| session locked | the session is locked (always) |
| fullscreen app in front | `pause_on_fullscreen`, and a fullscreen game or app is in front |

The fullscreen rule is there so games get the whole GPU: **while a game is fullscreen, every mypaper
screen is paused, including your other monitors.** It triggers when Windows reports an
exclusive-fullscreen Direct3D app or presentation mode (`SHQueryUserNotificationState`), or when the
foreground window covers its entire monitor, which catches borderless-fullscreen games and F11 video.
Maximized windows (which stop at the taskbar) and the desktop itself don't count. It's re-checked
only when window events fire, never on a timer. Battery, display and lock changes arrive as Windows
notifications to an invisible message-only window, so nothing polls.

## Themes

A theme is a folder:

```text
themes/aurora/
  theme.json
  aurora.wgsl
```

```json
{ "name": "Aurora", "wallpaper": { "kind": "shader", "source": "aurora.wgsl", "fps": 60 } }
```

Unknown fields are rejected, so typos fail loudly. Relative asset paths can't escape the theme
folder; absolute paths (your own pictures and videos) are fine. Your own themes go in
`%APPDATA%\mypaper\themes`; the Studio's **New theme** flow creates them for you.

| `kind` | Fields (defaults) | Bundled example | How it stays light |
|---|---|---|---|
| `shader` | `source`, `fps` (60), `scale` (1) | `aurora` | fps cap; `scale: 0.5` renders a quarter of the pixels and upscales |
| `image` | `source`, `fit` (cover) | `photo` | WIC decode, downscaled to the largest monitor; drawn once |
| `slideshow` | `folder`, `interval` (300 s), `transition` (1.5 s), `shuffle`, `fit` | `gallery` | idle between slides; worker-thread prefetch; at most 2 textures |
| `video` | `source`, `fit`, `fps` (30), `speed` (1) | `loop` | Media Foundation hardware decode, zero-copy into wgpu, pauses with the clock |
| `scene` | `source` (.glb/.gltf), `fps` (60), `scale` (1), `orbit` (4°/s), `parallax` (0.15), `background` | `islands` | merged buffers, few draw calls; static if orbit and parallax are both 0 |

`fit` is `cover`, `contain`, `stretch` or `center`. Image formats are whatever WIC decodes (jpg,
png, webp, heic, …); video is anything Media Foundation plays with 8-bit output (H.264, HEVC
8-bit, VP9 with the codec extension, …).

### Writing a shader wallpaper

Define one function. Everything else (uniforms, the fullscreen triangle, entry points) is
in [`prelude.wgsl`](crates/wallpaper/src/prelude.wgsl), which is appended after your code so
error line numbers match your file.

```wgsl
fn wallpaper(frag: vec2f) -> vec4f {
    let uv = frag / paper.resolution;   // 0..1, top-left origin
    return vec4f(uv, 0.5 + 0.5 * sin(paper.time), 1.0);
}
```

`paper.resolution`, `paper.mouse` (pixels), `paper.time` (seconds, stops while hidden),
and `paper.frame`.

## Injection: Windows only

Restyling native Windows UI (Settings, the taskbar, Start, Explorer) means injecting into
those processes, like [Windhawk](https://windhawk.net) does. mypaper's hard rule:

- **Strict allowlist**: specific Windows executables plus inbox packaged apps. Everything else
  is denied, including Microsoft-published games.
- **Must be signed by Microsoft**, so a renamed `explorer.exe` in a games folder is rejected.
- **Targeted, not global**: no `SetWindowsHookEx`, no following child processes. Games never load our code.
- Critical processes (`lsass`, `csrss`, `winlogon`, `dwm`, …) can never be targeted.

The rule lives in [`crates/inject`](crates/inject/src/lib.rs) as a pure function with tests,
so it's checked rather than trusted.

## Roadmap

1. ✅ **Engine**: shader wallpapers behind the icons, multi-monitor, pausing when covered, hot reload,
   supervisor, panic hotkey, fast dev loop.
2. ✅ **All wallpaper types and the desktop app**: image, slideshow, hardware video and glTF scenes,
   render scale, pausing for battery, lock, display off and fullscreen games, live config, the tray, and
   the Studio. Still open: cutting driver memory (see the note above), Rust hot-patching, web
   (WebView2) wallpapers, 10-bit/HDR video, and EXIF rotation.
3. **Native tweaks**: DWM accent, borders, corners and Mica on allowlisted windows. Palette
   extraction so the accent color follows the wallpaper. An undo journal for every change.
4. **Style engine and injector**: CSS-like selectors over the XAML visual tree (XAML Diagnostics) and
   uxtheme hooks for Win32, one theme language for every UI framework, pushed live into
   allowlisted processes. First target: restyling the Settings app live.
5. **Interactive 3D desktops**: embed Unity players (`-parentHWND`) for game-like desktops on top of the
   glTF runtime.
6. **Shell additions**: a dock, a top bar and widgets, for layouts Windows doesn't have.
7. **Studio, part 2**: a theme editor with an element inspector (click any UI element to get its selector).
