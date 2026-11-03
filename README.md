# mypaper

**Total Windows customization at close to zero cost.** Live wallpapers (shaders, video,
full 3D scenes), and eventually a full retheme of Windows itself: taskbar, Start,
Settings, File Explorer, title bars and icons. You could make Windows look like Linux, macOS or
something new, without your desktop paying for it.

> Status: **phase 1.** The engine, desktop attach, pausing, hot reload, supervisor and panic
> hotkey all work. See [Roadmap](#roadmap).

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
cargo build
cargo paper --windowed            # wallpaper in a normal window (safe dev mode)
target/debug/mypaper-core         # the real thing: behind your icons, supervised
```

**Panic hotkey: `Ctrl+Alt+Shift+Backspace`.** It stops everything and restores your desktop.
`Ctrl+C` in the console does the same.

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
mypaper-core        supervisor: spawn/restart, panic hotkey, single instance
  └─ mypaper-wallpaper   one process, one GPU device, one surface per monitor
        ├─ desktop attach    behind the icons (24H2+ layered layout, plus the legacy WorkerW)
        ├─ pause logic       WinEvent hooks → "is this monitor covered?" → sleep
        ├─ frame pacing      high-res waitable timer, per-theme fps cap
        └─ hot reload        file watcher → WM_RELOAD → recompile
```

| Crate | What it owns |
|---|---|
| [`crates/core`](crates/core) | Supervisor process, restart policy (unit-tested), panic hotkey |
| [`crates/wallpaper`](crates/wallpaper) | Engine: wgpu renderer, frame loop, shader prelude, dev window |
| [`crates/desktop`](crates/desktop) | Win32 plumbing: attaching to the desktop, monitors, occlusion, Ctrl+C |
| [`crates/theme`](crates/theme) | Theme format and validation |
| [`crates/inject`](crates/inject) | Injection allowlist policy (unit-tested). The injector itself is coming. |

### What keeps it light

Measured with release builds (Ryzen laptop, AMD iGPU and NVIDIA dGPU, two 1440p monitors):

| | CPU (whole machine) | Binary |
|---|---|---|
| Rendering Aurora at 60 fps (1280×720 window) | **0.23%** | engine 3.6 MB |
| On the desktop, both monitors covered | **0.00%** | core 0.36 MB, 8 MB RAM |

Memory is the open problem: the engine uses 170–290 MB, and roughly 190 MB of that is NVIDIA driver
DLLs. wgpu probes *every* GPU at startup, even though we render on the iGPU. Fixing this
(choosing the low-power adapter before wgpu opens the others) is the top item in phase 2.

- Event-driven throughout. No polling threads. Fully covered monitors are not rendered at all.
- The low-power (integrated) GPU is preferred, so a wallpaper never wakes your dGPU.
- `Mailbox` present plus our own timer: no vsync blocking, and frame latency is 1.
- Shader time stops while paused and resumes without a jump.
- Release builds use thin LTO, `panic=abort` and stripped binaries.

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

Unknown fields are rejected, so typos fail loudly. Asset paths can't escape the theme folder.

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
2. **Lighter and more wallpaper types**: choose the adapter without waking or loading the dGPU
   (see the memory note above), and get Rust hot-patching working. Then images (with crossfades), hardware-decoded video (Media
   Foundation), render scale (render at 50% and upscale, for big savings on 4K).
   Also pausing on battery and lock.
3. **Native tweaks**: DWM accent, borders, corners and Mica on allowlisted windows. Palette
   extraction so the accent color follows the wallpaper. An undo journal for every change.
4. **Style engine and injector**: CSS-like selectors over the XAML visual tree (XAML Diagnostics) and
   uxtheme hooks for Win32, one theme language for every UI framework, pushed live into
   allowlisted processes. First target: restyling the Settings app live.
5. **3D scenes**: a glTF scene runtime, plus embedding Unity players (`-parentHWND`) for game-like
   interactive desktops.
6. **Shell additions**: a dock, a top bar and widgets, for layouts Windows doesn't have.
7. **Studio**: a theme editor with an element inspector (click any UI element to get its selector).
