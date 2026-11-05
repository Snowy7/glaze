# Native app integrations

`mypaper-integrations` is the shared provider library. Native views live in
`crates/shell/src/integrations`. There are no browser, Node, Docker CLI, or VPN helper processes.
One lease represents one visible consumer. A bar, widget and panel share the provider's single
subscription. Closing the last consumer cancels its IO, unregisters callbacks, and joins its lane.
Lock/display-off and settings changes cancel all subscriptions. A disconnect preserves stale data
and waits for the user's Refresh/reconnect action; there is no reconnect timer, polling, port scan,
client heartbeat, or automatic launch of a vendor application.

## Studio configuration contract

Store settings in `%APPDATA%\mypaper\integrations.json`. Write atomically; the shell watches the
directory and retains the last valid configuration after invalid edits. Keys use camelCase;
unknown fields (including `token`, `password`, and `key`) are rejected. File limit: 256 KiB.

```json
{
  "notifications": {
    "enabled": false,
    "showText": false,
    "liveVerified": false,
    "appIds": {}
  },
  "media": { "enabled": true },
  "launcher": { "enabled": true },
  "t3": {
    "enabled": false,
    "endpoint": "ws://127.0.0.1:3773/ws",
    "protocolVersion": 2,
    "liveVerified": false,
    "threadLinks": {}
  },
  "obs": { "enabled": false, "endpoint": "ws://127.0.0.1:4455" },
  "docker": { "enabled": false, "pipe": "", "containers": [] },
  "tailscale": {
    "enabled": false,
    "pipe": "\\\\.\\pipe\\ProtectedPrefix\\Administrators\\Tailscale\\tailscaled",
    "liveVerified": false
  },
  "grok": { "enabled": false, "routines": [] }
}
```

Media and launcher preserve their existing enabled behavior when this file is absent. Other
integrations default off. An enabled setting alone does not start a provider: a visible module or
open panel must consume it. The built-in `notifications` bar module is retained; new bar module
names in `bars.json` are `t3`, `obs`, `docker`, and `tailscale` (ordinary module style/click options
apply). Put these names in a top zone or taskbar dock side. No custom command module is required.

| Settings | Meaning / Studio control |
|---|---|
| Every `enabled` | Integration toggle. Turning off cancels current consumers. |
| `notifications.showText` | Explicit opt-in to retain/display toast title/body. Off shows app identity and a generic card. |
| `notifications.liveVerified` | Qualification switch, default false. Enable only after packaged listener event delivery is proved with another app foreground. Otherwise the center reads on open/Refresh; no persistent live badge is displayed. |
| `notifications.appIds` | Exact dock group key or lowercase launch path → Windows AUMID. Used only when the dock item has no AUMID; no name/pixel inference. |
| `t3.endpoint` | Explicit local `/ws` endpoint; no port scan. Runtime discovery on this machine found 3773, but other installations may differ. |
| `t3.protocolVersion` | Only 2 supported, pinned to researched source `a21d788752695764028e6d26ba924f3963e05806`. Incompatible servers fail visibly. |
| `t3.liveVerified` | Default false: one authenticated summary snapshot on open. Set true only after a no-maintenance idle wire check. There is no five-second ping task. |
| `t3.threadLinks` | Thread ID → copied T3 hosted HTTPS link. Missing links disable Open for that row; routes and OS protocol registration are never guessed. |
| `obs.endpoint` | Existing obs-websocket 5 server. Outputs and Scenes subscription mask 68; no meters, stats, previews or elapsed-time ticker. |
| `docker.pipe` | Explicit named pipe from the selected existing **local** Docker context. Empty asks for setup. Monitoring never starts Desktop/WSL. |
| `docker.containers` | Optional exact container IDs. Empty watches local containers, bounded to 128. |
| `tailscale.pipe` | Existing Windows service's local named pipe. Uses identification impersonation; preserves service access checks. |
| `tailscale.liveVerified` | Default false: one `/status?peers=false` snapshot on open/Refresh, with short-lived HTTP requests for explicit controls. Live requires qualification on the installed unstable LocalAPI. |
| `grok.routines` | Up to 32 `{ "id", "title", "url", "input" }` entries. URL is HTTPS without embedded credentials/query/fragment. IDs: 1–64 letters/digits/`-`/`_`; unique. `input` is optional JSON, request bound 64 KiB. |

The launcher exposes Discord's ordinary direct-message web link and configured Grok routines.
Existing game/app shortcuts are unchanged. A dock app's **Recent items and app actions** entry
opens a bounded Shell Recent list. Windows' public API cannot enumerate pinned/custom Jump List
categories or another app's task list. `launcher::recent(app_id, true)` additionally exposes
automatic Frequent for hosts that want it. Shell destinations preserve their arguments.

## Credentials and consent

Studio should link the library directly and call:

```rust
mypaper_integrations::credentials::write("t3", bearer_token)?;
mypaper_integrations::credentials::write("obs", obs_password)?;
mypaper_integrations::credentials::write("grok/daily-brief", webhook_key)?;
// remove(key) disconnects the credential; never echo read(key).text().
```

Credential targets are `mypaper/integrations/<key>` under the current Windows user's Credential
Manager. Values are UTF-8, at most 2560 bytes. They are never serialized in settings. The
`credential` example accepts a **private secret-prompt file**, not a token argument. Credential
rotation requires Refresh/reconnect or closing/reopening the consumer. Removing a stored
credential does not revoke an already authenticated vendor session; use the vendor's revocation UI.

For notification consent, call `notifications::request_access()` on Studio's **UI thread**, only
after the user's Grant access click. Await its returned WinRT operation. Packaged delivery needs
`userNotificationListener`; mypaper never substitutes a timer when events are unsupported.

T3 must be a separately authorized client with `orchestration:read`. Its MCP audience and injected
agent tokens are not accepted substitutes. The documented command is
`t3 auth session issue --scope orchestration:read --label mypaper --token-only`; transfer its result
through the private secret prompt into Credential Manager. Do not print it or paste it in JSON.
Native WebSocket Authorization headers are supported by the researched server, so bearer tokens
never appear in URLs. No T3 config, database or existing credential store is modified/read for auth.

Grok routine actions run **only on a click**. A 200 is “accepted”, not “finished”. Redirects are
disabled so an authorization header cannot be forwarded to another endpoint. Completion and
needs-input notices come through Grok's Windows toasts, when enabled in that application.

## Data, events and actions

Public library types:

- `Hub::new(Fn(Provider))`, `Hub::acquire(Provider, &config::Config) -> Result<Lease>`,
  `Hub::stop_all()`. `Lease::{snapshot, retry, action}`; cloning/dropping a lease owns demand.
- `Provider::{Notifications, Media, T3, Obs, Docker, Tailscale}`.
- `Snapshot { revision, availability, data, action_error }` and
  `Availability::{Connecting, Live, Cached, Stale(String), Unavailable(String)}`.
- `Data::{Notifications(notifications::Snapshot), Media(media::Snapshot), T3(t3::Snapshot),
  Obs(obs::Snapshot), Docker(docker::Snapshot), Tailscale(tailscale::Snapshot)}`.
- `Action::{Dismiss(Vec<u32>), Media(media::Action), Obs(obs::Action),
  Docker { id, start }, Tailscale { connected }}`. Actions are bounded; stale/disconnected sources
  reject commands. Commands are discarded across reconnects rather than replayed.

The wake callback is a typed provider invalidation, sent only when a snapshot/status/action result
changes. Consumers read its latest retained snapshot; there is no unbounded queued event history.
Bar/widget media adapters translate this same provider into the existing typed shell waker messages.
The media view is static metadata/playback/advertised controls and cached artwork (256 KiB maximum), without an interpolated progress tick.

Notification counts mean **toasts retained by Windows**, grouped by AUMID. They are not unread
message counts; dismissing a card does not mark a message read. Unknown/unavailable is not zero.
T3 attention requires a pending runtime request, not `waiting` alone. Completed, failed, cancelled,
interrupted and rolled-back runs remain distinct. Initial/reconnected snapshots are baselines;
later transitions are bounded to 16 and deduplicated by run/status. Summary sequence numbers may
skip because the server filters events; every reconnect obtains a new authoritative snapshot.
Docker subscribes before its seed and treats lifecycle events as invalidations, then obtains a
bounded current list. This avoids reconstructing counters from a potentially lost 256-event replay.
Tailscale watches only mask 24582 (InitialState, InitialPrefs, NoNetMap, InitialStatus); unsupported
versions fail visibly. No engine, peer-wireguard, stats or traffic watch is requested.

## Verification

Qualification evidence and remaining gates are recorded in `crates/integrations/VERIFICATION.md`.
`liveVerified` is an explicit deployment qualification flag, not evidence by itself. Until the
packaging/auth/idle gates pass, keep notification/T3/Tailscale persistent badges disabled.

Protocol references and exclusions: [integration research](research/integrations.md),
[T3 authentication](https://github.com/pingdotgg/t3code/blob/a21d788752695764028e6d26ba924f3963e05806/docs/internals/environment-auth.md),
[OBS protocol](https://github.com/obsproject/obs-websocket/blob/master/docs/generated/protocol.md),
[Tailscale LocalAPI](https://github.com/tailscale/tailscale/blob/main/client/local/local.go).
