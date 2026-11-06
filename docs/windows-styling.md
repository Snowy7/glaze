# Native Windows styling

A theme can restyle Windows itself while it is active: window borders, title bars, corners,
backdrop (Mica and friends), the system accent color and dark mode. Everything uses supported public
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
follows the theme; `null` keeps Windows' own setting ("As is"); a value sets it. The fields and
values are the ones below. So styling Windows needs no copy of a built-in theme, and it survives a
wallpaper change. See [studio-ux.md](studio-ux.md#6-look-and-windows-styling-as-part-of-it-d2).

## The `windows` section

Every field is optional; an unset field leaves Windows' own setting alone. Unknown fields are
rejected.

| Field | Values | What it changes |
|---|---|---|
| `accent` | `"auto"` or `"#rrggbb"` | System accent color (registry, see [below](#system-accent-and-dark-mode)) |
| `border` | `"auto"` or `"#rrggbb"` | Window border (`DWMWA_BORDER_COLOR`) |
| `caption` | `"#rrggbb"` | Title bar background (`DWMWA_CAPTION_COLOR`) |
| `caption_text` | `"#rrggbb"` | Title bar text (`DWMWA_TEXT_COLOR`) |
| `corners` | `default`, `round`, `small`, `square` | Corner rounding (`DWMWA_WINDOW_CORNER_PREFERENCE`) |
| `backdrop` | `auto`, `none`, `mica`, `acrylic`, `tabbed` | System backdrop (`DWMWA_SYSTEMBACKDROP_TYPE`) |
| `dark_mode` | `true` / `false` | Dark title bars (`DWMWA_USE_IMMERSIVE_DARK_MODE`) and the system's app/system theme |
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
| `Software\Microsoft\Windows\DWM` | `AccentColor` (`0xAABBGGRR`), `ColorizationColor` (`0xAARRGGBB`) |
| `Software\Microsoft\Windows\CurrentVersion\Explorer\Accent` | `AccentColorMenu` (`0xAABBGGRR`), `AccentPalette` (8 swatches `R G B 00`: three lighter shades, the accent, three darker ones, then Windows' fixed orange) |
| `Software\Microsoft\Windows\CurrentVersion\Themes\Personalize` | `AppsUseLightTheme`, `SystemUsesLightTheme` (0 = dark) |

The accent shows on the taskbar and Start only if the user has "Show accent color on Start and
taskbar" enabled in Settings; mypaper doesn't change that switch.

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
- **When it runs**: on a theme change (or a theme without a `windows` section), Quit, the panic
  hotkey, Ctrl+C and normal exit. Core's own drop does it too, so error exits are covered.
- **After a crash or kill**: the journal is still there, and core's next start undoes it before doing
  anything else. Windows opened or styled by the dead run are reset to defaults too. Their original
  dark-mode value is lost, so if the dead run forced dark mode, windows get the system's current
  setting (Windows' own apps follow it). A second instance never undoes a running instance's journal.
- **By hand**: if something is ever stuck, quit mypaper, set the values in the table above back (or
  just pick your accent again in Settings), and delete `undo.json`.

## Behavior notes and limits

- Styling follows the theme in `config.json`, and the `windows` section is re-read whenever the
  wallpaper reloads its theme, so editing `theme.json` applies live.
- While a theme with `accent` or `dark_mode` is active, changing those in Settings by hand is
  overwritten the next time the theme changes colors, and undone at restore.
- Apps that set their own title bar color or backdrop may override ours on their next paint.
- Restyling the *inside* of File Explorer and Settings is the experimental [app styler](#app-styler-experimental)
  below; everything above is DWM and registry only.

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
    "rules": { "Grid#NavigationBarControlGrid": { "Background": "$accent/18" } }
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
  alpha. A token with no colour yet leaves its property unset.
- `resources` (theme resource overrides by key) is accepted in the format but **not applied yet**.
  Restyling Explorer's template brushes (hover, pressed) needs it.

### Presets

Data in [`crates/app-styler/presets`](../crates/app-styler/presets):

| Preset | Settings | Explorer |
|---|---|---|
| `calm` | flat neutral cards, no borders, grey selection mark | transparent bars, quiet borderless boxes |
| `glass` | transparent root, frosted round cards, `$accent` selection mark | transparent bars and details pane, frosted round boxes |
| `compact` | shorter cards and nav rows, smaller nav text | tighter command bar and boxes, smaller breadcrumb text |

### Undo and the crash guard

- Every value is recorded before it is changed: the element's own (local) value, or "none". It
  goes back when a rule stops wanting it, when the element leaves the tree (it may come back), and
  when the helper exits. A local value comes back as the property chain reports it (its value
  object, or its text recreated). WinUI 3 refuses some of those objects; then the property is
  cleared to its style's value, which in Explorer's header looks the same.
- **Crash guard**: if a styled app crashes (an NTSTATUS exit code, not a normal close or a Task
  Manager restart) within 60 s of being attached, that counts once. After two, the helper stops
  styling that app for the session and logs it. A process the allowlist rejects is never attached.
- Missing elements across Windows builds are not errors: a selector that matches nothing does
  nothing.
- Nothing is persisted in the apps. If anything is ever stuck, closing and reopening the window
  (or restarting Explorer) shows the stock look again.

### What can and can't be styled

- **Settings**: the whole window is XAML: title bar area, search box, navigation pane, breadcrumb,
  every page and card.
- **File Explorer** (folder windows): tabs (`TabView#TabView`), the navigation bar
  (`Grid#NavigationBarControlGrid` with back/forward/up/refresh, the address bar
  `Grid#FileExplorerAddressBarGrid`, the search box `AutoSuggestBox#FileExplorerSearchBox`), the
  command bar (`CommandBar#FileExplorerCommandBar`) and the details pane
  (`FileExplorerExtensions.DetailsPaneControl`). Home and Gallery are XAML too.
- **Not reachable**: the file list (DirectUI `DirectUIHWND`), the navigation tree (a Win32 tree
  view) and the status bar are not XAML, so XAML Diagnostics can't see them. They need a different
  mechanism (theme classes / `SetWindowTheme` inside the process): a later stretch.
- **Visual states win**: hover and pressed looks come from template storyboards and theme
  resources. A local `Background` shows at rest; hover still shows the template's brush.

### Tools

```text
mypaper-styler --dump PID [--winui] [--out FILE]   the live XAML tree, to write selectors
mypaper-styler --preview glass                      run with a preset, whatever the flag says
```

`--dump` attaches with no rules and writes an indented outline (`Type#Name`, with what is styled)
to `%LOCALAPPDATA%\mypaper\app-styler\tree-<pid>.txt`. Use `--winui` for Explorer's folder process.
The TAP's own log is `%LOCALAPPDATA%\mypaper\app-styler\styler.log`.
