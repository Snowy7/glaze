# Discord, calendars and personal widgets

Implemented on `native`, 2026-11-05. Settings remain in
`%APPDATA%\glaze\integrations.json`; secrets remain in Windows Credential Manager.

## What connects without a developer account

- Add Calendar, then choose **Calendars → On this PC → Read**. The widget also tries Windows'
  AppointmentStore automatically while visible. This reads the system's shared appointment store,
  including synced accounts that actually populate that store. Package identity alone does not
  import the contents of Outlook, new Outlook, or a browser. On this PC the API succeeded and
  returned no events; Google and Microsoft were not connected.
- Add Discord, then **Enable recent messages**. This enables the existing Windows notification
  listener and Discord-only notification previews (`notifications.discordPreviews`). Windows may
  ask for consent. Stable, PTB, Canary and Store
  AUMIDs are recognized; the widget's `appId` option supports an exact additional identity.
  These are Windows-retained notifications, not all messages, history, or unread counts. Quiet,
  disabled or dismissed notifications cannot be reconstructed.
- **Studio → Widgets → New custom widget** creates a new JSON definition and installs four
  editable examples in `%APPDATA%\glaze\widgets`. Edit the new file, then open the widget
  catalog. Every valid JSON file is a separate Custom entry, with multiple instances allowed.
  Reopening either picker reloads the catalog. Refresh reloads a definition and its data.

## Google Calendar

1. Create/select a Google Cloud project, enable **Google Calendar API**, and configure the OAuth
   consent screen. For personal testing, add your Google account as a test user.
2. Create an OAuth client of type **Desktop app**. A web, Android or iOS client is not a substitute.
3. In Calendar, choose **Calendars → Google Calendar → Connect**. On first setup, put the desktop
   client ID in the Windows dialog's User name field and its optional client secret in Password.
   The ID is saved as `calendar.googleClientId`; the secret, if supplied, is saved as
   `glaze/integrations/calendar/google/client-secret` in Credential Manager.
4. Sign in and approve read-only calendar access in your browser. Subsequent Connect clicks use
   the registered ID directly. Distribution beyond personal testing may need Google's verification.

The implementation uses an ephemeral `127.0.0.1` loopback listener, PKCE S256, random state,
`calendar.readonly`, offline access and refresh tokens. No embedded browser or hard-coded secret.
Desktop client IDs may instead be supplied in integrations.json or at build time through
`GLAZE_GOOGLE_CLIENT_ID`. No developer registration was available for live Google sign-in here.

## Microsoft / Outlook Calendar

1. Register an application in **Microsoft Entra → App registrations**. To support personal Outlook
   accounts as well as work/school accounts, choose **Accounts in any organizational directory and
   personal Microsoft accounts**.
2. Add a **Mobile and desktop applications** redirect URI:
   `http://localhost:53683/oauth/callback`. Configure the app as a public client. Do not create or
   ship a client secret. Add delegated Microsoft Graph **Calendars.Read** permission; tenant policy
   may require an administrator's consent.
3. In Calendar, choose **Calendars → Microsoft / Outlook → Connect**. On first setup, put the
   Application (client) ID in User name and leave Password empty. Sign in in your browser.

The ID is `calendar.microsoftClientId` (or build-time `GLAZE_MICROSOFT_CLIENT_ID`). Sign-in uses
the common authority, PKCE S256, state, `offline_access` and Graph `Calendars.Read`.
No Microsoft registration was available for live sign-in here.

Both providers enumerate calendars and fetch event occurrences, including recurring instances.
Events are merged into the existing month, week and agenda views; dots and rows retain calendar
colours. All-day end dates are exclusive, timed events use the Windows local timezone, and multi-day
events appear on each covered day. Imported events cannot be changed through the local editor and
are not written into the user's local-event storage. Cloud calendars load on open/Refresh, never
on a polling timer. Windows changes invalidate its local snapshot. The current bounded window is
180 days back and 365 days ahead, at most 1000 events per provider and 4096 visible day occurrences.
When a refresh fails, the previous source's events remain available with its error in Calendars.
The × beside an account removes its local credentials; revocation is done in the provider's account UI.

## Discord current call

Discord's official RPC API requires a registered application. Personal development is supported
for accounts on the application's tester list (up to 50); public access and protected RPC scopes
require Discord approval. An installed Discord process or open IPC pipe does not grant access.

1. Register a Discord application, configure its RPC Origin, and add your account as an app tester.
2. In the Discord widget's settings, choose **Connect current call**. Enter the Application ID and
   application client secret in the Windows dialog. Approve Discord's own authorization prompt.
3. The Call tab displays the selected channel, participants, speaking, mute and deafen state.
   It subscribes to official voice/speaking events on the configured local pipe, default
   `\\.\pipe\discord-ipc-0`, while the widget is visible. No client modification or account-token
   extraction is used. If another Discord instance owns a different pipe, configure that exact pipe
   in `discord.pipe` and Refresh; there is no scanning or reconnect timer.

The read path requests `rpc` and `rpc.voice.read`. For optional controls, set
`discord.voiceControls` to `true` and reconnect to request `rpc.voice.write`. Mute/deafen buttons
remain disabled without that grant. Discord documents voice-setting overrides as app-specific and
reset when the controlling RPC connection closes; the widget labels this behaviour. Read-only is
the default. The adapter supports tester/approved apps; this PC had no app ID/secret/tester grant,
so actual voice delivery and controls could not be qualified. The IPC fixture verifies registered
handshake, authentication, event frames and cancellation-safe partial reads.

`rpc.notifications.read` is also protected. The ordinary recent-message widget uses Windows'
notification API instead, so no Discord RPC notification grant is required. Broad personal DM
history and sending as the signed-in Discord user are not exposed by this implementation: ordinary
bots do not inherit a user's DMs, and self-bot/user-token automation is not a permissible substitute.

## Discord server history and sending

1. Create a bot in your Discord application's Developer Portal. Enable **Message Content Intent**
   for reading message bodies; verified/large applications may need approval for that intent.
2. Choose **Add bot to a server**. The first click collects the Application ID and bot token in a
   Windows-owned masked dialog and opens Discord's OAuth installation page. Only View Channels,
   Send Messages and Read Message History are requested (permission bitset 68608).
3. Choose your server in Discord. Its channel permissions still apply. In integrations.json, set
   `discord.channelIds` to the server text-channel IDs you want (maximum eight). Use Discord's
   Developer Mode → Copy Channel ID, or the channel ID from its copied URL. Set `discord.enabled`
   to true if you configured everything manually, then Refresh the widget.

The Messages tab shows up to 30 messages per configured channel, bounded to 100 total. It refreshes
on open/Refresh; no Gateway heartbeat or polling connection is created. Channel tabs choose the
destination. Click a history row to reply, type in the widget's box, then Send/Enter. Sending uses
the **bot's** identity, never the signed-in user's. Automatic mentions and reply pings are disabled.
No messages were sent in verification. Bot tokens cannot be obtained through a desktop OAuth bot
installation; the application's owner must create and supply the bot token once. A provider-hosted
bot would need a separately operated backend, which this local implementation does not invent.

Credential targets are under `glaze/integrations/`: `discord/bot`, `discord/client-secret`,
`discord/rpc-access`, `discord/rpc-refresh`, `discord/rpc-expiry`. Calendar OAuth tokens use separate
Credential Manager parts so large Graph tokens fit Windows' 2560-byte per-credential limit.
Never put tokens, passwords or secrets in integrations.json. The private-file `credential` example
remains available for setup without the native dialogs.

## References

- [Discord RPC, IPC restrictions and voice override semantics](https://docs.discord.com/developers/topics/rpc)
- [Discord OAuth scopes and bot authorization](https://docs.discord.com/developers/topics/oauth2)
- [Discord Gateway intents](https://docs.discord.com/developers/events/gateway#message-content-intent)
- [Discord's self-bot policy](https://support.discord.com/hc/en-us/articles/115002192352-Automated-User-Accounts-Self-Bots)
- [Windows appointment-store capability](https://learn.microsoft.com/en-us/windows/apps/package-and-deploy/app-capability-declarations#general-use-capabilities)
- [Google desktop OAuth](https://developers.google.com/identity/protocols/oauth2/native-app)
- [Google Calendar event occurrences](https://developers.google.com/workspace/calendar/api/v3/reference/events/list)
- [Microsoft desktop registration](https://learn.microsoft.com/en-us/entra/identity-platform/scenario-desktop-app-registration)
- [Graph calendarView](https://learn.microsoft.com/en-us/graph/api/calendar-list-calendarview)

Evidence: `evidence/personal-*.json`; native size sheets are in
`crates/shell/src/widgets/views/integrations/golden-personal/`.
