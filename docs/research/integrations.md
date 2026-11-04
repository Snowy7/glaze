# App integrations for native mypaper

Researched **2026-11-05**, against branch `native`. This is an integration feasibility study,
not an implementation or a measurement report. CPU and memory assessments below describe the
proposed adapter, including its helpers; they are not measured vendor-app footprints. Public
documentation does not establish what is enabled in the user's installed app or organization.

## Recommendation

Build shared Windows providers and a few small native protocol clients. Do not build a second
messaging client or a general app-scraping service. The useful first batch is **notifications,
media, launcher actions/recent items, T3 Code, OBS, Docker, and Tailscale**. Notifications
and T3 have explicit verification gates before they qualify for a persistent live bar.

For the three named requests:

- **Discord:** notification cards and launcher actions are practical now. Rich voice controls
  have a documented local pipe, but production access requires Discord approval. An open pipe
  is not permission to read everything.
- **T3 Code:** source-defined, authenticated subscriptions expose the right thread summaries.
  A native client is feasible without vendor changes. Its existing JavaScript client sends
  five-second pings, so copying that client would break our rule.
- **Grok Bot:** this is a real, separate desktop product. `grok.bot` redirects to xAI's
  official product page. Routine-start webhooks and Windows notifications are useful today;
  no public, general Bot-status subscription was found in the reviewed documentation.

Sources and limits for each conclusion follow below.

## Fit with this repository

The [native plan's Rules and Processes](../../plans/council/native-plan.md) put rendering in
software Direct2D, keep the shell free of a hardware graphics device, and retain three steady
product processes. A surface changes only when its display list changes. JSON-RPC providers
exist only while used. The stricter requirement for this study is **no recurring polling**, even
where older repository services or fetchers use timers.

[writing-plugins.md](../writing-plugins.md) points to the actual
[native service guide](../writing-widgets.md). Its existing model is executable providers,
newline-delimited JSON-RPC 2.0 over stdio, declared methods/events, and native compiled views.
Providers start on demand, suspend/resume, and stop after 30 seconds without users. They run as
the user, with a kill-on-close Job object; manifest permissions are descriptive, not a sandbox.
There is no HTML or JavaScript rendering plugin API.

Apply that model as follows; these are design recommendations, not existing API names:

1. Share one provider connection across its bar, widget and flyout consumers. Put inexpensive
   built-in Windows providers in the shell's service library; use optional Rust executables
   for independent app adapters. Do not create an always-running executable for every icon.
2. Translate vendor protocols into a small typed snapshot and declared change events. The
   vendor's WebSocket/MCP protocol is **not** automatically our stdio plugin protocol.
3. Subscribe and obtain an initial snapshot with race handling. Use sequence/cursor replay
   where available; resnapshot after a gap. Do not create counters by assuming no event was lost.
4. Keep bounded metadata, not message histories, token streams, logs, avatars or browser state.
   Return `unavailable` or `stale` on disconnect; do not present a disconnected source as zero.
5. Stop production and release network subscriptions on suspend. A pinned live bar is a real
   consumer; a closed flyout is not. Native UI code still belongs in `crates/shell`/`crates/ui`.
6. Attempt connection at activation and after a relevant OS/network/app event or explicit retry.
   No port-scanning loop, process-enumeration loop, endless reconnect backoff, periodic CLI
   command, or file-mtime poll. Existing window events can trigger a connection attempt;
   Windows service and directory notifications cover appropriate background services/files.
   None guarantees discovery of every headless server: allow an explicit endpoint and retry.
   [WinEvent][win-event], [service notifications][win-service], [directory notifications][win-dir].
7. Use Windows credential protection for secrets, same-user ACLs on our pipes, external-browser
   OAuth, and narrow read grants. A model's explicit report is useful, but is not authoritative
   lifecycle telemetry. Never interpret arbitrary model output as a launcher command.

### What “zero idle” means here

- **E — event candidate:** callbacks or an open blocking stream, without a client polling timer.
  It can sleep between actual events. This is a necessary architecture, not proof of zero CPU.
- **U — on demand:** one bounded read/action when the user opens or clicks something, then no
  background work. Cached content is labeled with its freshness.
- **M — maintenance traffic:** push with heartbeats, forced reconnects, token/subscription
  renewals, or server keepalives. It is not polling for data, but still wakes the client while
  the business state is unchanged. It does **not** meet a literal zero-idle promise.
- **X — excluded:** unavailable permissions, unsupported/private contracts, polling-only live
  state, or a resource/ToS conflict.

WebSocket does not imply E. Measure idle wire traffic and thread wakeups. If mandatory
maintenance cannot be removed, make the integration U, or keep it outside the default batch.
Do not silently weaken the rule because an adapter reports “0.0%” in Task Manager.

Effort estimates are engineering judgment: **S** is a contained adapter/action; **M** adds a
protocol, credentials, state reconciliation or packaging; **L** adds a substantial client,
backend or unstable automation. They exclude waiting for vendor/admin review. “Low ToS risk”
means a documented, authorized integration route; it is not a blanket clearance for every use.

## Ranked opportunities

Scores are an explicit prioritization heuristic: **value × feasibility ÷ cost**, each 1–5,
with feasibility 0 for an excluded scope. Cost includes implementation, ongoing resource cost
and maintenance. Scores assess the **scoped feature in the table**, not an imagined complete
integration. They are not benchmarks. Generic notification/launch fallbacks are shared, rather
than counted as a new rich integration for every app. Ties are ordered by usefulness here.

| Rank | Candidate and useful scope | V | F | C | Score | Idle fit / effort | Feasible without vendor changes? |
|---:|---|---:|---:|---:|---:|---|---|
| 1 | Shared SMTC, including local Spotify playback | 5 | 5 | 1 | 25.0 | E / S; substantial reuse | Yes, for sessions the player publishes |
| 2 | Launcher: automatic Recent/Frequent items | 4 | 5 | 1 | 20.0 | U / S | Yes; excludes pinned/custom Jump List categories |
| 3 | Steam game shortcuts and launch actions | 3 | 5 | 1 | 15.0 | U / S | Yes; excludes live downloads/friends |
| 4 | OBS recording/streaming/scene state and controls | 4 | 5 | 2 | 10.0 | E candidate / M | Yes; enable built-in WebSocket and authenticate |
| 5 | Docker container lifecycle and failure badge | 4 | 5 | 2 | 10.0 | E / M | Yes, against an already-running local engine |
| 6 | Home Assistant selected entities and actions | 4 | 5 | 2 | 10.0 | U default; live is M / M | Yes on demand; server heartbeat prevents strict live idle |
| 7 | GitHub PR/check/inbox flyout, fetched on open | 4 | 5 | 2 | 10.0 | U / M | Yes; live repository push needs a receiver |
| 8 | VS Code/Cursor diagnostics, tasks, selected agent hooks | 4 | 4 | 2 | 8.0 | E / M | Yes, through opt-in supported hooks/extension APIs |
| 9 | Grok Bot: start a saved routine and open the app | 3 | 5 | 2 | 7.5 | U / S–M | Yes; account/routine setup, not general status access |
| 10 | Shared Windows per-app notification counts | 5 | 4 | 3 | 6.7 | E **gated**, U fallback / M | Yes in principle; package/event behavior must be proved |
| 11 | T3 Code live thread summaries and attention | 5 | 4 | 3 | 6.7 | E **gated**, U fallback / M | Yes; native client, explicit scoped auth, version pin |
| 12 | Google Calendar agenda and join links | 4 | 5 | 3 | 6.7 | U; cloud push is M / M–L | Personal use yes; public OAuth review may be needed |
| 13 | Outlook Calendar agenda and join links | 4 | 5 | 3 | 6.7 | U; cloud push is M / M–L | Yes with consent; tenant policies can restrict it |
| 14 | Claude Desktop: MCP reports; Code-session hooks | 3 | 4 | 2 | 6.0 | E/U, workflow-limited / M | Yes; no general Chat/Cowork session feed established |
| 15 | ChatGPT Desktop: MCP reports; local Work/Codex hooks | 3 | 4 | 2 | 6.0 | E/U, workflow-limited / M | Yes for supported local configuration; directory approval differs |
| 16 | Tailscale local connection/exit-node state | 4 | 4 | 3 | 5.3 | E candidate / M | Yes; LocalAPI watch is explicitly unstable |
| 17 | Slack scoped mentions/messages and status actions | 3 | 3 | 3 | 3.0 | M live, U otherwise / M–L | Yes with workspace installation; not all-user inbox access |
| 18 | Telegram exact account unread/mention state via TDLib | 3 | 3 | 5 | 1.8 | Push, extra client; excluded by default / L | Yes technically; substantial session/network/cache cost |
| 19 | Discord production voice controls/all-client activity | 5 | 0 | 3 | 0.0 | X pending approval / M after access | No for unrestricted public distribution today |
| 20 | Teams old local meeting-control API | 4 | 0 | 3 | 0.0 | X, conflicting/retirement evidence / M | Do not assume availability; Graph is a different feature |
| 21 | WhatsApp personal-account live inbox | 4 | 0 | 4 | 0.0 | X / L | No supported general desktop inbox API found |
| 22 | Universal taskbar-overlay unread extraction | 4 | 0 | 5 | 0.0 | X / L | No supported getter/event/numeric contract found |

The per-integration evidence below also covers the richer variants that the ranked scope
intentionally excludes.

## Shared Windows mechanisms

### Notifications: many apps, one provider

**Mechanism/data/actions.** `UserNotificationListener` provides current Windows notifications,
source app identity and visual text. `NotificationChanged` signals additions/removals. Group
by AUMID, map it to dock apps, and offer a per-app flyout plus individual dismiss. This covers
Discord, Grok Bot, Slack, Teams, Telegram and WhatsApp **when those apps emit Windows toasts**.
The listener does not establish their internal read state. [Notification object][win-usernotification],
[event contract][win-notification-event].

**Auth/approval/ToS.** Microsoft's documented flow declares `userNotificationListener` in a
package manifest and requests user access on the UI thread. User consent is required and
revocable; there is no app-vendor OAuth. Low platform-integration risk. Exposing notification
text remains an explicit user choice. [Listener guide][win-notifications].
The permission request is a separate asynchronous API. [Access request][win-notification-access].

**Important local blocker.** The repository's
[notification provider](../../crates/bars/src/flyouts/notifications.rs:1) records that on build
26300 the unpackaged app can read/remove toasts, but event registration fails with
`0x80070490`. The native shell currently
[recounts every 15 seconds](../../crates/shell/src/bars/services.rs:308). That implementation
does **not** satisfy this research's rule. Reuse its models, not its timer fallback.

**Cost/effort/feasibility.** M. A shared native event listener is an E candidate, independent of
individual vendors. First prove package identity/capability and continuous event delivery while
another application is foreground. Microsoft also documents a notification-triggered background
task; if needed, assess its activation/process cost instead of introducing an unnoticed fourth
steady process. If the event route fails, use U: snapshot on flyout open/manual refresh, with
no persistent live badge. [Foreground and background delivery][win-notifications].

**Semantics to preserve.** The badge means “notifications currently retained in Windows,” not
“unread messages.” Muted channels, suppressed notifications, replaced/grouped toasts, manual
dismissal, Windows history limits, and reading on another device break equivalence. Removing a
toast does not mark its originating message read. Do not infer exact mentions from localized
text. The listener contract does not provide arbitrary replay of another app's toast buttons
or reply activation; default to opening the app, and use a specific destination only when its
documented link is available. `ToastNotificationHistory` is notification-history management,
not a documented cross-app push replacement for the consented listener.
[History API][win-toast-history].

### Taskbar badges, overlays and tray icons

`ITaskbarList3::SetOverlayIcon` is a **setter** used by an application to give its taskbar button
an icon and accessibility description. Its icon is a bitmap, not a standardized unread number.
The reviewed documented interface provides no matching cross-app getter or change subscription.
Replacing/hiding the taskbar does not make mypaper the recipient of this API.
[Overlay contract][win-overlay].

There may be app/version-specific accessible labels on Explorer's buttons. An optional,
user-triggered UI Automation experiment can inspect one, but neither a numeric label nor change
events are guaranteed. UIA explicitly warns that providers may omit events. A hidden/rebuilt
taskbar and app changes make this a poor default badge backend. No OCR, screenshot loop,
Explorer-memory scraping, DLL hook or undocumented taskbar-message interception.
[UIA events][win-uia].

The existing [tray host](../../crates/bars/src/tray_host.rs) is a separate Shell notification-area
mechanism. App-updated tray icons can be mirrored as icons; their pixels/tooltips are not a
reliable typed inbox count. **Effort L for a universal extractor; X for the default product.**

### Media: share the existing SMTC service

**Mechanism/data/actions.** Use `GlobalSystemMediaTransportControlsSessionManager`, its
session/current-session events, and each selected session's metadata/playback events. Sessions
expose source app identity, metadata, playback state and supported transport commands.
[Manager][win-media-manager], [session controls][win-media-session].

**Auth/ToS.** Windows access, no Spotify or other player OAuth. Packaged apps declare the
`globalMediaControl` capability. Low risk through the OS contract; each player chooses what to
publish and may refuse a command. Do not promise arbitrary queue/library or remote-device state.
[Session contract][win-media-session].

**Cost/effort/feasibility.** E, S, feasible today. The repository already has
[an event-based MTA worker](../../crates/bars/src/media.rs:1), active only while used, with an
80 ms debounce after an event and an indefinite channel wait otherwise. Share it across all
media surfaces. The default bar is static title/artist/playback state with previous/play/next.
No continuously interpolated timeline, marquee, spectrum or album-art download loop. A seek
action can be offered on interaction when the session advertises it.

### Launcher: Jump Lists and recent destinations

**Mechanism/data/actions.** `IApplicationDocumentLists::SetAppID` plus `GetList` retrieves an
app's automatically maintained Recent or Frequent destinations as Shell items/links. Open the
destination through Shell APIs. Use this on demand for a dock context flyout or launcher result.
[GetList contract][win-recent-list].

**Limits.** The public interface cannot enumerate the **pinned category, custom categories or
the task list**. An item that was Recent/Frequent and then pinned can still be returned by
`GetList`, but this does not expose its pinned membership or the complete pinned category.
`ICustomDestinationList` creates/manages the caller's Jump List; it is not a
universal read API for every other application's tasks. Do not parse undocumented
`AutomaticDestinations`/`CustomDestinations` databases to promise full Jump List parity.
[Document-list limits][win-recent], [GetList pinned-item nuance][win-recent-list],
[taskbar model][win-taskbar].

**Auth/ToS/cost/effort.** No OAuth or vendor approval; normal per-user Shell access, low risk,
U, S. Read on context-menu/launcher open, keep a bounded set, and release COM results on close.
Respect Windows recent-item privacy settings; an empty list is valid. Explicit actions such
as “open repository,” “launch game” or “run saved routine” can live beside Recent items, but
are curated mypaper commands, not harvested Jump List tasks.

### UI Automation and process/window discovery

UIA supports scoped property/structure/notification callbacks and can invoke exposed controls.
It is worth a **manual or narrowly scoped fallback**, not a universal app-state protocol:
virtualized/offscreen controls may not exist and events may be absent. Accessibility activation
can increase the target application's work, especially in Electron apps. Measure both ends.
[UIA event semantics][win-uia].

Use existing window events to identify/open an app. `WINEVENT_OUTOFCONTEXT` keeps the callback
outside its address space; `WINEVENT_INCONTEXT` explicitly maps a DLL into it and is disallowed.
No global accessibility-tree walker, synthetic keypress loop, or process-title parser for
authoritative mute/agent status. [WinEvent execution modes][win-event].

## Named integrations

### Discord

**Useful UX.** A bar voice indicator, channel/participants flyout, self-mute/deafen controls,
notification cards and “open Discord/channel.” Distinguish OS microphone mute from Discord's
self-mute/deafen; muting a Windows endpoint does not tell us Discord's state.

**Mechanism.** The official RPC transport is Windows named pipes
`\\?\pipe\discord-ipc-{n}`, with framed JSON and a client-ID handshake. Legacy localhost
WebSocket RPC is deprecated/private-beta-only. Relevant operations/events include
`GET_SELECTED_VOICE_CHANNEL`, `GET_VOICE_SETTINGS`, `SET_VOICE_SETTINGS`,
`VOICE_CHANNEL_SELECT`, `VOICE_STATE_*` and `VOICE_SETTINGS_UPDATE`.
[Official RPC][discord-rpc].

**Auth and approval.** Create a developer application, use authorized RPC authentication, and
obtain the exact scopes needed. `rpc`, `rpc.voice.read`, `rpc.voice.write` and
`rpc.notifications.read` are approved-partner scopes. Development tester access is not public
distribution approval. `messages.read` does not remove the surrounding RPC restrictions.
[OAuth scope table][discord-oauth].

**What is not unlocked.** Rich Presence `SET_ACTIVITY` publishes our activity; it does not read
every other game's activity. `activities.read` is explicitly unavailable to apps in the current
scope table. A server bot sees its authorized guild data, not the person's entire DM inbox or
client unread state. `NOTIFICATION_CREATE` is a notification feed, not an authoritative global
unread counter. [Scopes][discord-oauth], [RPC events][discord-rpc].

**Current SDK caveat.** The Social SDK added read-only desktop voice settings in September
2026, but `GetVoiceSettings` and its update callback still require an **approved Social SDK
integration**. Its own calls/lobbies are not automatically all calls in the Discord desktop
client. Do not treat that release as an approval-free route.
[SDK release][discord-sdk-release], [SDK method/access contract][discord-sdk-client].

**ToS risk.** Low for approved RPC and ordinary Windows notifications. High for extracting the
user token, using an unofficial user gateway/self-bot, borrowing another application's client
ID, or injecting a modified client. Discord explicitly forbids automated normal user accounts
outside its approved OAuth2/bot APIs. [Self-bot policy][discord-selfbot].

**Cost/effort/feasibility.** M after approval; local blocking pipe subscriptions are an E
candidate without a second Discord client. Send no recurring state reads, and skip speaking
activity animation. Verify the documented single-controller voice-setting lock and restoration
on disconnect before offering toggles: the RPC documentation says settings can reset when the
controlling app leaves. Without vendor cooperation, ship shared Windows notification badges
and launch/focus actions, not a promised public voice-control module. [Voice-setting behavior][discord-rpc].

### T3 Code

**Useful UX.** A bar with running and attention counts; a flyout with thread title, project,
provider, state and a few recent completions; one notification when a run finishes or needs
input; click to open the correct environment/thread. Never stream the transcript into a badge.

**Evidence/version.** This section uses upstream `pingdotgg/t3code` commit
`a21d788752695764028e6d26ba924f3963e05806`, not an assumption about older T3 builds. T3's
desktop app bundles an environment server; its clients use authenticated RPC, and subscriptions
are intended to avoid loading every thread's history. [Architecture][t3-architecture].

**Mechanism/data.** `/ws` serves Effect RPC with JSON serialization and a protocol-compatibility
check. `orchestration.subscribeShell` is a streaming RPC. Its shell projection contains titles,
project/provider IDs, status, active/latest run IDs and timestamps, pending runtime-request
summary, and recent visible-message metadata. Consume the shell snapshot and sequenced changes;
request details for the selected row only. `ShellStream` already coalesces updates and supports
omitting idle subagents/older settled threads. [RPC contract][t3-rpc],
[projection schemas][t3-schemas], [stream implementation][t3-shell-stream], [server route][t3-ws].

**Attention/completion semantics.** A pending permission/user-input request is attention;
`waiting` alone may mean waiting on another agent, not a question for the user. Keep failed,
canceled and completed distinct. A thread can finish several runs, so deduplicate completion by
run ID, not thread ID. The first snapshot establishes the baseline and must not emit hundreds
of historic completion toasts. These are proposed derivations from the schema, not additional
vendor fields. [Projection fields][t3-schemas].

**Auth/approval.** Pair as a distinct user-approved client with `orchestration:read`; do not
copy a provider's injected MCP token or the desktop's stored credentials. The source supports
scoped environment sessions and short-lived WebSocket tickets. Its external-MCP OAuth grants
are a separate audience: **MCP-client sessions are rejected on ordinary HTTP/WebSocket routes**.
A read-only MCP login cannot simply be reused on `/ws`. Configure the endpoint explicitly;
do not assume every desktop build uses port 3773. [Auth boundaries][t3-auth].

**Actions/MCP.** T3's app-owned `/mcp` can list/read threads and perform authorized thread
operations, with a documented outside-agent sign-in flow. This is useful for on-demand reads,
but repeated `t3_thread_list`/wait calls are not a lightweight event subscription. Start with
read-only mypaper access and open-thread actions. Use the app's copied/returned hosted thread
link rather than inventing a route or assuming the internal `t3-thread` syntax is OS-registered.
[MCP contract][t3-mcp], [outside agents][t3-outside], [link construction][t3-links].

**Cost and hard gate.** M, feasible without vendor changes using the published source contract.
T3's shared client explicitly sends a ping every **five seconds**, with a 15-second timeout;
using that client adds a runtime and idle wakeups. Build a small Rust transport, subscribe only
to summary state, and verify that loopback operation without a recurring client ping is accepted
and that the server does not impose equivalent idle traffic. This last point is **not proved
by the research**. If it fails, the compliant first version is a U flyout/launcher: connect,
read one snapshot and disconnect, not a live bar with a hidden five-second timer.
[Client heartbeat][t3-heartbeat], [client session][t3-session].

**ToS/maintenance risk.** Low for owner-authorized access to this MIT-licensed source, but
medium compatibility risk: these are T3's actual internal client contracts, not a promised
stable third-party SDK. Pin supported protocol versions and fail visibly on incompatibility.
No database scraping, transcript-file watching, extra T3 server or model calls to infer state.
[Repository/license][t3-repo], [protocol gate][t3-ws].

### Grok Bot, grok.bot, and the Grok chat app

**Identity resolved.** `https://grok.bot` currently redirects to `https://x.ai/bot`. Official
documentation distinguishes **Grok Bot**, a persistent-agent desktop/mobile product, from
the **Grok chat app**. Windows is explicitly supported. It is not necessary to guess that
“grok bot” means a Discord bot or an unofficial Grok wrapper.
[Product/redirect destination][grok-product], [product distinction][grok-faq].

**Useful data.** Finished work, needs-input/approval notices, unread activity, working state,
and an action to open a Bot or start a routine. The app documents those attention states and
Windows system notifications for finished/waiting Bots. That makes the shared notification
provider a supported, useful bridge, subject to delivery settings; it does not expose a complete
live Bot list. [Attention and notification behavior][grok-notifications].

**Documented action available today.** A saved routine can have a webhook URL and key, copied
from the desktop routine. Send an HTTP POST with `Authorization: Bearer <key>` and optional
JSON input. A `200` means the run was accepted/started, **not that it finished**. A launcher
entry such as “Grok Bot: run daily brief” can make one request, show acceptance, and release its
worker. The action spends Bot usage and may do real work, so run it only on an explicit click.
[Routine webhook contract][grok-routines].

**What was not found.** The reviewed Bot overview, FAQ, settings, routine and developer docs
do not publish a general local pipe, app-status WebSocket, CLI list/watch, outgoing lifecycle
webhook, or consumer Bot-session MCP server for third-party desktop monitors. This is a scoped
negative finding, not proof that none exists internally. The UI's run history is not an API.
[Bot documentation entry point][grok-overview], [routines][grok-routines].

**Other xAI interfaces are different.** The model API and its streaming responses are for
requests made through the API; they do not establish access to existing Grok/Grok Bot account
conversations. Grok custom MCP connectors let Grok call **our** tools and require a public
reachable endpoint; they do not reverse into an app-state feed. Enterprise inference hooks are
a governed model-request security path, not a casual desktop status API.
[API overview][grok-api], [connector direction][grok-connectors], [inference hooks][grok-inference].

**Auth/ToS/cost/effort/feasibility.** S–M for U webhook actions and shared notification cards;
no vendor engineering change needed, but the user must configure the account/routine, enable
notifications and provide the webhook credential securely. Low risk for these documented
features under the account's terms. A complete live monitor is X until a supported feed is
identified. Do not extract Cursor/SSO cookies, attach a debugger to the app, scrape its cloud
RPC or poll its UI. Optional explicit “report result to mypaper” tools would report only what
the Bot sends, with extra cloud/connection cost. [Product terms][grok-terms].

### Spotify

**Default.** Use shared SMTC for local title/artist, playing/paused, transport controls and
session choice. E/S, no Spotify authorization or vendor cooperation, conditional on the running
Spotify client publishing the session. This is already within the repository's media design;
avoid a separate Spotify resident provider. [Windows session API][win-media-session].

**Richer actions.** Spotify's Web API can read playback and perform authorized player/library
actions. It uses OAuth; native clients should use Authorization Code with PKCE and only the
read/playback-modification scopes required. Cloud playback reads are HTTP snapshots, not a
documented general playback-change subscription, so they belong in a U flyout, not a polling
now-playing bar. Device control is not a reason to embed the Web Playback SDK/browser.
[Playback endpoint and scope][spotify-playback], [PKCE][spotify-pkce].

**Approval/ToS/cost.** M for Web API extras; low risk through the official API with its policy
requirements, higher approval/distribution cost than SMTC. Current quota docs require Premium
for the development app owner and allow up to five authenticated users in development mode;
extended quota requires approval. Do not repeat the obsolete one-client-ID limit: Spotify's
July 2026 update increased it to 25, sharing quota per developer. These restrictions affect
Spotify Web API usage, not ordinary SMTC. [Quota modes][spotify-quota], [July update][spotify-july].

### Steam

**Useful UX/mechanism.** Launcher game entries from existing Start-menu/user-provided shortcuts;
invoke their Steam URI through the OS. Valve documents `steam://run/<appid>` behavior. This
needs no Web API key or Steamworks initialization. A library flyout can initially be just
the user's selected games, with no account synchronization. [Valve launch contract][steam-apps].

**Richer data limits.** Live downloads, friends/voice and current game are different problems.
Steamworks is an application SDK with initialization/identity requirements; its conventional
callback dispatch is called repeatedly, with Valve recommending more than 10 Hz. It is not a
drop-in, timer-free observer for an unrelated desktop shell. Do not spoof another game's AppID
or reverse engineer the client IPC. Local library manifest readers can be event-driven, but
their private formats are an extra compatibility commitment, not required for the first batch.
[Initialization and callbacks][steam-api].

**Auth/ToS/cost/effort/feasibility.** No additional auth for U shortcut launching; normal Steam
login/purchase checks still apply inside Steam. Low risk, S, feasible today. Steam remains closed
until the user launches something. Rich live client monitoring is X by default; do not keep
Steam or a Steamworks callback pump running just for the shell.

### GitHub

**Useful UX.** A developer flyout for review requests, PR checks and failures, with “open PR,”
“open repository,” and optional explicit mark-read actions. Read on open through REST/GraphQL
or an installed `gh` CLI that exits after the request. No extra CLI runtime stays resident.

**Push versus inbox.** GitHub documents personal notifications as optimized for polling with
`Last-Modified` and `X-Poll-Interval`; conditional requests remain polling. That endpoint does
not accept fine-grained PATs or GitHub App tokens, and documents classic PAT authentication with
`notifications`/`repo` scopes. Do not promise that one fine-grained token covers every feature.
[Notification endpoint][github-notifications].

**Live alternative.** Repository/GitHub App webhooks can push PR/review/check/workflow events
to a receiver, but they are not a complete personal-notification/read-state feed. A desktop
behind NAT needs a public receiver or relay. A backend must verify signatures, deduplicate
deliveries and forward only scoped metadata; its connection, hosting and maintenance count
toward cost. A relay is optional future infrastructure, not part of the native default.
[Delivery model][github-delivery], [event coverage][github-events].

**Auth/ToS/cost/effort/feasibility.** U/M for the useful initial flyout; official user auth or
an existing user's CLI credential flow, organization SSO/approval where required, low risk.
M–L for webhook infrastructure. Feasible without vendor changes, with permissions to the
selected repositories. Never claim an always-live inbox while periodically calling `gh`.

### Slack

**Mechanism/data/actions.** Socket Mode gives a Slack app an outbound WebSocket for scoped
Events API deliveries without hosting a public endpoint. `app_mention`, messages in authorized
conversations and configured status/DND actions can feed an attention flyout. An app-level
token with `connections:write` obtains the socket URL; bot/user OAuth tokens separately govern
data and actions. [Socket Mode][slack-socket], [token types][slack-tokens].

**Limits/auth/approval.** Install an app in each workspace; administrators can restrict this.
A bot's mentions/DMs are the bot's scope, not all DMs sent to the human. Exact human unread
state depends on user-token visibility, read markers and event coverage. Slack documents
unread fields only in certain conversation/method contexts; don't invent a universal unread
event or count all received messages as still unread. Do not reuse the desktop app's private
session token/cookie. [Conversation state][slack-conversations].

**Cost/effort/ToS/feasibility.** M–L, official API use is low risk with proper scope and
workspace consent. Socket Mode connections refresh regularly, even without business activity;
heartbeat/reconnection and TLS make it M, not strict E. It is feasible without vendor changes
as an opt-in integration, but the default is shared Windows notifications plus U app/channel
actions. A publicly distributed desktop app must not ship a shared app-level secret to every
installation; per-user/workspace setup or a backend changes the product and its cost.
[Connection lifecycle][slack-socket].

### Microsoft Teams

**Useful UX.** Notifications and meeting join actions now; potentially presence, in-call,
self-mute/camera and leave controls if a supported path is proven. OS endpoint mute is not
proof of Teams self-mute, and a calendar meeting is not proof of an active call.

**The local-API trap.** Microsoft still publishes “Manage API” device pairing instructions
for local controls. However, Elgato's own integration documentation, updated June 2026, says
the Teams Stream Deck plugin was removed on **2025-12-12**, Microsoft discontinued the required
API, and existing actions no longer function. That is direct evidence from the integration
maintainer and conflicts with Microsoft's still-live help page. Treat the old local WebSocket
route as **unconfirmed/retired for new work**, not a ready near-zero solution. This research
did not test an installed Teams version or find a replacement public local specification.
[Microsoft pairing help][teams-local-help], [Elgato retirement notice][teams-retirement].

**Supported cloud route.** Microsoft Graph presence change notifications use webhooks and
delegated work/school `Presence.Read.All`, not personal Microsoft accounts. Presence
subscriptions expire after at most an hour. It exposes availability/activity, not a control
channel for the user's local microphone/camera. Graph message subscriptions are another
permission/coverage problem; they do not automatically yield a desktop unread badge.
[Presence subscriptions][teams-presence].

**Auth/ToS/cost/effort/feasibility.** S for shared toast/open/join actions; M–L for authorized
Graph plus public receiver, renewal and tenant consent. Low risk on documented routes. Rich
local controls are X until verified on a supported interface; cloud presence is M with an
external service, not a zero-maintenance local substitute. Do not mine Teams data from Outlook
mailbox storage: Microsoft explicitly says that is unsupported. [Storage boundary][teams-storage].

### OBS Studio / obs-websocket

**Mechanism/data/actions.** OBS 28+ includes obs-websocket; no injected plugin/runtime is
needed. Enable it in OBS and connect to loopback (default port 4455). Identify/authenticate,
request initial recording/stream/scene state, then subscribe to `RecordStateChanged`,
`StreamStateChanged`, `CurrentProgramSceneChanged` and selected input-mute changes. Offer
record start/stop/pause, stream start/stop, scene selection and input mute.
[Built-in server/setup][obs-setup], [protocol][obs-protocol].

**UX.** Static red REC/LIVE bar labels and scene name; a flyout with explicit controls. A
notification for unexpected recording/stream termination. Avoid a blinking red light or
per-second duration counter; the flyout can calculate elapsed time when opened.

**Auth/ToS.** User enables the server and supplies its generated password. The protocol uses
a challenge/salt authentication exchange. Keep it local and preserve the existing server's
access settings. Low risk, no OBS approval requirement. [Authentication][obs-protocol].

**Cost/effort/feasibility.** E candidate/M, feasible today. One bounded native WebSocket client,
only while the module is used and OBS is running. **Do not subscribe to `InputVolumeMeters` or
other high-volume telemetry**, fetch previews, or poll `GetStats`/`GetStreamStatus`. State
events suffice. Confirm the chosen client/server do not introduce periodic idle heartbeats;
otherwise use bounded connect/read-or-act/disconnect operations on interaction.
[Event subscriptions][obs-protocol].

### Home Assistant

**Mechanism/data/actions.** Connect to the existing server's `/api/websocket`, authenticate,
and `subscribe_entities` with a small `entity_ids` allowlist. It sends initial compressed
state and changes; the current official documentation recommends it over all-state event
traffic. `call_service` invokes selected light/switch/scene actions.
[WebSocket contract][ha-websocket].

**UX.** A compact widget for three selected lights/scenes or one thermostat; an “away/home”
bar status and small control flyout. Show unavailable entities explicitly. No embedded HA
dashboard, camera feed, all-entity mirror or continuous temperature/energy graph.

**Auth/ToS.** User OAuth/token, or a user-created long-lived access token in an optional
native provider. Permissions are the HA user's permissions; an entity allowlist in mypaper
does not reduce the authority of the token. Low risk with the authorized first-party API,
no vendor cooperation needed. [Authentication phase][ha-websocket].

**Cost/effort/feasibility.** M, feasible without vendor changes as **U**, against an
already-running server. The server source constructs `WebSocketResponse(heartbeat=55)`: even
an unchanged LAN subscription has idle connection-maintenance traffic. Disabling our client's
own timer cannot remove that server behavior. Therefore a persistent live connection does
**not** meet the strict rule on the inspected server; use connect/read-or-act/disconnect and
label cached widget state. [Server heartbeat][ha-server].

Subscription traffic also depends on selected entities: a once-per-second power sensor is
costly even though it is push. A future supported low-maintenance transport would need its
own evidence; do not patch HA or add a custom broker merely to make the badge appear cheap.
Do not launch HA/Docker for this widget. [Entity filtering][ha-websocket].

### VS Code / Cursor

**Data/actions.** Workspace name, diagnostic-error count, running tasks and task exit status
are available through VS Code's public APIs (`onDidChangeDiagnostics`, task start/end events,
workspace events). An opt-in extension can send small changes to a same-user mypaper pipe.
Use `code --reuse-window`/`--goto` for documented open-file actions.
[Extension API][vscode-api], [CLI][vscode-cli].

**Agent status.** Cursor documents command hooks including `beforeSubmitPrompt`, `stop`,
session and subagent lifecycle events. A short-lived Rust helper can forward IDs/status and
exit. Do not hook every token or thought. VS Code now has harness-specific hooks: Copilot,
Claude and Codex use different configuration/event contracts. Verify the selected target and
version; the official page was updated November 4, 2026. Hooks do not imply a global API for
all preexisting agent sessions. [Cursor hooks][cursor-hooks], [VS Code target rules][vscode-hooks].

On Windows, Cursor invokes command hooks through PowerShell. A native reporter does not
eliminate that transient shell startup; count its cost and attach only to useful lifecycle
transitions. [Windows hook runner][cursor-hooks].

**Auth/ToS/cost/effort/feasibility.** M, E candidate, low risk through installed supported
extensions/hooks; user installation/workspace trust and organization policy apply. Reuse the
editor's **existing extension host**; do not inject a DLL, add a new interpreter or ship a
background Node bridge. A supported extension is an optional app extension, not a new runtime
in another process. If even extensions are unwanted, use command hooks/launcher actions only.

Hook events are observations, not permission decisions: our reporter must leave the agent's
normal behavior unchanged. Mark the state unknown after a crash/lost connection; a missed
`stop` must not produce “running forever.” Remote hooks run on the remote host and cannot
directly write the desktop's pipe without an explicitly configured transport. S for launch
actions, M for the event bridge; no vendor engineering change required.

### Telegram

**Cheap version.** Shared Windows notifications plus `tg://` or `t.me` chat/message/draft links.
S/U or shared E notification provider, no separate Telegram authentication. Notification count
still means Windows toasts, not Telegram unread. [Official links][telegram-links].

**Rich version.** Telegram's TDLib is an official, asynchronous native client library with
chat/read/mention updates, so exact account unread/mention state and message actions are
technically possible. It is **another authenticated Telegram client**, not an API exported by
the running desktop app. Obtain our own `api_id`/`api_hash` and complete user login/2FA; do not
read the official desktop's session files. A Bot API token is not the person's account access.
[TDLib model][telegram-tdlib], [app registration][telegram-app].

**ToS/cost/effort/feasibility.** L. Telegram permits third-party clients subject to its API
terms; that route is materially different from Discord self-bots. Nevertheless TDLib brings
network/session maintenance, native dependencies and caches. It can receive push without an
application polling loop, but the overall client has not been shown to meet our tiny-memory
and zero-maintenance budget. Technically feasible without vendor changes; reject as a default
badge provider unless measured and explicitly requested. [Library responsibilities][telegram-library].

### WhatsApp

**Useful feasible scope.** Shared Windows notification cards/badges and launch/focus the
official app. S, no additional WhatsApp login, E only if the generic notification gate passes;
otherwise U. No supported general local inbox/voice-control API for the personal Windows app
was found in the reviewed official developer material.

**Do not confuse Business with Desktop.** Meta's public developer offering is the WhatsApp
Business Platform, with business messaging APIs, webhooks and onboarding. It is useful for a
business-number dashboard, not evidence that an ordinary personal account's desktop unread
state is exposed. It adds business-account authentication, permissions and a public webhook
receiver; M–L and external cost for that different product. The detailed developer pages
returned login/429 responses during research, so their current granular onboarding/scopes were
not independently established here. [Official developer hub][whatsapp-business].

**ToS/resource risk.** High for unofficial linked-device clients or scraping private Web
protocols as a shortcut. WhatsApp explicitly warns that linking to unofficial versions
violates its terms and can lead to restrictions/bans. Browser-driven WhatsApp Web libraries
also fail our browser/runtime requirement. Do not install one merely to obtain a badge.
[Official unofficial-app policy][whatsapp-unofficial].

### Google Calendar

**Useful UX.** An agenda widget/flyout, next meeting title/time, and open/join links. The
strict initial design is U: fetch a bounded date window on open/manual refresh and retain a
dated cache. A cached next-event transition can use a **single known deadline**, rather than
polling the server or repainting a countdown every second.

**Mechanism/auth/approval.** Calendar REST API with external-browser OAuth; request the
narrowest suitable scope, such as `calendar.events.readonly` (or owned-events-only). Public
distribution using sensitive calendar scopes can require Google verification; personal/internal
testing has exceptions and consent settings. No cookie or browser-profile reading.
[Scopes][google-scopes], [verification requirements][google-verification].

**Push path.** `events.watch` creates expiring notification channels to an HTTPS callback
receiver. Notifications indicate change; use incremental synchronization to retrieve it.
Channels need renewal and are not a direct desktop WebSocket. Hosting a relay moves some
work off the desktop but still adds infrastructure, credentials, renewal and connection costs.
[Push][google-push], [incremental sync][google-sync].

**ToS/cost/effort/feasibility.** Low risk for official scoped APIs with their user-data policy;
M for U, L with a relay. Feasible for personal use without vendor engineering; public review
may be required. E between user actions in the U version; M for fully live cloud sync. An ICS
subscription that is repeatedly downloaded is polling, not a push workaround. Cached agenda
must not claim freshness after a meeting is changed elsewhere.

### Outlook Calendar

**Useful UX/mechanism.** The same agenda/next-meeting/open/join surface, through Microsoft Graph
with delegated calendar-read permission and normal user consent. Use U initially; no need for
Outlook itself to be running. Graph's calendar event subscriptions support work/school and
personal Microsoft accounts. Organization policy may impose admin consent restrictions.
[Outlook notifications and permission table][outlook-push].

**Live path.** Graph delivers event changes to a webhook endpoint. Subscriptions expire;
Outlook subscriptions without resource data have a maximum lifetime under seven days, while
rich notifications expire in under a day. A relay must renew them, validate deliveries and
reconcile missed changes. This is M, not a free local event feed.
[Subscription lifetimes][graph-lifetimes].

**Local alternative and its limits.** Classic Outlook exposes Object Model item-change
events, so a narrowly scoped out-of-process COM observer is possible when classic Outlook is
already running. Treat it as a separate optional compatibility adapter; never launch/retain
Outlook just to show the shell agenda. Do not assume it works in new Outlook: Microsoft says
COM/VSTO add-ins are unsupported there. An in-process .NET/VSTO bridge is outside our design.
[Classic event][outlook-item-event], [new Outlook extensibility][outlook-new].

**Auth/ToS/cost/effort/feasibility.** Low risk on authorized Graph/COM routes. M for bounded
on-demand Graph reads; L for a robust cloud relay or several Outlook generations. Feasible
without vendor engineering subject to account/tenant consent. Reuse cached event deadlines;
no periodic appointment scan or always-running Outlook helper.

### Claude Desktop

**General Chat/Cowork integration.** Claude can consume a user-installed local MCP server,
including native executables. A tiny mypaper MCP bridge could offer an explicit
“show result/attention card” tool, forwarded over a same-user pipe to our existing provider.
Local custom installation is documented; directory publication is a separate review path.
This makes Claude a client of our tools, **not a server exposing all its chats**.
[Desktop MCP/installation][claude-mcp].

**Code-session integration.** Claude Code has deterministic lifecycle hooks such as
`UserPromptSubmit`, `Stop` and `Notification`. Permission-prompt notifications are explicitly
intended for side effects like forwarding to an external service. Desktop Code sessions have
their own version/policy support; verify that workflow rather than assuming hooks cover
ordinary Chat or Cowork. A command hook can launch our native reporter only for a transition.
[Hooks][claude-hooks], [desktop Code behavior][claude-desktop].

**Data/actions/auth.** Show only reported session state, project label, done/attention and an
open-app action. User installs/configures the bridge/hooks and tool permissions; enterprise
policy can prohibit them. No independent Claude cloud key is needed for a local reporting
tool. A model choosing to call a tool is not a guaranteed session lifecycle event.

**ToS/cost/effort/feasibility.** Low risk on these extension points, M, feasible without vendor
changes for supported workflows. E for a native transition reporter; an MCP child has process
and session-lifetime overhead and must be measured. No Node/Python runtime is required by MCP
itself. Generic toast/open fallback remains useful. No consumer-chat global status API was
established; do not scrape SQLite or keep UI Automation active to manufacture one.

### ChatGPT Desktop

**Current supported surfaces.** Official documentation now describes shared MCP configuration
for the ChatGPT desktop app, Codex CLI and IDE extension, with local stdio and Streamable HTTP
servers. An optional native mypaper reporting tool is therefore possible; do not rely on old
claims that ChatGPT can only connect to remote MCP. It reports tool invocations, not all
consumer conversation state. [Official MCP setup][chatgpt-mcp].

**Local agent hooks.** Current hooks documentation covers `UserPromptSubmit`, `Stop`,
`Interrupt` and `PermissionRequest` for supported local Work/Codex workflows. Cloud-orchestrated
Work has different enterprise-only hook rules, even when some execution is local. Use only
the installed workflow's documented events; no promise of universal ChatGPT Chat monitoring.
Our reporter must make no allow/deny decision on a permission hook.
[Hook coverage and cloud limits][chatgpt-hooks].

**Data/actions/auth.** Bar/flyout for the opted-in agent sessions, a result/attention card,
open app or a user-copied conversation link. The user enables local hooks/MCP; admin policy
may restrict them. Do not copy ChatGPT session cookies or assume API keys expose consumer
conversations. For public directory distribution, OpenAI says local MCP submissions are
limited to approved partners; manual supported configuration and directory publication are
different feasibility questions. [Directory approval][chatgpt-local-publishing].

**ToS/cost/effort/feasibility.** Low risk through documented extension points; M for local
reporting, E/U depending on hooks versus explicit tools. A small native stdio bridge can sleep
but costs an additional process while ChatGPT retains it; prefer transition hooks when they
cover the need. No browser, no extra interpreter, no polling compliance/chat exports. Feasible
without vendor changes for supported local configuration; directory approval is not guaranteed.

### Docker Desktop

**Mechanism/data/actions.** Use the existing engine's streaming `GET /events` or, initially,
`docker events --format '{{json .}}'`. This is a live stream, not repeated `docker ps`.
Container start/stop/die/OOM/health events support a count and failure flyout. Seed with a
bounded container list, then reconcile events/snapshot without a race. Only 256 past events
are retained, so reconnect cannot assume arbitrary historical replay.
[Event semantics][docker-events], [Engine API][docker-api].

**UX.** Static container count and failure dot; a flyout of selected containers with explicit
start/stop/open actions. Desktop's CLI also documents start/stop/restart/status, but those
management operations are commands, not a background monitor.
[Desktop CLI][docker-desktop].

**Auth/ToS/cost.** Existing local engine/named-pipe access, no extra cloud OAuth for local
lifecycle data; normal daemon permissions still grant powerful authority. Use the selected
local context rather than assuming a fixed pipe or accidentally monitoring a remote engine.
Do not enable unauthenticated TCP. Low risk through official APIs. Never start Docker/WSL
just for a badge; the engine's own resource use must remain opt-in.
[Daemon access model][docker-access].

**Effort/feasibility.** E/M, feasible today. Prefer direct Rust Engine API streaming for the
final adapter; a resident Go CLI stream has additional memory even while sleeping. Filter
events server-side, cache only selected metadata, and resnapshot on a real gap. No `stats`
stream, log tail, image scans or health-check polling by mypaper.

### Tailscale

**Mechanism/data/actions.** Tailscale's own source exposes `WatchIPNBus`, an HTTP stream on
the LocalAPI's `/localapi/v0/watch-ipn-bus`; it decodes successive state notifications without
repeated status requests. The official Windows transport uses a named pipe and identifies the
calling Windows user for access control. Use a native client, initial state and only needed
notification fields. [LocalAPI/watch implementation][tailscale-local], [Windows pipe][tailscale-pipe].
The server handler streams notification callbacks directly; it does not add a heartbeat loop
at that HTTP boundary. This still needs idle-traffic qualification for the chosen backend
watch mask. [Server handler][tailscale-watch-server].

**UX.** A bar icon for connected/needs-login/stopped, a flyout with the local Tailscale address,
selected exit node and copy/open actions. One-shot CLI commands can provide a U initial
version; documented commands include status and preference changes. Never run `status --json`
every few seconds, and do not continuously probe peers just to display “online.”
[CLI][tailscale-cli].

**Auth/ToS.** Reuse the already-running service's local access checks; login remains with
Tailscale. User/admin policy governs preference changes. No cloud API key is needed for a
local read. Low risk through its own client surface, but **medium maintenance risk**:
`WatchIPNBus` is explicitly marked unstable, unlike some other LocalAPI methods.
[API maturity][tailscale-local].

**Cost/effort/feasibility.** E candidate/M, feasible without vendor changes. The adapter need
not duplicate the VPN or instantiate a userspace Tailscale node. Subscribe to bounded state,
not periodic engine/traffic telemetry, and verify idle deliveries under the chosen watch mask.
Tailscale's service networking remains its existing cost; do not count it as mypaper's adapter
or claim the combined VPN system has zero idle traffic.

## Recommended first batch: seven scoped features

Build order accounts for reuse and the user's requested apps, not just the arithmetic score.
These are minimal designs that preserve the resource rule; conditional live features must
fall back to U rather than add a polling workaround.

| Build | Feature | Surface | Minimal behavior and resource boundary |
|---:|---|---|---|
| 1 | Shared media / Spotify | Bar, optional media widget, flyout | Reuse one SMTC service; static metadata and three transport buttons. No timeline tick, marquee or separate Spotify process. |
| 2 | Launcher actions + Recent | Launcher and dock context flyout | Query automatic Recent/Frequent only when opened. Reuse existing game shortcuts; curated GitHub open actions and a user-configured Grok Bot routine POST. The HTTP worker exits/sleeps after the click; launching never prestarts a vendor app at idle. |
| 3 | Shared notifications | Dock/bar badges, notification center | One listener, AUMID grouping, retained-toast counts and dismiss/open-app. **Gate:** package/capability plus reliable events with another foreground app; otherwise snapshot-only on open. Covers Discord/Grok Bot and other chat apps without per-app clients. |
| 4 | T3 Code summaries | Bar and agent flyout, one-shot notifications | Read-only scoped client; summary snapshot/deltas, running/attention counts, at most a bounded recent-result list, open-thread links. **Gate:** native loopback transport without recurring idle pings; otherwise on-demand flyout. No transcript stream or extra T3/Node server. |
| 5 | OBS | REC/LIVE bar and control flyout | One authenticated local socket; state/scene/mute events only. Static labels, explicit controls, no meters, preview video, stats refresh or blinking. Disable when OBS/module is absent. |
| 6 | Docker | Developer bar badge and flyout | Existing engine only; initial bounded list plus filtered lifecycle stream. Selected-container failures, start/stop on click. No metrics/log stream and no engine/WSL startup for monitoring. |
| 7 | Tailscale | Network bar icon and flyout | Existing Windows service only; local named-pipe state subscription, own address/exit-node metadata and copy/open actions. Use the smallest watch mask, excluding engine/traffic telemetry; qualify idle deliveries. If unsupported or unavailable, one-shot CLI read on open, then exit. |

Keep Tailscale's unstable LocalAPI behind a small replaceable module. Home Assistant is a
useful subsequent U widget/flyout, or an on-demand substitute when Tailscale is not installed;
the inspected HA server's 55-second heartbeat excludes its persistent socket from this batch.
VS Code/Cursor/Claude/ChatGPT transition hooks can subsequently share the same native reporting
receiver instead of each creating an independent daemon.

### Acceptance evidence before enabling a persistent module

This is a short resource gate for the proposed work, not tests executed in this research:

- Compare disabled and enabled idle adapters under the native plan's measurement method;
  record CPU/wakeups, private memory, network frames and all helper processes. Reject recurring
  idle wakes. Count COM/accessibility or vendor-side work caused by the integration too.
- Confirm no idle paint, animation timer, browser process or hardware graphics device is
  introduced; opening a flyout performs bounded work and closing it releases transient state.
- Exercise app absent/exit/restart, access revoked, network loss and sleep/resume. The source
  must become unavailable/stale, release resources and await a real wake/retry signal.
- Verify state meaning once: retained toasts versus unread, human attention versus agent wait,
  failed versus successful completion, webhook acceptance versus finished work. Check snapshot
  races and duplicate transitions before turning them into user notifications.

## Avoid, and why

1. **Repeated REST/CLI reads**, including GitHub conditional requests, calendar/ICS refresh,
   Spotify playback reads, `docker ps/stats`, `tailscale status`, and “slow” five-minute loops.
   Sleeping between polls does not meet an event-only rule.
2. **Unexamined push SDKs.** T3's five-second pings, HA's 55-second server heartbeat and Slack's connection refresh illustrate
   that push data can still wake an idle client. An optional long-lived cloud relay is real
   infrastructure, not a cost-free way to claim native zero idle.
3. **Full secondary chat clients for a badge.** TDLib is legitimate but substantial; unofficial
   Discord/WhatsApp clients add unacceptable account, compatibility and resource risks here.
4. **Token/cookie/session extraction** from Discord, Grok Bot, ChatGPT, Claude or other apps;
   copied provider credentials and borrowed client IDs are not authorized integration methods.
5. **Injection or extra UI runtimes:** client mods, Explorer DLL hooks, remote-debugging bridges,
   embedded browsers, browser automation daemons, in-process .NET/VSTO shims, and always-running
   Node/Python wrappers. A native protocol client or supported transition hook is sufficient
   where the platform actually provides the feature.
6. **Taskbar pixel/OCR unread counts, private database parsing and global UIA scraping.** They
   provide unreliable semantics and can force work in both shell and target app. Windows
   notification counts should be labeled honestly rather than disguised as universal unread.
7. **Confusing MCP direction or scope.** Claude/Grok/ChatGPT consuming our tool does not expose
   their entire session state. T3's external MCP credentials also cannot authenticate its WS RPC.
8. **Treating the old Teams local API as confirmed.** Published vendor/partner evidence
   conflicts; the integration maintainer reports retirement. Do not build a dependency on it
   from an old sample or a still-live setup article.
9. **Continuous activity graphics:** speaking dots, audio meters, stream previews, animated
   progress, per-second agent elapsed time and all-entity HA sensors. Static states, bounded
   snapshots and real transitions carry most of the value.
10. **Prestarting expensive apps/services** or broad SDK subscriptions for dormant modules.
    No hidden Steam, Docker/WSL, HA, second agent host or parallel VPN node just for a shell icon.

## Primary sources

Repository links above describe the current local code. External sources were checked on the
research date. T3 citations are pinned to the inspected commit; other moving documentation
should be checked again during implementation. “No public interface found” is limited to the
official material reviewed, not a reverse-engineering claim.

[win-notifications]: https://learn.microsoft.com/en-us/windows/apps/develop/notifications/app-notifications/notification-listener
[win-notification-event]: https://learn.microsoft.com/en-us/uwp/api/windows.ui.notifications.management.usernotificationlistener.notificationchanged
[win-usernotification]: https://learn.microsoft.com/en-us/uwp/api/windows.ui.notifications.usernotification?view=winrt-26100
[win-notification-access]: https://learn.microsoft.com/en-us/uwp/api/windows.ui.notifications.management.usernotificationlistener.requestaccessasync?view=winrt-26100
[win-toast-history]: https://learn.microsoft.com/en-us/uwp/api/windows.ui.notifications.toastnotificationhistory.gethistory
[win-overlay]: https://learn.microsoft.com/en-us/windows/win32/api/shobjidl_core/nf-shobjidl_core-itaskbarlist3-setoverlayicon
[win-uia]: https://learn.microsoft.com/en-us/windows/win32/winauto/uiauto-eventsforclients
[win-event]: https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-setwineventhook
[win-service]: https://learn.microsoft.com/en-us/windows/win32/api/winsvc/nf-winsvc-notifyservicestatuschangew
[win-dir]: https://learn.microsoft.com/en-us/windows/win32/api/winbase/nf-winbase-readdirectorychangesw
[win-media-manager]: https://learn.microsoft.com/en-us/uwp/api/windows.media.control.globalsystemmediatransportcontrolssessionmanager?view=winrt-26100
[win-media-session]: https://learn.microsoft.com/en-us/uwp/api/windows.media.control.globalsystemmediatransportcontrolssession?view=winrt-26100
[win-recent]: https://learn.microsoft.com/en-us/windows/win32/api/shobjidl_core/nn-shobjidl_core-iapplicationdocumentlists
[win-recent-list]: https://learn.microsoft.com/en-us/windows/win32/api/shobjidl_core/nf-shobjidl_core-iapplicationdocumentlists-getlist
[win-taskbar]: https://learn.microsoft.com/en-us/windows/win32/shell/taskbar-extensions
[discord-rpc]: https://docs.discord.com/developers/topics/rpc
[discord-oauth]: https://docs.discord.com/developers/topics/oauth2
[discord-sdk-release]: https://discord.com/developers/docs/social-sdk/release_notes.html
[discord-sdk-client]: https://discord.com/developers/docs/social-sdk/classdiscordpp_1_1Client.html
[discord-selfbot]: https://support.discord.com/hc/en-us/articles/115002192352-Automated-User-Accounts-Self-Bots
[t3-repo]: https://github.com/pingdotgg/t3code
[t3-architecture]: https://github.com/pingdotgg/t3code/blob/a21d788752695764028e6d26ba924f3963e05806/docs/internals/overview.md
[t3-auth]: https://github.com/pingdotgg/t3code/blob/a21d788752695764028e6d26ba924f3963e05806/docs/internals/environment-auth.md
[t3-rpc]: https://github.com/pingdotgg/t3code/blob/a21d788752695764028e6d26ba924f3963e05806/packages/contracts/src/rpc.ts
[t3-schemas]: https://github.com/pingdotgg/t3code/blob/a21d788752695764028e6d26ba924f3963e05806/packages/contracts/src/orchestrationV2.ts
[t3-shell-stream]: https://github.com/pingdotgg/t3code/blob/a21d788752695764028e6d26ba924f3963e05806/apps/server/src/orchestration-v2/ShellStream.ts
[t3-ws]: https://github.com/pingdotgg/t3code/blob/a21d788752695764028e6d26ba924f3963e05806/apps/server/src/ws.ts
[t3-heartbeat]: https://github.com/pingdotgg/t3code/blob/a21d788752695764028e6d26ba924f3963e05806/packages/client-runtime/src/rpc/protocol.ts
[t3-session]: https://github.com/pingdotgg/t3code/blob/a21d788752695764028e6d26ba924f3963e05806/packages/client-runtime/src/rpc/session.ts
[t3-mcp]: https://github.com/pingdotgg/t3code/blob/a21d788752695764028e6d26ba924f3963e05806/docs/orchestration-v2/orchestrator-mcp-server.md
[t3-outside]: https://github.com/pingdotgg/t3code/blob/a21d788752695764028e6d26ba924f3963e05806/docs/user/outside-agents.md
[t3-links]: https://github.com/pingdotgg/t3code/blob/a21d788752695764028e6d26ba924f3963e05806/packages/shared/src/threadLinks.ts
[grok-product]: https://x.ai/bot
[grok-overview]: https://docs.x.ai/grok-bot/overview
[grok-faq]: https://docs.x.ai/grok-bot/faq
[grok-notifications]: https://docs.x.ai/grok-bot/settings-and-notifications
[grok-routines]: https://docs.x.ai/grok-bot/skills-routines-and-automations
[grok-api]: https://docs.x.ai/overview
[grok-connectors]: https://docs.x.ai/grok/connectors
[grok-inference]: https://docs.x.ai/grok-bot/inference-hooks
[grok-terms]: https://x.ai/legal/grok-bot-terms
[spotify-playback]: https://developer.spotify.com/documentation/web-api/reference/get-information-about-the-users-current-playback
[spotify-pkce]: https://developer.spotify.com/documentation/web-api/tutorials/code-pkce-flow
[spotify-quota]: https://developer.spotify.com/documentation/web-api/concepts/quota-modes
[spotify-july]: https://developer.spotify.com/blog/2026-07-23-web-api-quota-updates
[steam-apps]: https://partner.steamgames.com/doc/api/ISteamApps#GetLaunchCommandLine
[steam-api]: https://partner.steamgames.com/doc/api/steam_api#SteamAPI_RunCallbacks
[github-notifications]: https://docs.github.com/en/rest/activity/notifications
[github-delivery]: https://docs.github.com/en/webhooks/using-webhooks/handling-webhook-deliveries
[github-events]: https://docs.github.com/en/webhooks/webhook-events-and-payloads
[slack-socket]: https://docs.slack.dev/apis/events-api/using-socket-mode/
[slack-tokens]: https://docs.slack.dev/authentication/tokens/
[slack-conversations]: https://docs.slack.dev/reference/objects/conversation-object/
[teams-local-help]: https://support.microsoft.com/en-us/teams/calls-devices/connect-to-third-party-devices-in-microsoft-teams
[teams-retirement]: https://help.elgato.com/hc/en-us/articles/26249796361613-How-to-pair-Stream-Deck-plugin-with-Microsoft-Teams
[teams-presence]: https://learn.microsoft.com/en-us/graph/changenotifications-for-presence
[teams-storage]: https://learn.microsoft.com/en-us/troubleshoot/exchange/send-emails/ews-not-support-access-teams-data
[obs-setup]: https://obsproject.com/kb/remote-control-guide
[obs-protocol]: https://github.com/obsproject/obs-websocket/blob/master/docs/generated/protocol.md
[ha-websocket]: https://contributors.home-assistant.io/core/internals/websocket-api/
[ha-server]: https://github.com/home-assistant/core/blob/dev/homeassistant/components/websocket_api/http.py
[vscode-api]: https://code.visualstudio.com/api/references/vscode-api
[vscode-cli]: https://code.visualstudio.com/docs/configure/command-line
[vscode-hooks]: https://code.visualstudio.com/docs/agent-customization/hooks
[cursor-hooks]: https://cursor.com/docs/hooks
[telegram-links]: https://core.telegram.org/api/links
[telegram-tdlib]: https://core.telegram.org/tdlib/getting-started
[telegram-library]: https://core.telegram.org/tdlib/docs/
[telegram-app]: https://core.telegram.org/api/obtaining_api_id
[whatsapp-business]: https://whatsappbusiness.com/developers/developer-hub/
[whatsapp-unofficial]: https://faq.whatsapp.com/1217634902127718/?locale=en_US
[google-scopes]: https://developers.google.com/workspace/calendar/api/auth
[google-verification]: https://developers.google.com/identity/protocols/oauth2/production-readiness/sensitive-scope-verification
[google-push]: https://developers.google.com/workspace/calendar/api/guides/push
[google-sync]: https://developers.google.com/workspace/calendar/api/guides/sync
[outlook-push]: https://learn.microsoft.com/en-us/graph/outlook-change-notifications-overview
[graph-lifetimes]: https://learn.microsoft.com/en-us/graph/change-notifications-overview
[outlook-item-event]: https://learn.microsoft.com/en-us/office/vba/api/outlook.items.itemchange
[outlook-new]: https://learn.microsoft.com/en-us/office/dev/add-ins/outlook/one-outlook
[claude-mcp]: https://support.claude.com/en/articles/10949351-getting-started-with-local-mcp-servers-on-claude-desktop
[claude-hooks]: https://code.claude.com/docs/en/hooks
[claude-desktop]: https://code.claude.com/docs/en/desktop
[chatgpt-mcp]: https://learn.chatgpt.com/docs/extend/mcp
[chatgpt-hooks]: https://learn.chatgpt.com/docs/hooks
[chatgpt-local-publishing]: https://developers.openai.com/plugins/deploy/desktop-only
[docker-events]: https://docs.docker.com/reference/cli/docker/system/events/
[docker-api]: https://docs.docker.com/reference/api/engine/version/v1.51/
[docker-desktop]: https://docs.docker.com/desktop/features/desktop-cli/
[docker-access]: https://docs.docker.com/engine/security/protect-access/
[tailscale-local]: https://github.com/tailscale/tailscale/blob/main/client/local/local.go
[tailscale-pipe]: https://github.com/tailscale/tailscale/blob/main/safesocket/pipe_windows.go
[tailscale-watch-server]: https://github.com/tailscale/tailscale/blob/main/ipn/localapi/localapi.go
[tailscale-cli]: https://tailscale.com/docs/reference/tailscale-cli
