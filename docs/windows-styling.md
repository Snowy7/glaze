# Native Windows styling

A native look can restyle Windows itself while it is active: window borders, title bars, corners,
backdrop (Mica and friends), the system accent color, light/dark mode and accent visibility.
Selecting `config.json` > `look.style` supplies Windows defaults as well as the native shell's.
A wallpaper can refine those defaults with its `windows` section. Everything uses supported public
APIs (`DwmSetWindowAttribute` and the same per-user registry values the Settings app writes). Frame
styling runs in core; the separate [experimental App styler](#app-styler-experimental) is opt-in.
Style changes transition directly to the next values. Releasing fields restores only those
fields; quitting, the panic hotkey and crash recovery restore all remaining changes.

```json
{
  "name": "Midnight",
  "wallpaper": { "kind": "shader", "source": "midnight.wgsl" },
  "windows": {
    "accent": "auto",
    "border": "auto",
    "corners": "round",
    "backdrop": "mica",
    "dark_mode": true
  }
}
```

[`themes/examples/midnight`](../themes/examples/midnight) is a working example.

**Your look wins.** A theme's `windows` section is a suggestion: `config.json` › `look.windows`
(edited in Studio › Look › Windows) is your layer over it, field by field. A field absent there
follows the theme or native style; `null` keeps Windows' own setting ("As is"); a value sets it. The fields and
values are the ones below. So styling Windows needs no copy of a built-in theme, and it survives a
wallpaper change. See [studio-ux.md](studio-ux.md#6-look-and-windows-styling-as-part-of-it-d2).

## One switch for the desktop

```json
{ "look": { "style": "fluent", "appearance": "dark", "accent": "auto" } }
```

The pure resolver is [`glaze_theme::windows::resolve`](../crates/theme/src/windows.rs).
Layers are native style defaults < wallpaper `windows` < shared `look.accent`/`appearance` <
explicit `look.windows` fields. A missing `look.style` preserves the previous behavior: only
the wallpaper and explicit Windows layer manage Windows. Removing both releases Windows.

| Style | Corners | Backdrop | Caption, dark / light | Automatic accent | Accent on Start/taskbar; titles |
|---|---|---|---|---|---|
| `liquid` | round | acrylic | `#1c1c1e` / `#f5f5f7` | wallpaper accent | on in dark mode, off in light; on |
| `oneui` | round | acrylic | `#17171a` / `#f4f4f6` | wallpaper accent | on in dark mode, off in light; on |
| `fluent` | small | mica | `#202020` / `#f3f3f3` | wallpaper accent | on in dark mode, off in light; on |
| `solid` | square | none | `#1c1c1e` / `#f2f2f7` | wallpaper accent | on in dark mode, off in light; on |
| `mono` | square | none | `#141416` / `#f7f7f8` | `#f2f2f2` dark / `#1c1c1e` light | off; off |

Default borders use the resolved accent; caption text is white in dark mode and `#1c1c1e` in
light mode. These surface colours match the native UI kit. DWM has three corner choices rather
than the kit's arbitrary radii, and its acrylic is Windows' material, not engine Liquid Glass.
Explicit colours can add colour to mono; an explicit `look.windows.accent: "auto"` uses the
wallpaper there too. Cursors are never chosen by a style.

`look.appearance: light/dark` sets both app and system mode. If absent, a theme's `dark_mode`,
then its bar text tone, then the native dark default supplies the mode. `appearance: auto`
leaves Windows' mode alone and resolves caption/mono colours against its unmanaged app scheme.
The journal's original scheme prevents a previous Glaze override feeding back into auto.
In `look.windows`, `apps: light/dark/match/as-is` is the higher-priority mode control (`dark_mode`
is the theme's field). `match` follows this same appearance resolution; `as-is` releases mode.

For example, `look.windows: { "border": null, "accent_on_taskbar": false }` leaves borders to
Windows and disables accent on Start/taskbar while the other fields follow the selected style.
Both accent-visibility fields accept absent, null, true and false independently. Windows only
supports a coloured Start/taskbar in dark system mode, so light/automatic-light defaults keep
that switch off. Explicit overrides are still honoured.

Resolution runs only on the existing config/theme/palette events. A message-pumping styling
worker coalesces pending requests for 35 ms, then applies the latest one; core's supervisor and
panic hotkey do not wait on registry IO, signature verification or settings receivers. The worker
waits indefinitely when idle, with no polling or idle timer. The experimental app styler uses the same resolved `$accent` (including
explicit Windows accent overrides) and wallpaper `$dominant`. If Windows accent is explicitly
unmanaged with null, app tokens fall back to the shared look/wallpaper accent. Without a matching
palette, automatic colours wait; fixed colours and mono's neutral defaults work immediately.
Turning on style synchronization does not turn on the experimental app styler.

## The `windows` section

Every theme field is optional; an unset field follows the selected native style, or leaves
Windows alone when no style was selected. Unknown fields are rejected.

| Field | Values | What it changes |
|---|---|---|
| `accent` | `"auto"` or `"#rrggbb"` | System accent color (registry, see [below](#system-accent-and-dark-mode)) |
| `border` | `"auto"` or `"#rrggbb"` | Window border (`DWMWA_BORDER_COLOR`) |
| `caption` | `"#rrggbb"` | Title bar background (`DWMWA_CAPTION_COLOR`) |
| `caption_text` | `"#rrggbb"` | Title bar text (`DWMWA_TEXT_COLOR`) |
| `corners` | `default`, `round`, `small`, `square` | Corner rounding (`DWMWA_WINDOW_CORNER_PREFERENCE`) |
| `backdrop` | `auto`, `none`, `mica`, `acrylic`, `tabbed` | System backdrop (`DWMWA_SYSTEMBACKDROP_TYPE`) |
| `dark_mode` | `true` / `false` | Dark title bars (`DWMWA_USE_IMMERSIVE_DARK_MODE`) and the system's app/system theme |
| `accent_on_taskbar` | `true` / `false` | Accent on Start/taskbar (`Themes\Personalize\ColorPrevalence`) |
| `accent_on_title_bars` | `true` / `false` | Accent on title bars/borders (`DWM\ColorPrevalence`) |
| `cursors` | `"glaze-dark"`, `"glaze-light"`, a set name, or a folder | The mouse cursor scheme (`HKCU\Control Panel\Cursors` + `SPI_SETCURSORS`), journaled like the accent; see [desktop-extras.md](desktop-extras.md#cursor-themes) |

Border, caption, corner and backdrop attributes need Windows 11 (build 22000, and 22621 for
`backdrop`); older builds ignore them. Dark title bars need Windows 10 20H1 or later. Whether a
window *shows* a backdrop is up to the app: Mica always covers the title bar, and shows through the
client area only where the app draws nothing opaque.

## Which windows

A window is styled only if all of this holds:

- It is top-level, visible and has a title bar.
- Its process is not elevated. Tool windows receive no corner preference; transparent,
  cloaked and borderless full-monitor windows are excluded. Glaze's own apps do not pass
  the Windows-app allowlist.
- Its process passes the injection allowlist in [`crates/inject`](../crates/inject/src/lib.rs)
  (`glaze_inject::evaluate`): Explorer, Settings, the frames around UWP apps, a few inbox apps,
  never games or third-party apps. The inputs are the image path
  (`QueryFullProcessImageNameW`), the package family name (`GetPackageFamilyName`) and whether the
  image is Microsoft-signed.
- The signature check, [`signature::microsoft_signed`](../crates/inject/src/signature.rs), is
  `WinVerifyTrust` on the file, falling back to the catalog (`CryptCATAdmin*`) lookup that covers most
  Windows binaries. The signer chain must also be Microsoft's (leaf `Microsoft…`, root
  `Microsoft Root…`), so a validly signed third-party file doesn't pass. It works offline, and
  the styling allowlist verdict (including a rejection) is cached per PID and process creation
  time. Multiple windows, repeated show events and style changes do not hash that process again;
  a reused PID gets a new verdict.
- The expensive signature check only runs for processes the allowlist would accept anyway, so a game
  opening a window never gets its executable hashed.

Existing windows are styled when the theme activates (`EnumWindows`). New windows are caught by an
out-of-context `EVENT_OBJECT_SHOW` WinEvent hook on the styling worker, so nothing polls.
Destroy events retire remembered HWNDs before they can be reused. Each window keeps its first
dark-mode value and its successfully applied attributes. Updates write only changed attributes;
only released attributes are restored. A one-second check of tracked windows reads the actual
corner preference and reapplies it if the app reset it, without writing unchanged attributes.
An app's explicit backdrop is preserved independently of its corner preference.
No restore/reapply sweep runs during a style change.

Studio exposes As is (release), Default (DWM's normal choice), Round, Small and Square in
Windows > Corners. This control styles supported Windows apps, not arbitrary third-party apps.
The DWM preference is a hint: per-pixel-alpha windows, custom regions and app-drawn corners
can prevent visible rounding even when the call succeeds.

Glaze Files and Studio themselves use Windows' 8 logical pixel outer contour, scaled by
window DPI, with square edges when maximized or touching a monitor/work-area edge. Files'
alpha content, caption hover clip and engine glass share the same device-pixel radius.
Studio uses DWM's frame. Menus and flyouts also use 8 logical pixels; widget cards and bars
keep their own design radii.

## System accent and dark mode

`accent` and `dark_mode` write the per-user values Settings itself writes, then broadcast
`WM_SETTINGCHANGE` ("ImmersiveColorSet") so the shell and apps apply the change live:

| Key (under `HKCU`) | Values |
|---|---|
| `Software\Microsoft\Windows\DWM` | `AccentColor` (`0xAABBGGRR`), `ColorizationColor` (`0xAARRGGBB`), `ColorPrevalence` (1 = accent on titles/borders) |
| `Software\Microsoft\Windows\CurrentVersion\Explorer\Accent` | `AccentColorMenu` (`0xAABBGGRR`), `AccentPalette` (8 swatches `R G B 00`: three lighter shades, the accent, three darker ones, then Windows' fixed orange) |
| `Software\Microsoft\Windows\CurrentVersion\Themes\Personalize` | `AppsUseLightTheme`, `SystemUsesLightTheme` (0 = dark), `ColorPrevalence` (1 = accent on Start/taskbar) |

The two `ColorPrevalence` switches are independent, journaled with the other originals and
restored when their fields are released, styling stops, or core recovers after a crash.

## `"auto"` colors from the wallpaper

`"auto"` resolves to an accent color computed from the wallpaper:

1. After the first frame of each newly loaded theme (including hot reloads), the wallpaper engine
   draws the source once more into an offscreen target, shrinks it to 64x36 on the GPU with the
   shared blit, and reads that thumbnail back once. This is one extra draw and a 9 KB readback per
   theme load, never per frame ([`palette.rs`](../crates/wallpaper/src/palette.rs)).
2. Near-black and near-white pixels are skipped. The rest vote for a hue: by count for the
   *dominant* color, and by squared vividness (saturation times midtone-ness) for the *accent*, so a
   small vivid streak can win the accent while the dominant stays the broad color. The accent is then
   pushed to saturation 0.6-1.0 and lightness 0.45-0.62 so it works as a UI color. A wallpaper with
   no usable color (all black, all white) gets no palette, and gray wallpapers get a gray accent.
3. The result is written atomically to `%LOCALAPPDATA%\glaze\palette.json`:

   ```json
   { "theme": "D:\\themes\\midnight", "dominant": "#1a714f", "accent": "#19cd86" }
   ```

   `theme` is the folder it was computed for. A palette from another theme is never applied.
4. Core watches the file and applies the colors when it changes. Until the first palette arrives,
   `"auto"` fields are skipped; both `accent` and `border` use the palette's accent.

## Undo

Nothing is changed without a way back.

- **Windows**: each touched window remembers the dark-mode value it had, and gets
  `DWMWA_COLOR_DEFAULT`, `DWMWCP_DEFAULT` and `DWMSBT_AUTO` back for every attribute the theme set.
  Windows closed in the meantime are skipped, and the allowlist is checked again before a window is
  written to, in case its handle was reused.
- **Registry**: before the first write, the original of every value (including "absent", and its
  exact type: `REG_DWORD`, `REG_BINARY`, `REG_SZ`, `REG_EXPAND_SZ`) is saved to
  `%LOCALAPPDATA%\glaze\undo.json`, written atomically. Registry keys the styling had to create are
  recorded too and removed again. Later style and palette updates keep the first originals for
  fields that remain managed. Transitions diff against the registry, restore originals only of
  released values, and send one broadcast if anything changed (none for an unchanged registry).
  Released entries remain journaled through the broadcast and settle passes; a partial write or
  interrupted release keeps enough information for crash recovery, including the committed and
  pending ownership added in commit `7234e88`. Reacquiring a released field journals its then-current
  original. Restoring writes the originals back (deleting what was absent), announces the change,
  then re-checks and rewrites any value Windows changed in reaction (it keeps the DWM accent in sync
  with `AccentColorMenu`), so the registry ends up as it was. The journal is deleted after that.
- **When it runs**: when all Windows styling is
  released (no selected native style, theme styling or explicit Windows fields), Quit, the
  panic hotkey, Ctrl+C and normal exit. Core's own drop does it too, so error exits are covered.
  A theme without a `windows` section still follows a selected native style's defaults.
- **After a crash or kill**: the journal is still there, and core's next start undoes it before doing
  anything else. Windows opened or styled by the dead run are reset to defaults too. Their original
  dark-mode value is lost, so if the dead run forced dark mode, windows get the system's current
  setting (Windows' own apps follow it). A second instance never undoes a running instance's journal.
- **By hand**: if something is ever stuck, quit Glaze, set the values in the table above back (or
  just pick your accent again in Settings), and delete `undo.json`.

## Behavior notes and limits

- Styling follows the look and theme in `config.json`, and the `windows` section is re-read
  whenever the wallpaper reloads its theme, so editing `theme.json` applies live.
- While a theme with `accent` or `dark_mode` is active, changing those in Settings by hand is
  overwritten the next time the theme changes colors, and undone at restore.
- Apps that set their own title bar color or backdrop may override ours on their next paint.
- Corners are a DWM **preference**, not a guaranteed shape. On this machine, cross-process writes
  to Explorer and Settings returned `S_OK` and read back square/round/small correctly, without
  injection. Production checks the corner HRESULT and reads successful writes back; failures or
  mismatches are logged and are not remembered as successful applications. Actual visible rounding
  remains unverified in this check because another window covered both target windows. Microsoft
  also excludes maximized/snapped windows and some custom frames from rounding; see
  [rounded-corner guidance](https://learn.microsoft.com/en-us/windows/apps/desktop/modernize/ui/apply-rounded-corners).
  Suggested Studio wording: **Corner preference** — “Supported Windows frames only. Apps may
  override this; maximized and snapped windows stay square.” No Studio files were changed.
- Restyling the *inside* of File Explorer and Settings is the experimental [app styler](#app-styler-experimental)
  below; everything above is DWM and registry only.

## Direct-transition verification (2026-11-06)

The isolated [timing fixture](../crates/tweaks/examples/measure_style_transitions.rs) uses the real
style resolver, transition plans, on-disk undo journal and latest-request worker. Its registry is
in memory; it models two windows and two 20 ms settings receivers. The legacy path preserves the
old deactivate/activate ordering and 100 ms restore settle. Medians of three samples:

| Case | Config write → last fixture window settled, before → after | Main-thread blocked, before → after | Broadcasts | Attribute writes per window | Registry mutations |
|---|---:|---:|---:|---:|---:|
| Light → dark | 185.78 → 89.56 ms | 185.49 → 0.67 ms | 2 → 1 | 12 → 3 | 16 → 3 |
| Dark → light | 185.81 → 88.97 ms | 185.53 → 0.61 ms | 2 → 1 | 12 → 3 | 16 → 3 |
| One palette update | 43.74 → 88.33 ms | 43.44 → 0.70 ms | 1 → 1 | 6 → 1 | 4 → 4 |
| 20 palette events, 2 ms apart | 935.04 → 153.61 ms | 876.73 → 13.57 ms total | 20 → 1 | 120 → 1 | 80 → 4 |

Raw samples: [before](../crates/tweaks/evidence/style-sync/transition-before.json),
[after](../crates/tweaks/evidence/style-sync/transition-after.json). These measure fixture completion,
not Explorer/Settings paint latency, file-watcher scheduling or real signature IO. Main-thread time
includes reading/parsing fixture config and submitting the request. One isolated palette change
takes longer to settle because of the 35 ms debounce and Windows timer granularity; a drag avoids
replaying intermediate palettes. An already-running transition finishes, then only the newest
pending request runs. Shutdown joins the worker so final restoration finishes before core exits;
startup waits for crash recovery. Those waits are outside ordinary config-event handling.

64 tweaks tests pass, including A→B/B→A write/reset plans, palette-only diffs, identical no-ops,
each interrupted write/delete boundary, a Windows reaction during release, PID reuse, and requests
arriving while a receiver is blocked. Core's 19 tests pass with four existing desktop/Task Scheduler
tests ignored. Core checks and strict all-target tweaks Clippy pass.

The [corner probe](../crates/tweaks/evidence/style-sync/probe-corners.ps1) checks HRESULT/readback and
restores each HWND in `finally`; [results](../crates/tweaks/evidence/style-sync/corner-probe.json)
show both originals restored. Config SHA-256 stayed
`924298ABF9167B58970BFD18FDE315822CE6BD6D72B813026BBC265C1C086E6A`; focus did not change. No real
config or light/dark preference was written. Screen crops were inspected but showed the covering
window, so they are not presented as evidence of the target corners. Deployment and an uncovered
pixel-level corner check remain for the parent after the shared Studio merge.

## Earlier style-sync verification (2026-11-06)

`CARGO_TARGET_DIR=C:\glaze-targets\style-sync`. Config 21, theme 21 and tweaks 43 library tests
passed; core 16 passed with four existing Task Scheduler/tray tests ignored. Strict Clippy passed
for all config/theme/tweaks targets, and core/app-styler checks and the release workspace build
passed. Pure tests cover all five styles in both schemes, the absent/null/value layers, no palette,
mono's automatic accent, both independent accent switches, identical-event no-ops, and recovery
of first registry originals after ten style/scheme transitions and a palette update.

The live probe followed the real core's config/palette watches, rather than applying writes
itself. [Ten cases](../crates/tweaks/evidence/style-sync/) exercised five dark styles, Liquid light,
explicit Windows overrides, Mono light-to-auto, a palette update and releasing styling. Every
case compared all eight registry values (exact type/bytes/absence) with its pre-case state and
restored config and palette bytes. All ten config before/after SHA-256 values were:
`F4AF26AEC8255D043312EBF4C0A962868D6BC0C267C616371C9CE56F3AD219B9`.
The neighbouring app-styler soak already had its flag and glass preset enabled; these probes
preserved those settings, including the enabled file index, and did not enable an optional feature.
Their owner controls the final soak restoration.

- [Explicit overrides](../crates/tweaks/evidence/style-sync/explicit.json) selected a fixed accent,
  border/caption/text, square corners, no backdrop, light apps despite dark shared appearance,
  and independent accent visibility. The [app token](../crates/tweaks/evidence/style-sync/explicit-app-tokens.json)
  was the same `#c04080` as Windows.
- [Automatic Mono](../crates/tweaks/evidence/style-sync/auto-return.json) released the forced light
  mode, restored the original dark preference and supplied the [same `#f2f2f2` token](../crates/tweaks/evidence/style-sync/auto-app-tokens.json)
  to the helper, without depending on which process handled the config event first.
- The [palette event](../crates/tweaks/evidence/style-sync/palette-changed.json) changed the Windows
  accent to `#d06030`; the [helper's real rules file](../crates/tweaks/evidence/style-sync/palette-app-tokens.json)
  contained that same accent and `#80223344` for `$dominant/80`.
- [Releasing styling](../crates/tweaks/evidence/style-sync/release.json) removed `undo.json`, restored
  every journal original and returned Explorer's corner/backdrop attributes to default. Restoring
  the config re-applied the user's explicit acrylic backdrop and null border.

Verified visually: [native frame captions](../crates/tweaks/evidence/style-sync/native-frames.png),
actual Settings in [dark](../crates/tweaks/evidence/style-sync/liquid-dark-settings.png) and
[light](../crates/tweaks/evidence/style-sync/liquid-light-settings.png), plus Explorer's
[dark](../crates/tweaks/evidence/style-sync/liquid-dark-explorer-header.png) and
[light](../crates/tweaks/evidence/style-sync/liquid-light-explorer-header.png) chrome. Captions had
9,159–9,196 pixels matching their expected RGB within two channel values. Native reference windows
were owned, nonactivating and closed after capture; no clicks/keys, user-window movement or focus
changes were used. Full captures and their [hashes](../crates/tweaks/evidence/style-sync/full-capture-hashes.json)
are retained in `%LOCALAPPDATA%\Temp\glaze-tools\style-sync-full`; only compact crops are committed.

DWM's [colour attributes](https://learn.microsoft.com/en-us/windows/win32/api/dwmapi/ne-dwmapi-dwmwindowattribute)
are documented for setting, and their getters returned no colour on this build, so colour paint
was checked on owned standard frames. Explorer's queryable corner/backdrop/dark attributes followed
the styles. Settings sometimes repainted its own backdrop to none; a custom app header can retain
its own fill. The screenshots do not claim that DWM replaces such client rendering. Both
`ColorPrevalence` registry mappings follow Microsoft's [settings reference](https://learn.microsoft.com/en-us/windows/apps/develop/settings/settings-common);
the light-taskbar limit follows [Windows colour settings](https://support.microsoft.com/en-us/windows/experience/personalization/personalize-your-colors-in-windows).

| Check / budget | Before | After |
|---|---:|---:|
| Core CPU, % of one core, prescribed 30-second sampler | 0 | 0.05 |
| Core private memory | 3 MB | 3 MB |
| Extra styling process / worker / timer / polling loop | 0 | 0 |
| Exact config/palette + eight-value registry restoration | — | 10/10 cases |

The [before](../crates/tweaks/evidence/style-sync/measure-before.json) and
[after](../crates/tweaks/evidence/style-sync/measure-after.json) samples were on the shared live
desktop with other agents, indexing and the app-styler soak active. The after core sample corresponds
to about 15 ms CPU in 30 seconds; this is not an isolated proof of literal 0% idle or a performance
improvement. No new periodic work was introduced; resolution uses the existing events, and
private memory held. Other processes' changes in those samples are outside this slice.

Reproduce using `inspect_style_sync --check` (the read-only example), then
`evidence/style-sync/run.ps1 -Case <case>` under the shared desktop-lock rules, at least 30 seconds
between cases. `frame.ps1` handles the set-only colour paint check; `contact.ps1` compacts captures.
The fixture uses `File.Replace` for atomic config updates. Its initial PowerShell `Move-Item -Force`
replacement could expose a missing-file gap; the corrected final Mono light-to-auto run kept
the same helper PID (16688) throughout and restored all bytes/values again. No product test
failure remained.

The only implementation edit outside theme/config/tweaks was the small core styling call site.
The app-styler owner made its corresponding helper call-site edits. Studio was not edited: expose
the two new tri-state Windows accent-visibility fields in its Look/Windows editor; the selected
style already drives the synchronization.

For the next consumers: `glaze_theme::windows::Palette { dominant: Color, accent: Color }`,
`resolve(&Look, Option<&Theme>, Option<Palette>, system_dark: bool) -> Resolved`, where
`Resolved { windows: Option<WindowsStyle>, accent: Option<Color>, dominant: Option<Color> }`.
`WindowsStyle` has `accent_on_taskbar` / `accent_on_title_bars: Option<bool>`;
`look::WindowsLayer` uses `Option<Option<bool>>` for each to retain absent/null/value.
Core uses `Tweaks::unmanaged_dark_mode() -> bool`; the helper uses the stateless
`unmanaged_system_prefers_dark() -> bool` to read the same journal original. The underlying
`Undo::original(key: &str, name: &str) -> Result<Option<Option<Value>>>` distinguishes no journal
entry from an originally absent value. No new messages: core still handles `WM_STYLE_CONFIG`
and `WM_STYLE_PALETTE`, and the app helper uses its existing config/palette watch events.

## App styler (experimental)

Restyles the XAML inside **File Explorer** and **Settings**: backgrounds, corners, padding, fonts,
sizes, any dependency property, by rules. It runs our own code inside those two apps, so it is
opt-in and off by default:

```json
{ "experimental": { "app_styler": true }, "system": { "separate_process": true } }
```

Explorer is styled only with `system.separate_process` ([system.md](system.md)): folder windows
then open in their own Explorer process, which exits about a minute after its last window
closes (the helper lets go of it then, or it wouldn't). The shell's Explorer (desktop, taskbar, and any folder window that lives in it) is never
attached; see "Only processes that exit" below for why. Without `separate_process` the helper
logs one line (`not styling explorer: it needs system.separate_process = true ...`) and leaves
Explorer alone.

Settings is **off unless asked for** with `"look": { "apps": { "settings": { "enabled": true } } }`:
on Windows 11 26100, Settings ends itself on Personalization › Background (and Lock screen)
whenever XAML Diagnostics is attached, with or without rules (see "Settings and Personalization ›
Background" below). `"enabled": false` under `look.apps.explorer` switches Explorer off the same way. A
theme's `apps` can set `enabled` too; the user's wins. A switched-off app's live sessions put it
back and detach, and it comes back when the switch does.

Off costs nothing: no process, no hook, nothing loaded anywhere. On, core runs `glaze-styler`
(`--parent <core's pid>`); turning the flag off, quitting Glaze, the panic hotkey, core ending
for any reason (a crash included: the helper exits with it) or the helper dying for any reason
puts both apps back as they were. A helper started while the previous one is still exiting waits
up to 10 s for it. Studio has no switch for it yet (Experimental section, later).

### How it works

- It uses the documented **XAML Diagnostics** API, the one Visual Studio's Live Visual Tree uses.
  The helper calls `InitializeXamlDiagnosticsEx` for the target process. Windows then loads our TAP
  DLL (`glaze_app_styler.dll`) there and hands it `IXamlDiagnostics` and `IVisualTreeService3`.
  The TAP subscribes to the visual tree. Every element that appears (the existing tree first, then
  new windows, pages and re-created elements) is matched against the rules on its own UI thread
  and styled there. No polling.
- **Which processes**: only `explorer.exe` and `ImmersiveControlPanel\SystemSettings.exe`, and
  only if the injection allowlist ([`crates/inject`](../crates/inject/src/lib.rs), with the
  Microsoft-signature check) also accepts the process. The helper opens the process first and
  checks that very process, then keeps the handle open through the attach: a pid can't be given
  to another process while a handle to it is open, so the attach can't reach a different app.
  Its exit is watched from before the TAP loads (a crash while it loads counts). The TAP copy is
  used only if its bytes are still this build's. Settings runs with the inbox package identity
  `windows.immersivecontrolpanel_cw5n1h2txyewy`, which the allowlist names. Of Explorer's
  processes only a folder process is attached, never the shell's: the one that owns the shell
  window (`GetShellWindow`, the desktop's `Progman`) or a taskbar (`Shell_TrayWnd`,
  `Shell_SecondaryTrayWnd`). The helper logs the pid it refuses (`explorer (PID) is the shell's
  Explorer: never styled`), and while there is no shell window at all (Explorer restarting) it
  attaches to no Explorer. `--attach` and `--dump` refuse the shell's Explorer too.
- **Which framework**: Settings is system XAML (`Windows.UI.Xaml`). Explorer's folder windows are
  WinUI 3 (`Microsoft.UI.Xaml` from the Windows App SDK). The helper attaches through that SDK's
  `Microsoft.Internal.FrameworkUdk.dll`, loaded from the same folder Explorer uses, when the
  first folder window appears. Explorer's *system* XAML is the taskbar, which the styler never
  attaches to.
- **Rules reach the TAP** as one file per app, `%LOCALAPPDATA%\glaze\app-styler\<app>.json`.
  The helper rewrites it when `config.json`, the theme or the wallpaper's palette changes. The TAP
  watches that folder, so a change applies live without restarting either app. Elements no rule
  wants any more get their values back.
- **The DLL apps load** is a copy named by its content hash (`tap-<hash>.dll`, same folder): an app
  keeps a loaded TAP until it exits, so a new build loads beside the old one, and deploying never
  fights a locked file. Copies no process has mapped any more are deleted.
- **Combine with the DWM layer above**: Settings and Explorer's header already sit on Mica.
  `look.windows.backdrop: "acrylic"` (or `"mica"`) and rules that make backgrounds `Transparent`
  let that backdrop show through.

### Rules

A theme's `theme.json` › `apps` is the layer below the user's `config.json` › `look.apps`, field by
field, like `windows`. Both have the same shape:

```json
{
  "preset": "glass",
  "settings": {
    "rules": {
      "Grid#RootPageGrid": { "Background": "Transparent" },
      "ButtonEntityItem > Button#ContainerButton": { "CornerRadius": "12", "Padding": null }
    }
  },
  "explorer": {
    "rules": {
      "Grid#NavigationBarControlGrid": { "Background": "$accent/18" },
      "TabViewItem Path#SelectedBackgroundPath": { "Fill": "{ThemeResource ControlFillColorSecondaryBrush}" }
    },
    "resources": { "ControlFillColorDefaultBrush": "light-dark(#66FFFFFF, #0AFFFFFF)" }
  }
}
```

- **Layers**: preset < theme < user. Each property of each selector is its own field: a value set
  in a higher layer wins, and `null` takes a lower layer's value away (the property isn't set).
  `"preset": "none"` drops the preset.
- **Selectors** pick elements by type, `x:Name` and ancestry, CSS-style:

  | Selector | Matches |
  |---|---|
  | `Grid` | type `Grid` by its last segment (system XAML and WinUI 3 alike), or a full name like `Microsoft.UI.Xaml.Controls.Grid` |
  | `#RootGrid`, `Grid#RootGrid` | by `x:Name`, with or without a type |
  | `*` | anything |
  | `A > B` | `B` whose parent is `A` |
  | `A B` | `B` anywhere below `A` |

  When several rules set the same property on one element, the more specific selector wins (names,
  then types, then steps).
- **Properties** are dependency property names as XAML writes them (`Background`, `Foreground`,
  `CornerRadius`, `Padding`, `Margin`, `FontFamily`, `FontSize`, `Opacity`, `Visibility`,
  `BorderBrush`, `BorderThickness`, `MinHeight`, `Fill`...). An element without the property is
  skipped and logged once per type.
- **Values** are XAML text for the property's type: `"#80202020"`, `"Transparent"`, `"12"`,
  `"12,4,12,4"`, `"Segoe UI Variable Display"`, `"Collapsed"`. `$accent` and `$dominant` are the
  look's accent (or the wallpaper's) and the wallpaper's dominant colour; `$accent/40` adds a hex
  alpha. A token with no colour yet leaves its property unset. `{ThemeResource Key}` (or
  `{StaticResource Key}`) points the property at an app resource instead, resolved from the
  element's place in the tree, so it follows light/dark and the `resources` below.
- **`resources`** override the app's theme **brushes** by key (`ControlFillColorDefaultBrush`,
  `ControlFillColorSecondaryBrush` (hover), `ControlFillColorTertiaryBrush` (pressed),
  `SubtleFillColor*`, `CardBackgroundFillColor*`, `AccentFillColor*`, `TextFillColor*`, `TabView*`
  brushes...). Every control that uses a theme brush shares one brush object per theme; the
  override sets that object's colour, so controls' own templates, hover and pressed included,
  change live. `light-dark(LIGHT, DARK)` gives each theme its own colour; a plain colour is used
  for both. Two keys that are aliases of one brush change together (the first override wins), so
  check both themes before shipping one (Explorer's nav row shares a light-theme brush with the
  `ControlFillColor*` keys). Value resources (`ControlCornerRadius`, thicknesses) are **not**
  overridden: XAML can replace them in the dictionary but existing controls don't follow and the
  original can't be put back, so set `CornerRadius` etc. with a rule on the controls instead.

### Presets

Data in [`crates/app-styler/presets`](../crates/app-styler/presets):

| Preset | Settings | Explorer |
|---|---|---|
| `calm` | borderless 6 px cards and search box, flatter fills, a mono accent (grey toggles, checks and selection mark) | no lines around the command bar, 6 px address box, borderless search box |
| `glass` | borderless 10 px cards, a pill search box, softer translucent fills per theme, `$accent` selection mark | tabs and nav row on the window backdrop, a soft selected tab, pill address and search boxes, softer fills, transparent details pane |
| `compact` | 44 px cards, 30 px nav rows, 13 px nav text | 28 px command and nav buttons, 28 px address and search boxes, 13 px breadcrumb |

Before/after captures of each preset, light and dark: `C:\glaze-targets\app-styler\round2\shots`
(on the dev machine). Explorer's command bar row and file list are painted by Explorer's own
window behind the XAML, so they stay solid in `glass`.

### Undo and the crash guard

- Every value is recorded before it is changed: the element's own (local) value, or "none". It
  goes back when a rule stops wanting it, when the element leaves the tree (it may come back), and
  when the helper exits. Brush overrides are recorded and put back the same way.
- **Handles are not references.** XAML Diagnostics' handle table doesn't keep objects alive: a
  value type's handle (a colour, a number) is good for one `SetProperty`, and an object's handle
  can go stale and later name a different object. Using such a handle crashed Explorer in testing
  (an access violation in `Microsoft.UI.Xaml.dll` seconds after detaching). So the TAP holds a
  strong reference to every object it keeps (originals, its shared rule brushes, its scratch
  element) and asks for a fresh handle right before using one, keeps a value type's original as
  text and makes it again right before putting it back, and releases what it holds on the
  object's own UI thread (a thread that is gone leaks it instead).
- **Work for a UI thread carries no handles.** A restyle or clean-up is posted to the thread's
  message window as a job that finds its elements in the TAP's map when it runs, on that thread,
  where the tree callbacks keep the map true; an element is held while it is styled or put back.
  A job that arrives while the thread is inside our own work (XAML calls can pump messages) waits
  until that work is done. Each message window has a generation, checked before posting and when
  the job runs, so a closed window whose handle or thread id comes back never runs an old job;
  and a job only goes to a window whose procedure is this TAP build's (every build an app has
  loaded registers the same class name, so the class alone doesn't say whose window it is).
- **Closing**: when the helper goes, styling stops and each UI thread gets a clean-up job, queued
  behind whatever it is doing; it puts back every value and brush on that thread and lets go of
  what the TAP holds there. The TAP stops following the tree only after every thread has cleaned
  up (at most 10 s; a thread that doesn't answer leaks what is held there instead).
- **Nothing piles up**: the element strings each tree callback is handed are freed (XAML gives
  them to the callback), rule values no element uses any more are released after each restyle,
  and a UI thread whose window closed (its last element left) is retired: its overrides go back
  and its values and message window go. Explorer often closes a window but keeps its thread, and
  that tree goes without a removal: when the next window or popup opens on the thread, its old
  roots that no longer resolve are forgotten (there is nothing left to put back).
- **Crash guard**: if a styled app exits with an NTSTATUS-style code (`>= 0x80000000`: a crash, or
  being ended with `TerminateProcess(-1)` as `Stop-Process` and Task Manager's *End task* do)
  within 60 s of being attached, that counts once. Deaths of one app within 5 s of each other are
  one incident (an Explorer restart ends every Explorer process at once). After two incidents, the
  helper stops styling that app for the session and logs it (`not styling settings again this
  session`): it stops writing that app's rules and writes a stop marker (`<app>.off`) beside
  them, and every TAP session of that app still running (Explorer can have several processes)
  puts it back and detaches. A fresh helper session removes the markers and starts clean.
  Exercised 2026-11-06 with Settings, and (round 3) with two separate Explorer folder processes:
  the shell's Explorer, which survived, put its look back and detached. A process the allowlist
  rejects is never attached.
- The helper exits on its own only if `experimental.app_styler` reads off twice, a second apart:
  a config writer that replaces the file non-atomically briefly shows no file (defaults, off).
  Core stops the helper when the flag really goes off.
- **Soak, 2026-11-06** (glass, live desktop, about 45 min with the flag on, 29 of them scripted):
  Explorer windows and tabs opened, navigated (folders, Home, Gallery) and closed, Settings pages,
  two Explorer restarts, several Glaze redeploys and flag off/on cycles. No Explorer or Settings
  crash, no guard trip, bars and wallpaper back after each restart. Helper: 2.9–3.3 MB private,
  47 ms CPU in 120 s idle (0.04 % of one core; it wakes only for window-show events).
- **Known issue (round 3, 2026-11-06): Explorer keeps memory while attached.** With a session
  subscribed, each burst of two folder windows opened, navigated and closed (plus Settings pages)
  left 120–410 MB in Explorer's private bytes (754 MB to 2.4 GB over 7 bursts; handles 6.2k to
  14.6k). It is the subscription itself: with the TAP attached and **no rules** (no styling, no
  property reads) a burst still added 127–151 MB; with the flag off, 8–17 MB (3 bursts each way).
  Ending the session doesn't reliably give it back (once 2.5 to 1.3 GB within 10 s, once
  nothing); restarting Explorer does. The TAP's own map stays flat (it follows removals and drops
  trees that went without one). Evidence: `crates/app-styler/evidence/round3`.
- **What holds it (round 4, from WinUI's source).** XAML Diagnostics' `RuntimeObjectCache` keeps a
  strong reference to every parentless object it has seen, until that object is closed: every
  tree root, and every object outside the tree we ask a handle for. Explorer's island hosts
  (`DesktopWindowXamlSource`) add themselves as roots but on close only their inner `XamlIsland`
  signals the removal, so a closed window's root, and its tree, is never let go of. Only
  destroying XAML Diagnostics empties the cache; unadvising doesn't.
- **Recycling doesn't work (round 4).** Ending XAML Diagnostics after windows close and attaching
  a new session (handing the look over) kept Explorer's memory at the flag-off control, but
  **hung Explorer** after 13-14 recycles in all three runs: its UI thread spun in a corrupted
  property map (`DependencyObject::GetValue`). The teardown releases the UI threads' XAML
  objects from whichever thread lets go last, and in the shell Explorer every window has its own
  UI thread, so no thread can do it safely. Visual Studio never lets go of XAML Diagnostics; the
  code was reverted. The fix belongs in WinUI (close the root a `DesktopWindowXamlSource` added);
  the TAP can drop the per-handle part (no handles for objects outside the tree). Evidence and
  the burst table: `crates/app-styler/evidence/round4`.
- **Only processes that exit (round 5).** Since an attached process keeps every tree until it
  exits, the styler only attaches to processes that exit on their own: Settings
  (`SystemSettings.exe` exits when its window closes) and Explorer's separate folder process
  (exits after its last folder window). The shell's Explorer runs all session, so it is never
  attached. A subscribed folder process doesn't exit on its own, though: the trees XAML
  Diagnostics keeps hold the process too (styled, 3 of 3 lived on without a window; unstyled
  ones exit 45–57 s after their last window). So the helper **lets go of a folder process when
  its last folder window closes**: a destroy hook per attached process spots it, a per-process
  stop marker (`explorer.<pid>.off`) makes that process's TAP put everything back and detach, and
  the process then exits like an unstyled one (50–52 s; log: `explorer (PID) has no folder
  window left: letting go so it can exit`). That pid is never attached again: a window that
  still opens in it before it exits stays stock. Unlike round 4's recycling, no live XAML is left
  in the process to disturb. On this build every `explorer.exe <folder>` launch gets its own
  folder process (two windows opened a second apart: two processes). With that, styled folder
  processes exited 49.5–52.1 s after their windows closed (6 of 6), and the shell Explorer moved
  as much as with nothing attached (+11 MB over 3 bursts, control +10 MB).
- **Known issue (round 5): styled folder windows can hang.** Two of 15 styled folder processes
  in the bursts hung within seconds of attaching, their UI thread spinning at a full core in
  WinUI's `ItemsView::GetElementIndex` (a walk up the tree that never ends) from a `GettingFocus`
  raised by `FileExplorerFolderViewController::SetInnerFocus`; one was on Gallery. No TAP code on
  the stack; none of 13 unstyled ones hung. Windows opened straight on Gallery didn't reproduce
  it (glass 0 of 5, attached with no rules 0 of 5, nothing attached 0 of 5), so whether it needs
  a rule or only XAML Diagnostics is still open. The flag stays **off** until that is understood:
  a hung folder window has to be ended from Task Manager.
- **Settings and Personalization › Background (round 5).** With XAML Diagnostics attached,
  Settings fails fast when that page opens: a stowed `E_INVALIDARG` ("The parameter is
  incorrect") from `CDependencyObject::SetEffectiveValue` while a control template applies a
  `TemplateBinding` to a `Border` during layout (`Windows.UI.Xaml.dll` 10.0.26100.9711,
  `propertysystem.cpp` line 1625; no TAP code on the stack). It happens attached with **no
  rules** too (3 of 3), and never without the styler (2 of 2), so it is XAML Diagnostics on that
  page, not a style. Nothing the TAP can do avoids it, so Settings is off unless
  `look.apps.settings.enabled` is `true`; the crash guard stops styling Settings after two such
  exits. Personalization › Lock screen does the same; Themes, Start, Fonts, Colours, Display,
  About, Network, Bluetooth, Sound, Taskbar and Privacy don't. Evidence:
  `crates/app-styler/evidence/round5`.
- Missing elements across Windows builds are not errors: a selector that matches nothing does
  nothing.
- Nothing is persisted in the apps. If anything is ever stuck, closing and reopening the window
  (or restarting Explorer) shows the stock look again.

### What can and can't be styled

- **Settings**: the whole window is XAML: title bar area, search box, navigation pane, breadcrumb,
  every page and card.
- **File Explorer** (folder windows): tabs (`TabView#TabView`; the selected tab's shape is
  `TabViewItem Path#SelectedBackgroundPath`), the navigation bar (`Grid#NavigationBarControlGrid`
  with back/forward/up/refresh; the address box that's drawn is
  `AddressBarControl > Grid#PART_LayoutRoot`, the search box
  `AutoSuggestBox#FileExplorerSearchBox`), the command bar row (`CommandBarControl_Wave1 > Grid`,
  with its top and bottom lines, and `CommandBar#FileExplorerCommandBar`) and the details pane
  (`Grid#DetailsViewControlRootGrid`). Home and Gallery are XAML too.
- **Not reachable**: the file list (DirectUI `DirectUIHWND`), the navigation tree (a Win32 tree
  view) and the status bar are not XAML, so XAML Diagnostics can't see them. They need a different
  mechanism (theme classes / `SetWindowTheme` inside the process): a later stretch.
- **Visual states win over local values**: hover and pressed looks come from template
  storyboards and theme brushes. A local `Background` shows at rest; to change hover and pressed,
  override the brushes with `resources`.

### Tools

```text
glaze-styler --dump PID [--winui] [--out FILE] [--props "SEL; SEL"]
                                                    the live XAML tree, to write selectors
glaze-styler --preview glass                      run with a preset, whatever the flag says
```

`--dump` attaches with no rules and writes an indented outline (`Type#Name`, with what is styled)
to `%LOCALAPPDATA%\glaze\app-styler\tree-<pid>.txt`. `--props` adds every property value of the
elements the selectors (`;`-separated) pick, with where each comes from (local, style, template,
visual state) and brushes' colours, so a rule can target what is actually drawn. Use `--winui` for
Explorer's folder process. The TAP's own log is `%LOCALAPPDATA%\glaze\app-styler\styler.log`.

XAML Diagnostics silently drops initialization data much longer than 256 characters (the TAP is
then never created), so the helper passes one folder and short file names.
