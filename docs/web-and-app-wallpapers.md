# Web and app wallpapers

Two wallpaper kinds draw with something other than Glaze's own renderer: `web` (a page in
WebView2) and `app` (an external program, such as a Unity player build). Both draw with child
windows of the desktop windows Glaze already creates, so the engine neither draws nor presents
frames for them. What it still does is decide *when they are visible*, and suspend them when they
are not. That is what keeps them cheap.

| | `web` | `app` |
|---|---|---|
| Runs as | WebView2 (Edge) processes, started by Glaze | One process you provide |
| Monitors | One WebView per monitor | Monitor 0 only |
| Hidden means | page hidden and **suspended** (`TrySuspend`) | process **suspended** (`NtSuspendProcess`) |
| Dies when | the theme changes or Glaze exits | the theme changes or Glaze exits (Job object) |
| Example | [`themes/webdemo`](../themes/webdemo) | [`themes/app-example`](../themes/app-example) (a Unity how-to) |

## `web`

```json
{ "name": "Web demo", "wallpaper": { "kind": "web", "url": "index.html" } }
```

`url` is either an `http://` / `https://` address, or a path to a local `.html` file: relative to
the theme folder, or absolute. Relative paths may not use `..`, like every other theme asset.

**Local pages** are served from a virtual host, `https://glaze.theme/`, mapped to a folder
(`SetVirtualHostNameToFolderMapping`), not loaded from `file://`. The page gets a real origin, so
`fetch`, ES modules, workers and relative asset paths work. The folder is the theme folder when the
page is inside it (so subfolders such as `js/` and `img/` work), otherwise the page's own folder. A
missing file makes the theme fail to load with a clear error (the red "broken" screen) instead of
showing a blank page.

**One environment, one WebView per screen.** A single WebView2 environment is shared by all screens,
with its profile in `%LOCALAPPDATA%\glaze\webview2`. Each screen gets its own controller as a child
of that screen's window, filling its client area, so the browser processes are shared and you pay for
one renderer per page, not one browser per monitor.

**Browser flags:** `--disable-features=msSmartScreenProtection` (a wallpaper loads its own content, so
no reputation lookups) and `--autoplay-policy=no-user-gesture-required` (nobody clicks a wallpaper).
Background throttling is left **on**, so the browser throttles whatever a hidden page still runs.

**Turned off:** context menus, DevTools, the status bar, zoom (and pinch zoom), script dialogs,
swipe navigation and browser accelerator keys. The page background is black until the first paint.

### Staying light

When a screen is hidden (covered by a maximized window, a fullscreen game, battery, lock, display
off, or paused from the tray or Studio), Glaze calls `put_IsVisible(false)` and then
`ICoreWebView2_3::TrySuspend` on its WebView. A suspended page runs no script, timers or rendering, so
its processes use no CPU at all (it does not give memory back, see [Measurements](#measurements)).
When the screen is visible again it calls `Resume` and
`put_IsVisible(true)`, and the page continues where it left off. A page that is still loading when
it is hidden is suspended as soon as its first navigation completes.

A page that animates should still cap itself: the demo throttles `requestAnimationFrame` to 30 fps
and draws at 1/8 of the screen's resolution. WebView2 composites and presents on its own, so
Glaze cannot pace a page for you. A visible web page is the heaviest wallpaper type (see
[Measurements](#measurements)), so prefer low frame rates and mostly static content.

### What a page can't do

- **No input, unless asked.** Wallpaper windows sit *behind* the desktop icons, so clicks and keys
  never reach the page. Keys never do. With `"interactive": true` in the theme's wallpaper section, the
  mouse (move, buttons, wheel) is forwarded while the pointer is over the *bare* desktop, and arrives as
  ordinary `mousemove` / `mousedown` / `mouseup` / `wheel` events (see
  [desktop-extras.md](desktop-extras.md#interactive-wallpapers) and `themes/interactive`). Links and
  forms still make little sense: treat the page as a canvas.
- **No audio policy.** Autoplay is allowed, so a page with sound will play it. Mute in the page.
- **No crash recovery yet.** If the WebView2 browser process dies, that screen stays blank until the
  theme reloads (touch a file in the theme folder, or re-select the theme).
- Editing any file in the theme folder reloads the theme, which recreates the WebViews (a short
  black flash). That is the normal hot reload, and it is handy while developing a page.
- Needs the WebView2 Runtime (preinstalled on Windows 11 and current Windows 10). If it is missing the
  theme fails to load and the log says where to get it.

## `app`

```json
{
  "name": "My scene",
  "wallpaper": {
    "kind": "app",
    "command": "MyScene.exe",
    "args": ["-parentHWND", "{hwnd}", "delayed"]
  }
}
```

`command` is relative to the theme folder or absolute. In `args`, `{hwnd}` is replaced with the
decimal handle of **screen 0's window** (the first monitor), wherever it occurs in an argument. That
is the form Unity's `-parentHWND <handle> delayed` expects. See
[`themes/app-example`](../themes/app-example/README.md) for the Unity setup.

**Starting.** Glaze creates the process suspended, puts it in a Job object, and only then lets it
run, with a window event hook already watching its process id. It is started with
`STARTF_USESHOWWINDOW` and `SW_SHOWNOACTIVATE`, and Glaze never calls `SetForegroundWindow`, so the app
does not take focus. (An app that insists on activating its own window cannot be stopped from
outside; Unity's embedded player does not.)

**The Job object** has `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`. When the theme changes, Glaze exits or
Glaze is killed, Windows closes the last handle to the job and terminates the app. It cannot outlive
glaze.

**Finding the window.** Apps that honor the parent argument (Unity) create their window as a child of
the screen window: Glaze just sizes it to fill. Apps that make their own top-level window instead
are adopted: for up to 5 seconds Glaze listens for window creation and show events from that
process (a WinEvent hook, delivered through the engine's message loop, so nothing polls and nothing
blocks). The first window that is visible, unowned, top-level and not a tool window is reparented:
caption, borders and system menu are stripped, `WS_CHILD` is set, `SetParent` moves it into the screen
window and it is sized to fill. Hidden helper windows that every GUI thread creates (IME, tooltips)
are skipped. If no window shows up in 5 seconds, Glaze assumes the app draws straight into the
screen window and stops looking (the log says so).

### Staying light

While screen 0 is hidden, the whole process is **suspended** (`NtSuspendProcess`) and its working set
is trimmed (`EmptyWorkingSet`). It resumes when the screen is visible again.

Why suspend rather than hide the window: hiding a window does not stop a Unity player. It keeps
simulating and rendering to a window nobody sees, which is exactly the cost Glaze exists to avoid.
Suspending costs the app nothing (0% CPU and GPU) and needs no cooperation from it. It has two
trade-offs:

- Anything the app does in the background stops too: audio, network, timers. A suspended game looks
  frozen for the instant after resume, and its clocks jump (Unity clamps `Time.deltaTime`).
- A suspended process can't answer window messages, so Glaze never sends it any while it is
  suspended (resizing waits until it is running again) and uses asynchronous window positioning, so a
  busy app can't block the engine. Because the app's window is a *child* of a desktop window, Windows
  does not show a "not responding" ghost for it.

A process is never suspended before its window has been found or the 5 second wait is over, so a slow
starter is not frozen mid-start.

### Limits

- **One instance, monitor 0 only.** Other monitors get no window from the app and show nothing of it.
  (`{hwnd}` is a single handle, and most players render into exactly one window.) A multi-monitor app
  would need one instance per screen, or an app that spans the virtual desktop.
- No restart if the app exits or crashes: the wallpaper area stays blank. Switch theme or restart the
  wallpaper from the tray.
- Only the process Glaze starts is watched. If it launches another process that makes the window
  (a launcher stub), nothing is embedded. Point `command` at the real executable.
- No input: like every wallpaper, the app sits behind the icons and gets no clicks.

## Glass over them

Widgets and bars get the same Liquid Glass over a web or app wallpaper as over any other. The
engine has no frame of its own to draw their plates into here, so it draws each plate as a glass
overlay instead (the flyouts' machinery: a capture of the screen without our windows, and a
click-through window directly under the surface), with the plate's own material, style and
corners. The capture follows the wallpaper: a still page costs nothing, an animated one redraws the
glass at most 30 times a second (fewer under the theme's `fps`), and a covered or held screen keeps
its glass as last drawn without capturing. Details and measurements:
[glass.md](glass.md#glass-over-web-and-app-wallpapers-p14-6-november-2026).

## How it plugs into the engine

Two small generic hooks on the `Source` trait (in `crates/wallpaper/src/sources/mod.rs`) serve every
source that owns its own windows:

```rust
fn native(&self) -> bool { false }                              // draws with its own child windows
fn layout(&mut self, screen: usize, hwnd: HWND, size: (u32, u32)) {}  // a dirty screen, instead of render
fn set_visible(&mut self, screen: usize, hwnd: HWND, visible: bool) {}
```

- `sources::load` also receives the screen windows (`&[HWND]`, in screen order). Sources create their
  child windows or processes from them, and report failures synchronously, so a bad theme keeps the
  previous one running.
- When `native()` is true, the engine skips acquiring and presenting a frame for the source. A dirty
  screen (new, resized in dev mode, resumed, reloaded) calls `layout` instead of `render`.
- `set_visible` is called whenever a screen pauses or resumes (covered, minimized) or the whole
  engine is held or released (battery, lock, fullscreen app, user pause). Every screen is visible when a
  source is created. Sources that run nothing of their own ignore it.
- The main thread is a COM single-threaded apartment (`window::init_com_apartment`), as WebView2
  requires. WebView2's completion handlers arrive through the same message pump the engine
  already runs, so no extra thread is involved.

- Because these sources never acquire a frame, a screen's swapchain is now created on first
  `acquire` (`Screen::acquire` in `gpu.rs`), not when the screen is created. Web and app themes
  never create one: the engine process measured **126 MB** working set instead of **193 MB**
  with the web demo (and about 120 MB less private memory), since an unpresented 1440p swapchain per
  monitor is pure cost.

## Measurements

Debug build of the engine, two 2560x1440 monitors, WebView2 runtime 156, Windows 11, a busy machine
(other work was running, so treat the visible numbers as +/- 30%). CPU is the total of the
WebView2 processes (the engine's children), as a share of one core / of the whole 16-thread machine.

| `web`, `themes/webdemo` | CPU, one core | CPU, machine | WebView2 working set |
|---|---|---|---|
| visible, both screens, 30 fps | 13-19% | 0.8-1.2% | ~400 MB (7 processes) |
| hidden (suspended) | **0.00%** | **0.000%** | ~385 MB, unchanged |
| both monitors covered at startup | 0.3% (the first second) | 0.02% | |

- Suspended is exactly zero: every one of the 7 `msedgewebview2` processes had 0 CPU time over 8
  seconds. The page resumes and keeps animating.
- **Memory is not freed by suspending**, at least not within seconds: the working sets did not
  shrink. (The browser keeps the pages' memory; Windows would trim it only under pressure.) Suspending
  buys CPU, not RAM.
- Visible cost is mostly Chromium's own frame pipeline (the GPU process plus a renderer per
  screen), not the page: drawing the demo at 1/4 or 1/8 resolution, with gradients or with sprites,
  measured the same within noise. If a page does not need 30 fps, the biggest lever is its frame rate.

| `app`, a small GDI test renderer | |
|---|---|
| embedded (child window or reparented top-level window) | fills the 2560x1440 screen window |
| suspended | threads all `Suspended`, 0.00% CPU, working set trimmed from ~7 MB to ~0.1 MB |
| resumed | animates again (the test app logs a heartbeat that stalls exactly while suspended) |
| theme switched, or Glaze hard-killed | the app process is gone within a second |
| focus | the foreground window never changed |

No Unity build was available. Embedding was tested with a purpose-built Win32 renderer in both
modes: one that honors a `--parent <hwnd>` argument, and one that opens its own top-level window.
