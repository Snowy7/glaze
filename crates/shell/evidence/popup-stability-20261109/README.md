# Popup stability and jump-list icons — 9 November 2026

The live fixes are in Glaze and its locally patched UI library, stillkit commit `6e04ebc`.

The flicker had several paths: native frost uploaded a transparent layered bitmap again on
resize/movement, async jump-list completion closed and reopened the root menu, and native
content retained the engine's 90 ms capture wait. Dock magnification could also switch
between native frost and an engine plate at application-window edges.

Native frost now initializes constant layered alpha before first show and retains its
composition tree. Geometry uses `SetWindowPos`; content/frost visibility changes together
in a deferred Win32 batch. Native menus/flyouts present immediately, async rows and icons
update in place, and native bars keep one provider over wallpaper and applications. Engine
plates are excluded for these native surfaces.

`popup_stability` uses no-activate windows and only internal `Menu::handle` hover events.
It checks this process's HWNDs with PrintWindow and samples DWM-composited checkerboard
edges after verifying every sampled pixel belongs to this PID. It saves no raw frames.

`frame-timing.json` records 360 synchronized samples: menu open, hover/submenu open-close,
async menu resizing, dock reveal-hide, magnification and flyout repaint. Assertions check
retained frost handles, visible frost, nonblank content, suppressed checkerboard edges and
that the fixture never takes foreground. The final run has zero blank/unfrosted frames.

The icon captures use real Explorer/Helium section structures and shell icons, replacing
private destination labels with examples. No private destination or task is executed.

- [Pinned shell icons](icons/dock-menu-live.png)
- [Task and custom-category icons](tasks/dock-menu-live.png)
- Seven jump-list tests pass, including generated-link IconLocation, DPI sizing and cache reuse.
- All bars tests: 130 passed, 2 existing desktop/live tests ignored.
- Native backdrop tests: 9 passed. CPU-mode GPU-driver check: 1 passed.
- stillkit all-targets check with golden rendering enabled passed.

Icons load on the worker after the first menu delivery. The bounded session cache uses
icon resource/target path and physical size. The UI kit's existing `Entry::with_icon`
supports both these bitmap icons and the app/Pin/Close column.
