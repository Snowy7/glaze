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
