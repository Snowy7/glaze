# app-example: a Unity scene as a wallpaper

This folder is a template, not a runnable theme: there is no `theme.json` here, because it
needs a player build that is too big to ship in the repo. Follow the steps below to make one.
For how `app` wallpapers work (embedding, suspending, limits), see
[`docs/web-and-app-wallpapers.md`](../../../docs/web-and-app-wallpapers.md).

## 1. Build the player

In Unity 6 (any 3D/URP template), build a **Windows x86_64** standalone player of your scene.
Useful player settings:

- **Fullscreen Mode: Windowed**, and **Run In Background: on** (Resolution and Presentation).
  The player is embedded as a child window, never the foreground window; with Run In Background
  off Unity would pause itself the whole time.
- **Resizable Window: on**, so the player can follow its parent's size.
- Target a modest frame rate in a script, since mypaper only suspends the player while it is
  hidden and does not pace it: `Application.targetFrameRate = 30;` and
  `QualitySettings.vSyncCount = 0;`.
- Turn the **Splash Screen** off (a splash is a separate window and delays embedding).

## 2. Make the theme folder

Copy the whole build output (`MyScene.exe`, `UnityPlayer.dll`, `MyScene_Data/`, `MonoBleedingEdge/`,
and so on) into a theme folder, and add a `theme.json` next to the exe:

```text
%APPDATA%\mypaper\themes\myscene\
  theme.json
  MyScene.exe
  UnityPlayer.dll
  MyScene_Data\
  ...
```

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

- `command` is relative to the theme folder (or an absolute path).
- `{hwnd}` is replaced with the decimal handle of the screen window the player should render into.
  Unity's `-parentHWND <handle> delayed` makes the player create its window as a child of that
  handle and show it once it is ready.
- Other arguments are passed through unchanged, for example `-screen-quality Medium`,
  `-window-mode borderless` or `-force-d3d11`.

## 3. Use it

Set `"theme": "myscene"` in `%APPDATA%\mypaper\config.json` (or pick it in the Studio). mypaper starts
the player on the first monitor's desktop window and keeps it there:

- While that monitor is covered by a maximized window, a fullscreen game is running, the PC is on
  battery (with `pause_on_battery`), the session is locked or the display is off, the player is
  **suspended**: 0% CPU and GPU.
- Switching theme, or stopping mypaper (even by killing it), ends the player.
- Only the first monitor shows the scene; other monitors are left blank.

## If the player makes its own window

A player (or any renderer) that ignores the parent argument and opens a normal top-level window is
adopted anyway: mypaper finds that process's first visible top-level window within about 5 seconds,
strips its title bar and borders, and moves it into the screen window.

## Without Unity

Anything that opens a window works the same way, so you can try the embedding with any small
renderer. Point `command` at it and use `{hwnd}` only if it takes a parent handle.
