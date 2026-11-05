# Personal integration qualification — 2026-11-05

Implementation commits: `f89d3a5`, `f63c97e`, `b29f074` plus the final evidence commit.
Repository/branch: `D:\Coding\projects\desktop\wallpaper`, `native`, shared checkout.

| Check | Result |
|---|---|
| `cargo check -p mypaper-shell -p mypaper-studio` | Passed after the concurrent owners finished their shared API edits. |
| `cargo test -p mypaper-integrations` | 18 passed (9 library, 9 integration); includes Credential Manager, provider lifecycle and cancellation-safe Discord IPC frames. |
| `cargo test -p mypaper-widgets --lib` | 131 passed, 3 ignored. |
| Shell `custom_tests` | 2 passed: imported data stays separate from local events; dense native frames, account scrolling and custom templates at 150% scaling stay within their frames. |
| Studio `goldens::widgets` | Passed for dark and light; both images were generated, dark was inspected. |
| Clippy for integrations/widgets/shell | Passed; remaining warnings were in other agents' bars/flyouts/toggles code. No warning remained in owned runtime files. |
| Four shipped custom examples, real evaluation | All passed in isolated APPDATA: static text, GitHub HTTPS JSON (`1.99.1`), local file and bounded local command (disk gauge `88`). Each was present as a separate multi-instance catalog entry. |
| `cargo build --release --workspace` | Passed with `CARGO_TARGET_DIR=C:\mypaper-targets\main`; final successful rebuild took 1m17s. |
| `scripts/deploy-local.ps1` | Completed under its desktop lock. Package `mypaper.Desktop_0.1.0.3_neutral__21v47vsar82ra` is installed. Core/shell/wallpaper resumed from `target\release`. |
| Open Studio | Left running as required. Deploy swapped its executable without closing the user's window; restart Studio to use the new Widgets entry. |
| Windows AppointmentStore | The real read-only API succeeded and returned 0 events. This proves the store is accessible here, not that Outlook/Google accounts populate it. See `personal-unpackaged.json`, whose `packaged` field is explicitly false. |
| Live native calendar | Captured and inspected at 415×313 after deployment, using PrintWindow under the desktop lock. Month layout, date, controls and new Calendars action render. No events were available from the current real Windows store. |
| Real Discord | Discord was running; its retained Windows notification count was 0. No developer app, bot credentials or RPC/tester grant was configured. No message or voice-setting action was sent. The minimized Discord window could not provide a useful screenshot and was left untouched. |

The native fixture sheets were inspected directly: `golden-personal/discord.png` (including call,
setup and bot composition), `calendar.png` (month, agenda and sources) and `custom-fixture.png`
(text/stat/list/gauge). They use labelled fixture data, not the user's Discord account.

## Remaining qualification limits

- The complete shell library suite terminates with Windows exception `0xe06d7363`; serial execution
  stops at `flyouts::tests::every_flyout_matches_its_golden_image_and_takes_clicks_on_its_whole_body`.
  The owned focused tests pass. No flyout code was changed to work around it.
- `Invoke-CommandInDesktopPackage` hung while attempting a separate read-only diagnostic process.
  The diagnostic did not produce an evidence file and its launcher was stopped. It is not counted
  as a successful packaged API probe. The deployed widget screenshot and installed identity are
  separate observations.
- Google and Microsoft OAuth, real recurring account data, Discord RPC delivery/controls and bot
  history/sending require the user's registered client IDs/grants and credentials. These were not
  invented, extracted from apps or tested by sending messages. Setup is documented in `../PERSONAL.md`.
- Authenticated-provider CPU/RAM/idle gates were not measured without those registrations. The
  full product's performance gate belongs to the combined desktop run, not these fixture tests.

## Shared hooks needed by this slice

The additive hooks outside dedicated provider/view files are in the widget model (the Custom kind
and refresh deadline), widget host (field submission and catalog reload), view dispatcher, Studio's
Open action dispatch, notification reading (Discord-only preview consent), and the package's
`appointments` capability. Calendar view changes are limited to event iteration, calendar colours
and hiding edit controls for imported events; the month grid code was not rewritten. Existing
T3/Grok dispatcher hooks from their owner were preserved in the shared interface commit.

Public interfaces: `Provider::{Calendar,Discord}`, `Data::{Calendar,Discord}`,
`Action::DiscordVoice { muted, deafened }`, `config::{Calendar,Discord}`,
`notifications.discordPreviews`, `calendar_oauth::{Service,connect,disconnect}` and
`custom::{Definition,Template,Source,load,catalog,evaluate,install_examples}`. The ordinary catalog
is still `registry::builtins()`; file `widgets/name.json` registers as `custom-name`.

No user application received synthetic input. No user window was moved, minimized or closed.
