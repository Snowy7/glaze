# N4 implementation handoff

Branch: `t3/native-desktop-widgets`. No push or merge.

Implementation commits:
- `5c232a6` native desktop surfaces, image/arc ops, calendar and windowless text component.
- `fcffc57` widget services conversion, linked weather, browser/SDK cleanup and guides.
- `3cac530` 17 native views, grid editing, manifest, shell wiring, Studio metadata adapter and fixture.
The final evidence/check commit follows these three; it also forwards input-language changes to
windowless RichEdit while preserving normal HWND language-context handling.

The shell contains 17 compiled widget ids, native edit mode and one shared media implementation.
`glaze-widgets` is a services library; core no longer starts its executable. The HTML widget
folders, SDK, widget WebView host/environment, CSS plates and widget backdrop request producer are
deleted. Weather is linked and uses bounded blocking WinHTTP on a sleeping worker. Existing layout,
settings, per-instance storage, shared activity and replacement ids are retained.

The desktop surfaces are top-level layered tool windows immediately above Progman. The old
`DesktopChild` API name remains, with `parent` used as a z-order anchor. The UI uses software D2D/WIC,
DirectWrite and windowless RichEdit `ITextServices2::TxDrawD2D`. No D3D hardware device is created.
Top-level windows follow N0.2's measured visibility and WGC-exclusion result; a WARP DComp alternative
was not implemented because this topology already passed native visibility and input tests and keeps
client HWNDs available to the overlay exclusion list.

## Measurements

| Gate | Measured | Result |
|---|---:|---|
| Six widgets + warmed menu private memory, <=40 MB | 21 / 22 / 22 MB; median **22 MB** | Fixture passes; merged shell unmeasured |
| Idle CPU, <=0.1% of one core | official 0.18 / 0.21 / 0.18%; median **0.18%** | **FAIL** |
| Actual-wall-time CPU (tools/perf math) | 0.18227 / 0.15623 / 0.15621%; median **0.15623%** | **FAIL** |
| Minute clock | 2 -> 3 -> 4 total presents over consecutive samples | 1/minute passes |
| Redraws outside scheduled/data ticks | successive total presents 41 -> 72 -> 103 | 30 stats + 1 clock per minute; no extra idle presents |
| Own fixture WebView2 children / vendor GPU drivers | 0 / none | Passes for fixture |
| Both monitors visible | 6/6 center hits in all 3 final samples | Passes |
| Activation / click / Show Desktop / Explorer recovery | 12/12 activations; click and restore pass; 6 recreated HWNDs | Passes in locked interaction run |
| Native goldens and hit tests | 17 PNGs and 17 ids covered | Passes |
| Existing real layout | 8/8 instances loaded read-only; old-format storage fixtures pass | Passes; no real per-instance storage files were present |
| Mixed-DPI / language IME composition | two actual monitors both 96 DPI; Unicode Arabic/CJK keyboard save passes | Mixed DPI and actual IME composition unmeasured |
| Widget glass / <=1 frame plate alignment / whole product budgets | local publisher stub; N2/N5 absent from this base | Not measured |

**Not ready to merge against the performance gate.** CPU remains above budget after native arc ops,
conditional stats fields, cached PDH buffers, raw interface-table reads, and upward-rounded waits.
The extra 70-second profile (`profile.log`) recorded 0.26036% one-core CPU; accumulated diagnostics
were 56,909 us in stats calls, 65,684 us in painting, and 265 host polls including startup. These
are wall-time totals and not a CPU attribution. No vendor drivers were loaded. The next performance
pass should profile the UI-thread sampler and software-present path before integration is approved.

The standalone fixture contains six production widget surfaces and a warmed, closed native menu on
both real 2560x1440 monitors, both at 96 DPI, media paused/no active SMTC playback, minute clock.
It uses isolated APPDATA/LOCALAPPDATA. Official measurements use the shared `measure.ps1`, 3 x 60 s.
Internal measurements use `tools/perf::cpu_use` with actual elapsed wall time. The official script
normalizes by its requested 60 s although GPU counter collection takes longer; report both values.
A bars/engine integration fixture was not available in this checkout; the glass module is the
explicitly permitted stub. Whole-shell and whole-product gates need a merged N2/N4/N5 run.

## Behavior and visual verification

`interactions.log` records 12/12 app activations, alpha-1 hit testing, a real mouse click, Unicode
keyboard entry persisted in notes, Show Desktop and restore, all six windows recreated after an
Explorer restart, plus drag/resize persisted as snapped grid data. Z-order is reconciled from both
WinEvents and the surface's own position/visibility messages, with no idle z-order polling.

Verified: `widgets-contact-sheet.png`, generated from all 17 production-renderer golden PNGs in
`crates/shell/src/widgets/golden/`. Reviewed the live desktop screenshot as well; top-level native
content is visible. The final live crop is `live/widgets.png`. Click hit-target tests cover all 17 ids
(including body-only read-only views); every widget has an alpha/hit-body golden check. External
actions such as opening Settings, launch targets and RSS URLs were not all executed on the user's
machine. Actual installed IME candidate/composition UI and mixed-DPI transitions remain unmeasured;
the successful Arabic/CJK check uses native Unicode keyboard events. RichEdit's selection, undo,
bidirectional drawing and no-redraw-feedback contract have component tests.

The fixture's guards restore the media, minimized apps, focus and cursor changes it owns.
The fixture never stops a Glaze process. The original core executable was restarted under the
lock after the last fixture. After another agent's perf/plates run, the final locked restoration
confirmed core, native shell and wallpaper running (`restored.json`). The existing `bars.json`
requests taskbar mode `hide`, and N2's running shell actively enforces it. The stock taskbar was
observed visible while that shell was stopped, but is hidden with the user's normal configuration;
that preference was not rewritten by N4. This differs from IMPLEMENTING's stock-taskbar-visible
exit condition. Other agents changed the running product between sample sets; covered samples from earlier runs were excluded from final gate evidence.

## Checks

Passed: UI 17 unit + 1 no-vendor-driver integration test; widgets 120 tests; weather 27;
shell 15; Studio widget adapter 9; plus the manually enabled read-only real-layout check (8/8).
All 17 golden PNGs match after blessing the intended native visuals. Arabic/CJK RichEdit drawing,
selection, undo and no paint-feedback behavior pass. Two widget tests are normally ignored (real
WASAPI cost and sampler profiling); the sampler profile was run separately. The read-only real-data
test is ignored by default because CI has no user's data.

Strict `cargo clippy --all-targets -- -D warnings` passes for UI, widgets, weather and shell.
`cargo check -p glaze-core`, `cargo check -p glaze-wallpaper`, Studio's Rust check and
`pnpm typecheck` pass. Studio dependencies were installed from the frozen lockfile without scripts.
No dev server was started. Release fixture builds were used only for the requested live tests.

## Integration and ownership notes

- Replace `crates/shell/src/glass.rs`'s no-op `publish` with N2's publisher. `Plate.id` encodes the
  client HWND for overlay exclusions. Rectangles are monitor-local physical pixels; convert to the
  shared-memory protocol's virtual-desktop coordinates when combining monitor snapshots. Publish
  each drag frame, changes only at rest, empty lists when hidden/removed/shutting down. Actual glass
  plates, <=1 frame motion alignment and web/app overlay exclusions are unverified until wired.
- `crates/desktop/Cargo.toml` retains an empty `webview` feature solely because N2's base bars crate
  still requests that feature. The environment implementation and dependency are deleted. The old
  bars binary cannot compile against the removed environment; merge N2 and remove that feature.
- Outside explicit ownership: `apps/studio/src-tauri/src/widgets.rs`, `commands.rs`, and
  `apps/studio/src/components/stage/WidgetFrame.tsx` are a minimal adapter to the compiled catalog.
  Existing settings forms remain usable after folders are deleted. Studio stage previews are name
  placeholders until N7. Rust widget tests and TS typechecking pass; Studio UI was not run visually.
- Outside explicit ownership: `crates/wallpaper/src/sources/web.rs` removes only the obsolete widget
  backdrop CapturePreview function, request-file polling, and its two state fields. Web wallpaper
  pause-still CapturePreview remains because it serves the wallpaper itself. N5 still owns the
  engine's 1/8 BMP `backdrop.rs` request consumer/deletion.
- `crates/desktop/src/lib.rs`/Cargo.toml remove the shell environment export/dependency; root
  `Cargo.lock` records library/dependency conversion. `docs/writing-plugins.md` now points to the
  native service-plugin guide. These are minimal supporting edits outside named source ownership.
- Bars sources and bars modules were not edited; shell `main.rs` changes only wire widgets/events,
  hold facts, deadlines and the motion timer into the existing loop.

## Exact interfaces for later slices

- `Widgets::new(&mut Ui, HWND) -> Result<Widgets>`, `raw_message(&MSG)`, `handle(&mut Ui, inbox::Msg)`,
  `fact(usize, isize)`, `on_event(&mut Ui, SurfaceId, &Event)`, `poll(&mut Ui)`, `timeout_ms() -> u32`,
  `timer_handle() -> HANDLE`, `frame_ready()`. Inbox `WM_APP + 100/101/102/103` means edit / overlay /
  layout reload / service data. Internal lParam is 0 off, 1 on, 2 toggle; legacy registered messages
  carry that value in wParam to class `glaze.widgets.host`.
- `Kind::DesktopChild { parent: HWND }`; `Surface::keep_above_desktop() -> Result<bool>`,
  `raise_in_desktop_band() -> Result<()>`, `desktop_order_valid() -> bool`.
- `glass::publish(monitor: &str, plates: &[Plate])`;
  `Plate { id: u64, rect: Rect, radius: f32, material: Material }`;
  `Material { clarity: f32, blur: f32, refraction: f32, highlight: f32, tint: Rgba }`.
- `TextField::new(HWND, Rect, multiline: bool, size: f32, Rgba) -> Result<TextField>`;
  `set_text(&str) -> Result<()>`, `text() -> Result<String>`, `set_rect(Rect)`, `focus()`, `blur()`,
  `revision() -> u64`; `Surface::set_text_field(Option<TextField>)`;
  `Op::TextField { rect: Rect, field: TextField, revision: u64 }`.
- `Event::DpiChanged(u32)`, `VisibilityChanged`, `Cancelled`, `TextChanged`, `TextSubmit`.
  RichEdit invalidation uses private `WM_APP + 120`, with no borrowed payload.
- `calendar::Date { year: i32, month: u32, day: u32 }`, `MonthGrid::new(Date, week_start: u32)` and
  `MonthGrid::view(&self, &mut DisplayList, Rect, today: Date, text: Rgba, accent: Rgba, base: u32,
  &TextStyle)`. Day hit ids are `base + day`. `Date` provides civil arithmetic and ISO week numbers.
- `Ui::decode_image(&[u8], (u32,u32)) -> Result<Image>`;
  `Op::Image { rect: Rect, image: Arc<Image>, opacity: f32, source: Option<Rect> }`;
  `Op::Arc { center: Point, radius: f32, sweep: f32, width: f32, color: Rgba }`, sweep in radians.
- `registry::builtins() -> Registry`; `Stats::subscribe_fields(&str, &str, Duration, Instant,
  SampleFields { gpu: bool, disks: bool, net: bool, battery: bool, network: bool })`.
  Existing `Stats::subscribe`/`request` retain all fields for external consumers.
- `Waker::for_window(Sender<Msg>, HWND, message: u32) -> Waker` posts queued service completions.
  Existing JSON-RPC plugin framing, method/event declarations, timeouts and data directories remain.


Generated-file cleanup: automatic approval review rejected removal of the verified workspace-local
fixture folders and oversized full-desktop capture with "blocked by policy". These generated data,
cache and raw-shot files remain ignored by `artifacts/n4/.gitignore`; only selected logs, measurements,
the cropped native-widget screenshot, contact sheet, chart and report are tracked. No fixture or
helper process remains running.
