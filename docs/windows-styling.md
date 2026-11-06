# Native Windows styling

A native look can restyle Windows itself while it is active: window borders, title bars, corners,
backdrop (Mica and friends), the system accent color, light/dark mode and accent visibility.
Selecting `config.json` > `look.style` supplies Windows defaults as well as the native shell's.
A wallpaper can refine those defaults with its `windows` section. Everything uses supported public
APIs (`DwmSetWindowAttribute` and the same per-user registry values the Settings app writes). No code
runs inside any other process, and everything is undone when the theme changes, mypaper quits, the
panic hotkey fires, or core restarts after a crash.

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

[`themes/midnight`](../themes/midnight) is a working example.

**Your look wins.** A theme's `windows` section is a suggestion: `config.json` › `look.windows`
(edited in Studio › Look › Windows) is your layer over it, field by field. A field absent there
follows the theme or native style; `null` keeps Windows' own setting ("As is"); a value sets it. The fields and
values are the ones below. So styling Windows needs no copy of a built-in theme, and it survives a
wallpaper change. See [studio-ux.md](studio-ux.md#6-look-and-windows-styling-as-part-of-it-d2).

## One switch for the desktop

```json
{ "look": { "style": "fluent", "appearance": "dark", "accent": "auto" } }
```

The pure resolver is [`mypaper_theme::windows::resolve`](../crates/theme/src/windows.rs).
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
The journal's original scheme prevents a previous mypaper override feeding back into auto.
In `look.windows`, `apps: light/dark/match/as-is` is the higher-priority mode control (`dark_mode`
is the theme's field). `match` follows this same appearance resolution; `as-is` releases mode.

For example, `look.windows: { "border": null, "accent_on_taskbar": false }` leaves borders to
Windows and disables accent on Start/taskbar while the other fields follow the selected style.
Both accent-visibility fields accept absent, null, true and false independently. Windows only
supports a coloured Start/taskbar in dark system mode, so light/automatic-light defaults keep
that switch off. Explicit overrides are still honoured.

Resolution runs only on the existing config/theme/palette events: no extra process, hook,
thread, timer or polling. The experimental app styler uses the same resolved `$accent` (including
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
| `cursors` | `"mypaper-dark"`, `"mypaper-light"`, a set name, or a folder | The mouse cursor scheme (`HKCU\Control Panel\Cursors` + `SPI_SETCURSORS`), journaled like the accent; see [desktop-extras.md](desktop-extras.md#cursor-themes) |

Border, caption, corner and backdrop attributes need Windows 11 (build 22000, and 22621 for
`backdrop`); older builds ignore them. Dark title bars need Windows 10 20H1 or later. Whether a
window *shows* a backdrop is up to the app: Mica always covers the title bar, and shows through the
client area only where the app draws nothing opaque.

## Which windows

A window is styled only if all of this holds:

- It is top-level, visible and has a title bar.
- Its process passes the injection allowlist in [`crates/inject`](../crates/inject/src/lib.rs)
  (`mypaper_inject::evaluate`): Explorer, Settings, the frames around UWP apps, a few inbox apps,
  never games or third-party apps. The inputs are the image path
  (`QueryFullProcessImageNameW`), the package family name (`GetPackageFamilyName`) and whether the
  image is Microsoft-signed.
- The signature check, [`signature::microsoft_signed`](../crates/inject/src/signature.rs), is
  `WinVerifyTrust` on the file, falling back to the catalog (`CryptCATAdmin*`) lookup that covers most
  Windows binaries. The signer chain must also be Microsoft's (leaf `Microsoft…`, root
  `Microsoft Root…`), so a validly signed third-party file doesn't pass. It works offline, and
  verdicts are cached per path.
- The expensive signature check only runs for processes the allowlist would accept anyway, so a game
  opening a window never gets its executable hashed.

Existing windows are styled when the theme activates (`EnumWindows`). New windows are caught by an
out-of-context `EVENT_OBJECT_SHOW` WinEvent hook on core's main thread, so nothing polls.

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
3. The result is written atomically to `%LOCALAPPDATA%\mypaper\palette.json`:

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
  `%LOCALAPPDATA%\mypaper\undo.json`, written atomically. Registry keys the styling had to create are
  recorded too and removed again. Later palette updates keep the first
  originals. Restoring writes the originals back (deleting what was absent), announces the change,
  then re-checks and rewrites any value Windows changed in reaction (it keeps the DWM accent in sync
  with `AccentColorMenu`), so the registry ends up as it was. The journal is deleted after that.
- **When it runs**: before applying a different resolved style, when all Windows styling is
  released (no selected native style, theme styling or explicit Windows fields), Quit, the
  panic hotkey, Ctrl+C and normal exit. Core's own drop does it too, so error exits are covered.
  A theme without a `windows` section still follows a selected native style's defaults.
- **After a crash or kill**: the journal is still there, and core's next start undoes it before doing
  anything else. Windows opened or styled by the dead run are reset to defaults too. Their original
  dark-mode value is lost, so if the dead run forced dark mode, windows get the system's current
  setting (Windows' own apps follow it). A second instance never undoes a running instance's journal.
- **By hand**: if something is ever stuck, quit mypaper, set the values in the table above back (or
  just pick your accent again in Settings), and delete `undo.json`.

## Behavior notes and limits

- Styling follows the look and theme in `config.json`, and the `windows` section is re-read
  whenever the wallpaper reloads its theme, so editing `theme.json` applies live.
- While a theme with `accent` or `dark_mode` is active, changing those in Settings by hand is
  overwritten the next time the theme changes colors, and undone at restore.
- Apps that set their own title bar color or backdrop may override ours on their next paint.
- Restyling the *inside* of File Explorer and Settings is the experimental [app styler](#app-styler-experimental)
  below; everything above is DWM and registry only.

## Verification (2026-11-06)

`CARGO_TARGET_DIR=C:\mypaper-targets\style-sync`. Config 21, theme 21 and tweaks 43 library tests
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
are retained in `C:\Users\you\AppData\Local\Temp\mypaper-tools\style-sync-full`; only compact crops are committed.

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

For the next consumers: `mypaper_theme::windows::Palette { dominant: Color, accent: Color }`,
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
{ "experimental": { "app_styler": true } }
```

Off costs nothing: no process, no hook, nothing loaded anywhere. On, core runs `mypaper-styler`;
turning the flag off, quitting mypaper, the panic hotkey or the helper dying for any reason puts
both apps back as they were. Studio has no switch for it yet (Experimental section, later).

### How it works

- It uses the documented **XAML Diagnostics** API, the one Visual Studio's Live Visual Tree uses.
  The helper calls `InitializeXamlDiagnosticsEx` for the target process. Windows then loads our TAP
  DLL (`mypaper_app_styler.dll`) there and hands it `IXamlDiagnostics` and `IVisualTreeService3`.
  The TAP subscribes to the visual tree. Every element that appears (the existing tree first, then
  new windows, pages and re-created elements) is matched against the rules on its own UI thread
  and styled there. No polling.
- **Which processes**: only `explorer.exe` and `ImmersiveControlPanel\SystemSettings.exe`, and
  only if the injection allowlist ([`crates/inject`](../crates/inject/src/lib.rs), with the
  Microsoft-signature check) also accepts the process. That check runs again right before every
  attach. Settings runs with the inbox package identity
  `windows.immersivecontrolpanel_cw5n1h2txyewy`, which the allowlist names.
- **Which framework**: Settings is system XAML (`Windows.UI.Xaml`). Explorer's folder windows are
  WinUI 3 (`Microsoft.UI.Xaml` from the Windows App SDK). The helper attaches through that SDK's
  `Microsoft.Internal.FrameworkUdk.dll`, loaded from the same folder Explorer uses, when the
  first folder window appears. Explorer's *system* XAML is the taskbar, which the styler never
  attaches to.
- **Rules reach the TAP** as one file per app, `%LOCALAPPDATA%\mypaper\app-styler\<app>.json`.
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

Before/after captures of each preset, light and dark: `C:\mypaper-targets\app-styler\round2\shots`
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
- **Crash guard**: if a styled app exits with an NTSTATUS-style code (`>= 0x80000000`: a crash, or
  being ended with `TerminateProcess(-1)` as `Stop-Process` and Task Manager's *End task* do)
  within 60 s of being attached, that counts once. Deaths of one app within 5 s of each other are
  one incident (an Explorer restart ends every Explorer process at once). After two incidents, the
  helper stops styling that app for the session and logs it (`not styling settings again this
  session`); a fresh helper session starts clean. Exercised 2026-11-06 with Settings. A process
  the allowlist rejects is never attached.
- The helper exits on its own only if `experimental.app_styler` reads off twice, a second apart:
  a config writer that replaces the file non-atomically briefly shows no file (defaults, off).
  Core stops the helper when the flag really goes off.
- **Soak, 2026-11-06** (glass, live desktop, about 45 min with the flag on, 29 of them scripted):
  Explorer windows and tabs opened, navigated (folders, Home, Gallery) and closed, Settings pages,
  two Explorer restarts, several mypaper redeploys and flag off/on cycles. No Explorer or Settings
  crash, no guard trip, bars and wallpaper back after each restart. Helper: 2.9–3.3 MB private,
  47 ms CPU in 120 s idle (0.04 % of one core; it wakes only for window-show events).
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
mypaper-styler --dump PID [--winui] [--out FILE] [--props "SEL; SEL"]
                                                    the live XAML tree, to write selectors
mypaper-styler --preview glass                      run with a preset, whatever the flag says
```

`--dump` attaches with no rules and writes an indented outline (`Type#Name`, with what is styled)
to `%LOCALAPPDATA%\mypaper\app-styler\tree-<pid>.txt`. `--props` adds every property value of the
elements the selectors (`;`-separated) pick, with where each comes from (local, style, template,
visual state) and brushes' colours, so a rule can target what is actually drawn. Use `--winui` for
Explorer's folder process. The TAP's own log is `%LOCALAPPDATA%\mypaper\app-styler\styler.log`.

XAML Diagnostics silently drops initialization data much longer than 256 characters (the TAP is
then never created), so the helper passes one folder and short file names.
