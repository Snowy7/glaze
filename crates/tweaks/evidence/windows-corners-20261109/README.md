# Windows corner verification — 9 November 2026

The corner changes were built and deployed successfully, with the stale-binary guard passing.
Files was closed with WM_CLOSE and relaunched through `runas /trustlevel:0x20000` with
`--no-activate`. Studio was not running at deployment. The later Files and Studio fixtures
used isolated APPDATA and were closed after verification. The user's restored Files remains open.

## Geometry

- stillkit owns `WINDOW_CORNER_RADIUS = 8` logical pixels and the Windows `corner_radius(HWND)`
  resolver. It scales with GetDpiForWindow and returns zero when maximized or when the client
  reaches a monitor/work-area edge. Native app frames follow that same state through DWM.
- Files passes the resolver's device-pixel value to its engine glass and outer display-list
  clip. Its caption hover, fills and outlines consequently follow the same contour. Layered
  Files uses per-pixel alpha rather than a custom SetWindowRgn; Studio retains the native DWM frame.
- Studio already used native DWM rounding, not an oversized engine-glass plate. Its helper now
  respects edge/maximized geometry too.
- Default menu tokens and shell flyouts use 8 logical pixels. Widget cards and dock/bar geometry
  are unchanged.

`files-vs-windows.png` compares the deployed Files client contour with a separate native
Windows test window using DWMWCP_DEFAULT. `corner-modes.png` shows all four native modes.
`app-corner-states.png` shows floating, edge and maximized geometry for Files and Studio.
These are Windows.Graphics.Capture HWND captures, composited on grey. No other application's
pixels are included. Files' invisible resize margins are cropped for the comparison.

The tested monitor and all captured windows report **96 DPI / 100%**. Therefore the requested
100% and current-monitor-scale comparisons coincide. Higher physical DPI was not available
in this run and was not claimed as a separate visual test.

The maximized fixture sets WS_MAXIMIZE and matching work-area bounds with SWP_NOACTIVATE,
without using an activating ShowWindow command. Only the disposable fixture windows were moved.
The user's restored Files window was captured read-only. Foreground HWND stayed unchanged in
the mode and geometry capture sequences (`modes.json`, `geometry-cases.json`).

## Windows-corners tweak

**Overall: partly works as a general other-app feature.** It works on the tested normal native
frame, but production intentionally restricts styling to allowlisted Microsoft Windows apps.
It does not style browsers, Unity or arbitrary third-party apps, and it does not inject code.

| Mode | DWM value | Foreign HWND set/read | Visible capture | Existing/new/reset/release/drop |
|---|---:|---|---|---|
| Default | 0 | Works, HRESULT 0 | Normal Windows rounding | Works |
| Round | 2 | Works, HRESULT 0 | Rounded | Works |
| Small | 3 | Works, HRESULT 0 | Smaller rounding | Works |
| Square | 1 | Works, HRESULT 0 | Square | Works |

No call was rejected on the tested standard WinForms frame. This does not establish support
for every class. Windows documents that alpha-layered and custom-region windows can prevent
native rounding even when a preference is supplied; Glaze Files draws and clips its own contour.

| Case | Result and evidence |
|---|---|
| Studio reachability | Works: Look > Windows > Corners is an enabled, visible ComboBox in the live isolated Studio (`studio-control.json`). Default is now explicitly available beside As is, Round, Small and Square. |
| Studio writes | Works in the real action-route fixture test for all five choices (`studio-controls.log`). No global user setting was toggled. |
| Applying changes live | Works at Styler::update on the owned foreign process. Production config/theme watcher routing was inspected; the entire Studio-to-production-core path was not exercised because that would change real windows. |
| Existing windows | Works for all modes in the separate-process lifecycle test. |
| Newly opened windows | Works for all modes through the actual WinEvent hook. |
| App resets preference | Fixed: the existing one-second tracked-window timer now reads the actual corner preference and repairs drift. All four modes passed after the host reset itself to a different mode. Unchanged values cause no rewrite. |
| App already owns backdrop | Fixed: an explicit backdrop no longer excludes all other frame attributes. Its material is preserved independently. This case was inspected in code, not visually tested with a second backdrop class. |
| Elevated / UIPI | Explicit token-elevation exclusion added. Runtime elevated verification remains incomplete: `whoami` reported Medium Mandatory Level, and both attempted inherited-token hosts were medium. The opt-in elevated test correctly failed against that invalid fixture; no elevation prompt was used. |
| Borderless full-monitor window | Works: tracked corners are released on entry and reapplied after returning; tested without WS_EX_TOPMOST. |
| Exclusive fullscreen games | Not runtime tested. The unchanged strict Microsoft-app allowlist rejects games and third-party apps. No game or user window was manipulated. |
| Tool windows | Works: managed corners are released when the owned host becomes a tool window, then reapplied when it returns. Tool surfaces may still follow dark mode by design. |
| Glaze's own windows | Excluded by the production Windows-app allowlist; the styler process is also explicitly skipped. Policy tests passed (`policy-tests.log`). |
| Turn off / field release | Works: update to an unmanaged field and Styler drop restore the captured original. Foreign-window test passed for every mode; existing unit tests also cover non-default original values and failed-restore retries. |
| Crash recovery | DWM reset_all restored the owned foreign window to Default. Journal/crash unit tests passed. A real core crash was not induced. Crash recovery uses DWM defaults; normal shutdown can restore captured originals. |

The foreign lifecycle test is opt-in. Its cfg(test)-only PID filter rejects every process
except the separately launched host, while permitting that one host past the Microsoft-image
allowlist. Its non-elevated check remains active. No test mode was applied to a user's real window.

Current user preference: `look.windows.corners = "round"`, style `liquid`; the active theme has
no `windows` override. Config and theme were backed up before live tests and restored byte for
byte afterwards; both SHA-256 comparisons passed (`restoration.json`).

## Checks and deployment boundary

- stillkit golden-enabled test suite passed; final library run: 124 passed, 1 ignored.
  Its CPU no-vendor-GPU-driver integration test passed. Files' import table has no added or
  removed DLLs and no forbidden GPU imports (`imports.json`). The standalone stillkit CPU
  integration binary also imports Windows' d3d11/dcomp system DLLs for backdrop support; that
  alone is not a vendor-driver load, and the runtime no-driver test passed.
- stillkit clippy passed with the then-existing Menu `useless_conversion` warning. Strict
  `-D warnings` flagged the other agent's Menu line; it was not edited by this task.
- Glaze tweaks: 70 unit tests passed before the second ignored fixture test was added;
  foreign lifecycle subsequently passed with all four modes and tool/full-monitor cases.
- Studio corner action test passed. Both targeted flyout look tests passed. Eight injection
  policy/signature tests passed.
- Release workspace build and plugin build passed. Deployment and the freshness guard passed
  at deployment and again before the late parallel stillkit pull. `deployed.json` records
  matching built/deployed hashes for core, shell, wallpaper, Files and Studio.

The deployed source pins stillkit **e8db92773c860fc7e99ddb2cf58666e394684f15** on both Cargo lines.
This task's pushed stillkit commits are **3f42c3e**, **ee0992e**, and **e8db927**.

A later required pull brought in the other work's **7fdd2ec** (Menu and activation changes).
The post-pull guard then rejected another deployment because Menu source is newer than the
built shell (`post-parallel-freshness.log`). Per the requested concurrency boundary, no rebuild
loop or second deployment followed. The verified corner build remains live; incorporating
that later shared update into a fresh shared build/deployment is left for the coordinating agent.

References: [Windows 11 geometry](https://learn.microsoft.com/en-us/windows/apps/design/signature-experiences/geometry),
[native rounded-corner limits](https://learn.microsoft.com/en-us/windows/apps/desktop/modernize/ui/apply-rounded-corners).
