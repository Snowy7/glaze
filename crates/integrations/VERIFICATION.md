# Integration implementation handoff — 2026-11-05

The first batch is implemented, with conditional live features gated. **This is not full live-batch
qualification.** Notification event delivery failed on this unpackaged shell, Tailscale's live bus
delivered unchanged-state traffic, and authenticated live T3 access remains pending. Those sources
default to snapshot-on-open; no polling or recurring ping fallback was added.

Implementation commits on `t3/native-app-integrations`:

- `b17c792` — demand-owned providers, credentials and protocol clients.
- `1f7c02a` — native modules/panels/badges/actions, shared media adapters, bounded metadata and Studio docs.

The following report/evidence commit completes the handoff. Nothing was pushed or merged.

## Measured gates

These are **single 60-second headless adapter samples**, with a preceding disabled 60-second sample
in the same process. CPU is percent of one core from Windows process CPU time; memory is private MiB
(`PROCESS_MEMORY_COUNTERS_EX.PrivateUsage`). Each activation has a finite 2-second startup grace
period. Added memory is the post-activation private footprint minus the disabled baseline; end-of-sample
private memory is also recorded. This is adapter evidence, not the plan's full-shell median-of-three fixture.
Unchanged display-list comparisons were tested separately with the production software renderer.

| Provider/mode | Disabled CPU | Enabled idle CPU | Added private MiB (gate ≤5) | Idle deliveries / snapshot changes | Result |
|---|---:|---:|---:|---:|---|
| SMTC | 0.00% | 0.00% | 2.629 | 0 / 0 | Meets these single-sample adapter limits |
| Notifications, snapshot | 0.00% | 0.00% | 1.668 | 0 / 0 | Meets these single-sample adapter limits |
| Tailscale, snapshot | 0.00% | 0.00% | 0.168 | 0 / 0 | Meets these single-sample adapter limits |
| Tailscale, IPN bus | 0.00% | 0.00% | 0.215 | **32 / 0** | **Fails strict unchanged-state wakeup gate** |
| Notifications, live events | — | — | — | Registration failed `0x80070490` | **Not qualified** |
| T3, authenticated live | — | — | — | Pairing token not available | **Pending** |
| OBS / Docker / Grok | — | — | — | No live authenticated backend/action | **Fixture/compile verification only** |

Raw, metadata-only JSONL is under [`evidence/`](evidence). No notification text, thread titles,
personal recent-item paths, tokens, app configuration, or passwords were persisted there.

Remaining merge gates: the native plan's **three 60-second full-shell comparisons with every
eligible integration enabled**, aggregate CPU beyond the current baseline, per-provider measurements
with authenticated T3/OBS/Docker, open/close RAM and first-paint latency, and vendor-side/wire wakeup
qualification. No values for these are estimated or claimed. A sleeping socket alone is not proof
that its server has no maintenance traffic. This slice adds no helper process; each active provider
owns a cancellable Rust lane in the host process.

## Checks and live observations

- `cargo check -p mypaper-integrations` and `cargo check -p mypaper-shell`: passed.
- `cargo test -p mypaper-integrations`: **10 passed**. Covers real loopback Effect RPC/OBS fixtures,
  Windows named-pipe HTTP stream fixtures for Docker/Tailscale, reference-counted sharing,
  start/stop/explicit reconnect, cancellation, generation replacement, T3 sequencing/baselines/
  enrichment/attention/completion, Docker failure semantics, configuration bounds, and a separate
  Credential Manager test target which was removed afterwards, plus byte-bounded Unicode toast text.
- `cargo test -p mypaper-shell`: **37 passed, 1 existing ignored**. The ignored test reads this user's
  actual widget layout; it is unrelated to these integration changes. The widget-only fixture also
  compiles with the new shared SMTC adapter.
- `cargo test -p mypaper-bars config::tests`: **18 passed**, covering the existing module/config contract.
- `cargo clippy -p mypaper-integrations --all-targets -- -D warnings`: passed.
- `cargo clippy -p mypaper-shell -- -D warnings`: passed. Strict shell **all-targets** Clippy is
  blocked by the existing boolean-literal `assert_eq!` in `src/flyouts/tests.rs:498`; that other
  slice's assertion was not rewritten.
- New UI: **16 golden PNGs**, including panel scales 1 and 1.5, bar modules, dock notification badge,
  Recent actions, and Grok acceptance. Tests verify hit routing, body alpha, control bounds and no
  idle damage. Golden fixtures use the production `flyouts::build` and bar/dock view functions.
  Images were visually inspected; the notification calendar remains under the cards. These are
  headless native render/click-target checks, not live desktop input automation.
- SMTC, notification snapshot, and Tailscale snapshot were also started, explicitly retried and
  stopped against the real Windows APIs. Notifications' live registration failed identically on
  initial activation and explicit retry; no timer was substituted.
- Tailscale **1.104.1**, commit `7f4efe814ff14b064eb080344389ceb9ea2d37ac`: existing protected local pipe
  accepted mask 24582. A live 60-second sample delivered 32 messages without a displayed-state change.
  Keep `tailscale.liveVerified` false on this installation. Real VPN preference changes were **not**
  invoked; their request paths and results were exercised in the pipe fixture.
- Windows Shell Recent and Frequent for `Microsoft.Windows.Explorer`: **12 items each**, bounded
  reads; first Recent read **96.22 ms**, Frequent **20.26 ms** on the worker, without launching any
  destination. Only counts/timings were recorded. The panel's first frame does not wait for this IO.
- T3 discovery read only `~/.t3/userdata/server-runtime.json`'s endpoint/process metadata and the
  running process's listening sockets. Both identify PID **11944**, port **3773**.
  `http://127.0.0.1:3773/ws?orchestrationProtocol=2` returned **401** unauthenticated, confirming the
  local route. A secure secret card for a separate `orchestration:read` bearer timed out. No existing
  T3/MCP credentials, database, signing keys or transcript files were read for auth. No T3 setting
  was changed. Authenticated subscription/idle-wire/open-thread live checks remain pending.
  Open is enabled for rows with a copied HTTPS entry in `t3.threadLinks`; rows without one remain
  disabled until Studio/user setup supplies the link.
- OBS is installed; its configuration reports enabled WebSocket port **4455** with authentication,
  but OBS was not running and the endpoint was not listening. The password was not inspected.
  Docker Desktop is installed and its service was stopped. Neither vendor app nor WSL was started
  for monitoring. OBS/Docker real-server qualification is pending; their native protocol fixtures pass.
- Grok routines require the user's configured webhook URL and Credential Manager key, and an
  explicit click. No paid routine was invoked. HTTPS request code is compile-checked and configuration
  validation is unit-tested; a live Grok request/acceptance response is pending setup.

All tests and live provider probes that required the desktop lock took it and released it in
`finally`. No visible shell, engine, vendor app, browser, or helper was launched/stopped by this
slice; the user's desktop/taskbar settings and app configurations were left alone.

## Registration hooks and outside-ownership edits

Each registration block is at most 30 lines. New provider/UI code is under the owned directories.

| Path | Hook / reason |
|---|---|
| `crates/shell/src/main.rs` | Declare integrations; drain coalesced provider changes; forward lock/display facts. |
| `crates/shell/src/bars/mod.rs` | Module clicks, demand union over visible bars, invalidation, and a boxed app-context panel action. Visibility for existing pure view/model modules permits the new golden tests. |
| `crates/shell/src/bars/view.rs` | Integration module content, current-app retained-toast label and dock badge drawing. |
| `crates/shell/src/bars/services.rs` | Swap the media adapter, forward cached foreground identity, and remove the old 15-second notification-count fetch. |
| `crates/shell/src/flyouts/mod.rs` | Register integration/app-actions/routine panels and the shared notification center; expose the existing Kit/calendar to sibling panels. Legacy notification rendering is test-only. |
| `crates/shell/src/flyouts/kit.rs` | Two unused production glyphs are test-only with their legacy fixture. |
| `crates/shell/src/flyouts/launcher.rs` | Append integration entries after deciding whether the app index needs a read; dispatch explicit Grok clicks and preserve entries after index replies. |
| `crates/shell/src/widgets/mod.rs` | Import the shared widget media adapter. |
| `crates/shell/src/widgets/live.rs` | Include a small adapter bootstrap from the owned integration folder and drain it in the fixture's two loops; otherwise the standalone widget fixture could not compile after the import change. |
| `crates/shell/src/widgets/model.rs`, `tests.rs` | Remove the media progress deadline and update its existing scheduling assertion to require event-only playback updates. This is a necessary outside-registry edit. |
| `crates/bars/src/config.rs`, `actions.rs` | Add serializable `t3`, `obs`, `docker`, `tailscale` module names, without assigning a periodic topic. These are necessary small edits outside the shell registries. |
| `crates/shell/Cargo.toml`, `Cargo.lock` | Link the new library and lock native transport dependencies. No root workspace edit was needed (`crates/*`). |
| `docs/bars.md`, `docs/widgets.md` | Replace stale polling/progress descriptions with the shared subscription behavior and link the Studio configuration contract. |

## API for the next slices

The exact persisted keys, default values, credential names, supported limits and Studio consent
entry point are in [`docs/integrations.md`](../../docs/integrations.md).

```rust
Provider::{Notifications, Media, T3, Obs, Docker, Tailscale}
Hub::new(wake: impl Fn(Provider) + Send + Sync + 'static) -> Hub
Hub::acquire(&self, Provider, &config::Config) -> anyhow::Result<Lease>
Hub::stop_all(&self)
Lease::snapshot(&self) -> Snapshot
Lease::activity(&self) -> Activity { attempts: u64, deliveries: u64 }
Lease::retry(&self)
Lease::action(&self, Action) -> anyhow::Result<()>
// Clone/Drop own visible demand; an old generation's lease cannot affect its replacement.

Snapshot { revision: u64, availability: Availability,
           data: Option<Data>, action_error: Option<String> }
Availability::{Connecting, Live, Cached, Stale(String), Unavailable(String)}
Data::{Notifications(notifications::Snapshot), Media(media::Snapshot),
       T3(t3::Snapshot), Obs(obs::Snapshot), Docker(docker::Snapshot),
       Tailscale(tailscale::Snapshot)}
Action::{Dismiss(Vec<u32>), Media(media::Action), Obs(obs::Action),
         Docker { id: String, start: bool }, Tailscale { connected: bool }}
```

The callback carries a typed provider invalidation; consumers read its retained snapshot, with
equality suppression and no queued transcript/log stream. Actions are provider-checked, bounded,
rejected while disconnected, and discarded across reconnects. T3 summaries retain at most 256
threads and 16 deduplicated run transitions. Notification history is bounded to 512 cards and three
256-byte UTF-8 text elements each (display names are 128 bytes). Docker lists are bounded to 128 selected containers; event
delivery invalidates the list instead of reconstructing counters. A reconnect always resnapshots.

`credentials::{write, read, remove}` use `mypaper/integrations/t3`, `.../obs`, and
`.../grok/<routine-id>`. `notifications::request_access()` returns the WinRT operation and must be
called on the Studio UI thread after a consent click. `launcher::{recent, open, start_routine}` are
on-demand worker operations. `flyouts::Kind::{Integration(Provider), AppActions, RoutineResult}`
are the new shell messages; ordinary `Kind::Notifications` now uses the shared provider and calendar.

Before enabling persistent notification/T3/Tailscale modules, complete the failed/pending gates
above. The `liveVerified` settings are qualification flags, not proof of qualification.

## Desktop integration widgets — 2026-11-05

Six catalog entries now have kit views and demand-owned model hookups: `discord`, `t3`, `obs`,
`docker`, `tailscale`, and `grok`. Each declares `SizeSpec.allowed = [[2, 2], [4, 2], [4, 4]]`.
They use kit layout, text and palette roles, vector icons, hover/press/focus states and transparent
glass bodies. A hidden or held widget drops its lease; another visible consumer may retain the
shared provider. No polling, reconnect timer, client heartbeat or vendor process was added.
OBS's visible elapsed clocks schedule seconds only during active recording/streaming; paused,
disconnected, hidden and inactive clocks have no deadline.

| Widget | Implemented state and controls | Live verification / remaining setup | Reviewed size sheet |
|---|---|---|---|
| Discord | Exact configured Windows AUMID filter, retained toast count/cards, Open Discord. Privacy setting still controls text retention. | Real notification snapshot and zero-count desktop state. Voice participants, speaking, self-mute/deafen and exact unread mentions are unavailable through the existing provider; approved Discord RPC access is required. | [Discord](../../docs/media/native/widget-discord.png) |
| T3 Code | Active thread/provider status, approvals/questions, recent finished threads; copied configured thread links open on click. Native masked Connect prompt exchanges a one-time pairing code for `orchestration:read`. | Real port 3773 is listening; mypaper credential is absent. Pairing, summaries and narrowing pass loopback fixtures. User must pair and supply copied HTTPS entries in `t3.threadLinks`; absent links leave rows disabled. Live subscription remains gated by `liveVerified`. | [T3 Code](../../docs/media/native/widget-t3.png) |
| OBS | Recording/streaming state and clocks, scene switcher, first microphone mute, recording/streaming start/stop. Streaming-only medium layout uses the stream clock. | OBS was not running and port 4455 was not listening. Real obs-websocket fixtures verify initial scenes/mic mute, explicit scene/mute request payloads, sharing, cancellation and no idle requests. User must run OBS with WebSocket enabled and store its password. | [OBS](../../docs/media/native/widget-obs.png) |
| Docker | Container lifecycle/failure state, start/stop, CPU/memory samples on explicit Refresh. Unknown CPU stays unknown. | Docker Desktop service was stopped; no vendor app or WSL was started. Named-pipe fixtures verify lifecycle/control and one-shot stats requests. User must select the existing local engine pipe. | [Docker](../../docs/media/native/widget-docker.png) |
| Tailscale | Connection, local address, exit node, bounded online/offline peer snapshot and connect/disconnect. | Real protected local pipe read; connected state and peer names inspected on the desktop. Snapshot mode retained because the earlier IPN idle qualification failed. Peer state updates on explicit refresh; VPN controls were tested only in fixtures. | [Tailscale](../../docs/media/native/widget-tailscale.png) |
| Grok Bot | Configured saved routines, explicit Run, session-local recent request receipts (sending/accepted/failed). | Bot process is running, but no routines/keys were configured. Desktop setup state and fixture receipts inspected. User must configure webhook routines and keys. A 200 means accepted; general Bot execution/completion status has no provider contract. No paid routine ran. | [Grok Bot](../../docs/media/native/widget-grok.png) |

Media retains the existing shared now-playing/media-pill widgets: a third widget would duplicate
their functionality. A real SMTC read succeeded; two deliveries occurred during the short active
user-session sample, so that sample is not evidence of unchanged-state idle behavior.

### Checks and measurements

- Integration library tests: **12 passed**; strict all-target Clippy passed. Fixtures cover the
  T3 bootstrap form exchange and scope narrowing without persisting a fixture token, OBS scene/mic
  payloads and clocks, Docker one-shot CPU/memory parsing and pipe requests, Tailscale peers,
  credentials, subscriptions and cancellation.
- Widget library: **125 passed, 3 intentionally ignored**.
- Full shell library, serial native rendering: **72 passed, 6 intentionally ignored**. Earlier
  golden mismatches were resolved; all 23 catalog goldens pass. The six integration baseline
  images were added to `crates/shell/src/widgets/golden/` for that shared test.
- The focused widget tests check identical-input/no-damage behavior at 1× and 1.5× in all three
  sizes, bounds, glass without outlines, exact Discord identity filtering, inactive deadlines,
  paused/disconnected clocks and disabled stale Docker actions. The six size sheets include
  populated, setup, stale and empty states and were opened and visually reviewed.
- Strict shell Clippy encountered three `unnecessary_cast` warnings outside this slice, in
  `flyouts/quick.rs` and `widgets/views/toggles.rs`; those paths were left to their owners.
- `CARGO_TARGET_DIR=C:\mypaper-targets\main cargo build --release --workspace` passed. Deployed
  with `scripts/deploy-local.ps1`, which owns the desktop lock. The final desktop check temporarily
  placed all six widgets in free portions of DISPLAY2: **6/6 visible**, inspected over the live
  glass wallpaper. Layout and integration settings were restored in `finally`; no verification
  instances remain. No input was sent to Discord, T3, OBS, Docker, Grok or Tailscale.
- Local live captures are `%TEMP%\mypaper-integration-widgets-live.png` and
  `%TEMP%\mypaper-widget-deployment.png`. They include real desktop/network context and are not
  committed. Committed size sheets use synthetic metadata.

These fresh adapter samples are **one 10-second observation each**, after a one-second disabled
baseline and finite two-second activation grace. They are not the native plan's median-of-three
60-second full-shell gates. Raw metadata is under `evidence/widget-*.jsonl`.

| Read-only provider mode | CPU, one core | Added private MiB (provider gate ≤5) | Deliveries / displayed-state changes |
|---|---:|---:|---:|
| Tailscale snapshot | 0.00% | 0.488 | 0 / 0 |
| Notifications snapshot (Discord consumer) | 0.00% | 1.727 | 0 / 0 |
| Existing SMTC, user session active | 0.00% | 2.129 | 2 / 2 |

Full-shell performance, authenticated live T3/OBS/Docker qualification, Discord RPC and Grok
completion events remain unmeasured/unavailable; the table does not claim those gates passed.

### Additive APIs and shared paths

- `obs::Snapshot` adds scenes, microphones and two anchored `Clock`s; actions add
  `SwitchScene(String)` and `MuteInput { name, muted }`. Subscription mask is now **76**
  (Outputs | Scenes | Inputs), excluding meters. Mic state is seeded through `GetInputMute` and
  changes through `InputMuteStateChanged`; scene/input lists refresh only on their events.
- `Action::DockerMetrics` samples at most 16 running containers with
  `/stats?stream=false&one-shot=true`. `docker::Snapshot::usage` holds `Usage` by exact ID;
  lifecycle reconciliation invalidates samples. A missing prior CPU delta remains `None`.
- `tailscale::Snapshot` adds bounded peers and `peers_known`. Snapshot reads now include peers;
  no peer polling or extra watch traffic was enabled.
- `launcher::routine_runs()` returns at most 16 session-local receipts shared by launcher and
  widget submissions. `RoutineState` is `Sending | Accepted | Failed`; completion is not inferred.
- `credentials::prompt_t3_pairing()` is invoked only by an explicit Connect click. The bootstrap
  code is not persisted. `pairing::t3_blocking(endpoint, code)` makes one bounded loopback
  `/oauth/token` exchange and stores the resulting narrow bearer in Credential Manager.
  Protocol source: [T3 environment HTTP contract](https://github.com/pingdotgg/t3code/blob/a21d788752695764028e6d26ba924f3963e05806/packages/contracts/src/environmentHttp.ts).
- The host changes outside the provider/view directories are the new widget kinds and model
  field/deadline in `widgets/model.rs`, one sync hook and one action hook in `widgets/mod.rs`,
  and six dispatch arms plus the module declaration in shared `widgets/views/mod.rs`. The view
  owner committed that shared dispatch. The catalog-only baseline PNGs are the other necessary
  artifacts outside `views/integrations/`. No grid/edit, bars, flyouts, Studio or engine code was
  edited by this widget slice.

Implementation commits include `c222aa7`, `daf03e2`, `e6ae486`, `c6d6280`, `e8b84d2` and `47552cf`.
The shared view owner's `619734a` / `a5fbe77` also include the integration dispatch and some
concurrent integration view/artifact updates from the shared checkout. No push was made.
