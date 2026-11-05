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
- The `styles` field of a theme (XAML/Win32 restyling through the injector) is a separate, later
  feature; this document only covers `windows`.
