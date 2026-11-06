# Native rebuild: feature parity checklist

Audit date: 5 November 2026. Old baseline: `main` at `f378a4c`. Native baseline: checked-out `native`, HEAD `61a3da44ae41d107f35a633aa9a1f6f734e23810`, plus the files present while this audit was read. Other agents are changing N3, bars and the engine concurrently; this is a source snapshot, not a release verdict.

The original audit was read-only. Widget follow-up incorporates native through `9cc02b1`; source parity and live evidence are recorded in [docs/widgets.md](../../docs/widgets.md#widget-parity-follow-up-validation). Performance gates remain separate from behavior status. Bars/flyouts follow-up on 5 November updates the owned source rows and records production measurements in docs/bars.md. **Done means an implemented, reachable source path was found; it does not mean live-tested.** Partial means the old user outcome or one of its controls is incomplete. N3 takes precedence for its assigned flyouts, launcher, notification center and menu presentation, even where a provider or tint-only menu already exists. Bar and engine work is mentioned in notes rather than relabeled N3.

Each row is one checklist item; related settings with the same status are enumerated together. Counts apply to rows, not individual JSON properties. Studio rows measure both the editor and whether its controls still affect the desktop. The final backlog references row IDs and excludes N3 and deliberate cuts.

**Status counts:** **276 done**; **18 dropped on purpose**; **5 partial**. **299 rows total.**

Old locations below are paths at `f378a4c`, read with `git show f378a4c:<path>` and inventoried with `git ls-tree -r f378a4c`. Abbreviations in that column: `B/` = `bars/shell/src/`; `RB/` = `crates/bars/src/`; `RW/` = `crates/widgets/src/`; `W/` = `widgets/`; `S/` = `apps/studio/src/`; `ST/` = `apps/studio/src-tauri/src/`. Native evidence links point to current files, whose contents may change after this audit.

Approved intentional cuts are narrowly scoped: [plan lines 350–352][losses] remove theme CSS, gradient/image/noise/glow fills, neon, ambient widget motion, two launcher layouts, on-widget settings, HTML widgets, the DWM host backdrop and stand-ins. [Plan line 222][style-plan] also explicitly lists bevel corners. This does **not** waive data, interaction, accessibility, opaque-surface or settings parity. The retained launcher layouts are Win11 and Compact; Grid and Dashboard map to those respectively. Screenshot exclusion is only a **conditional** loss, [plan lines 347–349][capture-plan], not an unconditional deletion.

## Top bar

| ID / feature and settings | Status | Old location | Native evidence and one-line note |
|---|---|---|---|
| T01 Enable, select monitors (`all`, `primary`, devices), top/bottom edge, height | **done** | `RB/config.rs`, `B/top/top.ts` | [Bar host][bar-host], [bar window][bar-window]: creates and reserves each selected display's bar. |
| T02 Strip, floating and islands; `margin`, `padding`, `spacing`, `width` full/auto/DIPs/%; `\|` separators | **done** | `RB/config.rs`, `B/top/build.ts` | [Bar view][bar-view]: three zones and independently sized island groups retain geometry. |
| T03 `left`, `center`, `right` module order, deduplication and custom modules | **done** | `B/top/modules.ts`, `B/top/build.ts` | [Bar view][bar-view]: orders and deduplicates modules per bar. |
| T04 App-bar work-area reservation, display/DPI changes and Explorer restart | **done** | `RB/appbar.rs`, `RB/app.rs` | [Bar host][bar-host], [appbar][appbar]: reservations are re-established and layout reconciled. |
| T05 Independent top/dock designs and preset pairings; macos, fluent, capsule, minimal, islands, floating, monochrome | **done** | `RB/presets.rs`, `B/designs/`, `B/top/build.ts` | [Presets][presets], [bar view][bar-view]: independent pairings retain their metrics, glyph sets, Fluent search/grouping, macOS app weight and the surface-free minimal design; broader N6 style controls remain under C06–C15/C25. |
| T06 Neon design / neon-glass pairing | **dropped on purpose** | `crates/bars/presets/neon-glass.json`, `B/designs/` | [Plan lines 350–351][losses]: neon/glow/gradient decoration is deliberately removed. |
| T07 Start, Search and quick-settings/power entry buttons | **done** | `B/top/modules.ts`, `B/actions.ts` | [Bar host][bar-host]: buttons reach typed actions; native panel contents are tracked under N3 below. |
| T08 Virtual desktops: count/current indicators and click-to-switch | **done** | `RB/desktops.rs`, `B/top/modules.ts` | [Bar services][bar-services], [bar host][bar-host]: registry-backed desktops and stepwise switching remain. |
| T09 Foreground application name and real icon; ignore bar activation | **done** | `RB/apps.rs`, `B/top/modules.ts` | [Bar services][bar-services]: retains the last external foreground window and resolves its app identity. |
| T10 Clock seconds, full token format, quoted literals, two-line formats; locale clock on taskbar | **done** | `B/clock-format.ts`, `RB/clock.rs` | [Clock formatter][clock-format], [bar view][bar-view]: NLS formatting and minute/second scheduling are implemented. |
| T11 Media title/artist, play/pause click and previous/next wheel | **done** | `RB/media.rs`, `B/top/modules.ts` | [Bar host][bar-host], [bar services][bar-services]: SMTC subscriptions and wheel actions are wired. |
| T12 Network offline/ethernet/Wi-Fi state, signal-strength glyph and SSID tooltip | **done** | `RB/network.rs`, `B/top/modules.ts` | [Bar view][bar-view]: signal levels, offline/ethernet glyphs and SSID/strength hover descriptions. |
| T13 Volume low/mid/high/mute glyph, numeric level tooltip, wheel adjustment/unmute and middle-click mute | **done** | `RB/audio.rs`, `B/top/modules.ts` | [Bar view][bar-view], [host][bar-host]: low/mid/high/mute glyphs, level tooltip and retained wheel/middle-click controls. |
| T14 Battery percentage/charging, hide when absent, power-settings click | **done** | `RB/system.rs`, `B/top/modules.ts` | [Bar view][bar-view], [host][bar-host]: battery events, presence and click destination remain. |
| T15 CPU/RAM stats, two-second sampling and Task Manager click | **done** | `RB/system.rs`, `B/top/modules.ts` | [Bar services][bar-services], [view][bar-view]: samples only when subscribed. |
| T16 Language abbreviation/region and foreground layout tracking | **done** | `RB/flyouts/language.rs`, `B/top/modules.ts` | [Bar services][bar-services]: follows foreground and language events; selector is N3. |
| T17 Notification count/bell, visible-only 15-second recount | **done** | `RB/flyouts/notifications.rs`, `B/top/modules.ts` | [Bar services][bar-services]: notification-count lane feeds the visible bell; full center is N3. |
| T18 Tray icons, promoted order, left/right/middle/double click and Explorer-host mode | **done** | `RB/tray.rs`, `RB/tray_host.rs`, `B/top/modules.ts` | [Bar services][bar-services]: configured tray hosting survives hidden bars/fullscreen; promoted order and all callbacks remain. |
| T19 Per-module left/middle/right/wheelUp/wheelDown actions, default/none, exec+args, URI/path open | **done** | `B/actions.ts`, `RB/bridge.rs`, `RB/config.rs` | [Bar host][bar-host]: dispatches host-owned config bindings with wheel throttling. |
| T20 Explicit `native-start`, `native-notifications`, `native-quick-settings` bypass actions | **done** | `B/actions.ts`, `RB/bridge.rs` | [Bar host][bar-host]: explicit native actions bypass custom panels and invoke Windows. |
| T21 Module `style.color/background/border/padding/radius/size/weight` (solid paints) | **done** | `B/style/modules.ts`, `RB/config.rs` | [Bar view][bar-view]: module-local solid fill, edge and typography are consumed. |
| T22 Module icon override: pack key or absolute/theme-relative image | **done** | `B/style/modules.ts`, `RB/assets.rs` | [Bar icons][bar-icons], [view][bar-view]: pack/file overrides and SVG alpha masks use the module text color. |
| T23 Custom command, args, interval, timeout and first-line/JSON text | **done** | `RB/custom.rs`, `B/top/modules.ts` | [Custom provider][custom], [bar services][bar-services]: worker command execution and visibility subscription remain. |
| T24 Custom JSON `icon`, `tooltip`, `class`, failure dimming/error tooltip | **done** | `RB/custom.rs`, `B/top/modules.ts` | [Bar view][bar-view]: JSON icons resolve through pack keys, tooltips are visible, and failures dim with their error; CSS classes remain an intentional cut. |
| T25 Hide on fullscreen owning display; suspend periodic work elsewhere; lock/display-off behavior | **done** | `RB/app.rs`, `docs/bars.md` | [Bar host][bar-host], [bar services][bar-services]: visible-topic union and periodic-topic filtering implement these conditions. |
| T26 Keyboard Tab/Enter/Space activation and focus indication on bar buttons | **done** | `B/dom.ts`, `B/top/modules.ts` (HTML buttons) | [Bar host][bar-host]: Tab/arrows traverse, Enter/Space invoke and Escape returns focus; visible focus rings and assistive focus stay island-local. |
| T27 Hover names/tooltips and custom/error descriptions | **done** | `B/dom.ts`, `B/top/modules.ts` | [Bar host][bar-host]: delayed visible module/app/custom/error tooltips on a pooled native surface. |

## Dock

| ID / feature and settings | Status | Old location | Native evidence and one-line note |
|---|---|---|---|
| D01 Enable, display selection, floating/taskbar styles, all four edges | **done** | `RB/config.rs`, `B/dock/styles.ts` | [Bar host][bar-host], [view][bar-view]: both layouts and edge placement are implemented. |
| D02 Icon size, margin/padding/spacing, app alignment, task button width, horizontal titles | **done** | `RB/config.rs`, `B/dock/styles.ts` | [Bar view][bar-view]: explicit dock.button wins over the titled-button default; existing geometry and alignment remain. |
| D03 Pins before stable-start-order running apps; AUMID/exe/shortcut grouping and real app names/icons | **done** | `RB/apps.rs`, `RB/icons.rs`, `B/dock/model.ts` | [Bar services][bar-services], [apps][apps]: retained grouping, ordering and asynchronous icon descriptions. |
| D04 `running` all/false/monitor; `split` one item per window | **done** | `RB/apps.rs`, `B/dock/model.ts` | [Bar services][bar-services]: per-monitor filtering and split-window items are implemented. |
| D05 Import Windows taskbar pins when omitted; explicit empty pins remain empty | **done** | `RB/config.rs`, `RB/app.rs` | [Bar services][bar-services]: `.lnk` enumeration and Explorer fallback preserve the old rule. |
| D06 Launch/activate/restore/minimize on left click; middle-click new instance | **done** | `RB/system.rs`, `B/dock/dock.ts` | [Bar host][bar-host]: user-click activation, restore and minimize paths remain. |
| D07 Context menu: windows, new instance, Pin/Unpin, close individual/all windows | **done** | `B/dock/dock.ts`, `B/menu.ts` | [Bar host][bar-host]: individual/all closes, launch and pin commands use the shared glass menu. |
| D08 Persist Pin/Unpin and drag reorder; drag running app into pins; drag pin off to remove | **done** | `B/dock/drag.ts`, `RB/bridge.rs` | [Bar host][bar-host]: drag capture and atomic pin edits implement all four actions. |
| D09 Magnify enabled/scale/range, cosine neighbor falloff, spring movement; disabled on taskbar/reduced motion | **done** | `B/dock/magnify.ts` | [Bar window][bar-window], [motion][bar-motion]: native scale/shift springs follow the pointer. |
| D10 Auto-hide edge sensor, one-second leave delay, no reservation, keep open during menu/preview/drag | **done** | `RB/bar.rs`, `B/dock/dock.ts` | [Bar window][bar-window], [host][bar-host]: edge sensor and hide deadlines preserve these behaviors. |
| D11 Window-count badges (`counter`) | **done** | `B/dock/dock.ts` | [Bar view][bar-view]: badges show for multiple windows. |
| D12 Running indicator dot/bar/pill/none and running/active/attention colors | **done** | `B/dock/dock.ts`, `B/style/apply.ts` | [Bar view][bar-view]: dot, bar, pill and none retain distinct geometry and state colors. |
| D13 Glow indicator | **dropped on purpose** | `B/dock/dock.ts`, `B/designs/` | [Plan line 350][losses]: glow decoration is an intentional loss. |
| D14 Attention flash state and pulse/bounce/none; six pulses or three bounces then persistent attention | **done** | `RB/app.rs`, `B/dock/dock.ts` | [Bar window][bar-window]: six opacity pulses or three bounces, then persistent attention; none suppresses animation. |
| D15 Launch bounce stops at first window or four seconds | **done** | `B/dock/dock.ts` | [Bar window][bar-window], [host][bar-host]: bounded launch animation remains. |
| D16 Hover preview enabled/delay, immediate item switch, 280 ms leave grace, live thumbnail/click/close | **done** | `B/dock/preview.ts`, `RB/preview.rs` | [Native previews][dock-preview], [host][bar-host]: live DWM thumbnails and actions are wired. |
| D17 Preview layout on narrow screens / many windows / vertical docks | **done** | `B/dock/preview.ts`, `B/flyout.ts` | [Native previews][dock-preview]: monitor-fitted paged grids at each DPI; vertical docks place previews beside the dock. |
| D18 Show-desktop extra and taskbar end sliver | **done** | `B/dock/dock.ts`, `B/dock/styles.ts` | [Bar view][bar-view], [host][bar-host]: the taskbar has its narrow end sliver; both desktop affordances invoke Win+D. |
| D19 Recycle Bin open/empty with Windows confirmation and live full state | **done** | `RB/trash.rs`, `B/dock/dock.ts` | [Bar host][bar-host], [trash][trash]: live empty/full glyphs, opening and Windows-confirmed emptying. |
| D20 No foreground theft except deliberate clicks; click-through outside dock shape | **done** | `RB/bar.rs`, `B/region.ts` | [Bar host][bar-host], [surface][surface]: activation guards and alpha-shaped surfaces retain this behavior. |
| D21 Hover app name label and keyboard activation of dock items | **done** | `B/dock/dock.ts` | [Bar host][bar-host]: delayed app-name tooltips and keyboard traversal/activation with a visible focus ring. |

## Flyouts and menus

| ID / feature and settings | Status | Old location | Native evidence and one-line note |
|---|---|---|---|
| F01 Module-anchor placement, screen clamping, toggle-close, outside click/Escape and focus handback | **done** | `B/flyout.ts`, `B/flyouts/surface.ts`, `RB/flyouts/glue.rs` | [Flyouts][flyouts]: anchored/clamped placement, one-panel toggle, Escape/deactivation close and foreground handback. |
| F02 Flyout/menu family mono/fluent/macos/material; glass/tint controls and open animations | **done** | `B/flyouts/ui.ts`, `B/style/apply.ts`, `RB/style.rs` | [Flyout look][flyout-look], [native menu][ui-menu]: family typography/radii/grouping, shared material/tint and finite alpha/position animation. |
| F03 Provider lanes, live watches, async replies and stop while closed | **done** | `RB/flyouts/service.rs`, `B/flyouts/protocol.ts` | [Flyouts][flyouts]: typed lane replies/live watches; lanes release five seconds after close; notification state uses the shared integration lease. |
| F04 Quick Settings Wi-Fi/Bluetooth on/off and Wi-Fi drill-in | **done** | `B/flyouts/quick.ts`, `RB/flyouts/quick.rs` | [Quick panel][quick-panel]: real Wi-Fi/Bluetooth tiles and Wi-Fi drill-in. |
| F05 Quick Settings power-mode cycle, battery-saver/DND/Focus state and Settings links | **done** | `B/flyouts/quick.ts`, `RB/flyouts/quick.rs` | [Quick panel][quick-panel]: power-mode cycle and battery-saver/DND/Focus state with Settings destinations. |
| F06 Built-in WMI / external DDC brightness, master volume/mute, mixer chevron, battery and Settings | **done** | `B/flyouts/quick.ts`, `RB/flyouts/quick.rs` | [Quick panel][quick-panel]: WMI/DDC brightness, endpoint volume/mute, mixer, battery and Settings. |
| F07 Wi-Fi radio, scan-on-open, SSID deduplication, signal/security and live scan completion | **done** | `B/flyouts/network.ts`, `RB/flyouts/wifi.rs` | [Wi-Fi panel][wifi-panel]: radio, scan-on-open, deduplicated SSIDs, signal/security and scan-change replies. |
| F08 Saved Wi-Fi connect; open/Personal password profile; disconnect; Enterprise/WEP Settings | **done** | `B/flyouts/network.ts`, `RB/flyouts/wifi.rs` | [Wi-Fi panel][wifi-panel]: saved connect/disconnect and Windows password prompt; Enterprise/WEP go to Settings. |
| F09 Wi-Fi location-permission explanation, location Settings; ethernet and VPN links | **done** | `B/flyouts/network.ts` | [Wi-Fi panel][wifi-panel]: location denial/no-adapter states, location Settings and ethernet/VPN destinations. |
| F10 Mixer master volume/mute, output list and optional `audio.switchOutput` | **done** | `B/flyouts/mixer.ts`, `RB/flyouts/mixer.rs` | [Mixer panel][mixer-panel]: master/mute, output list and gated default-device switching. |
| F11 Mixer per-app/system-sounds sliders/mutes, names/icons, live sessions and default-device changes | **done** | `B/flyouts/mixer.ts`, `RB/flyouts/mixer.rs` | [Mixer panel][mixer-panel]: session names/icons/sliders/mutes and device/session events; dragging preserves its captured row. |
| F12 Installed keyboard layouts/IME labels, selected state and change foreground app's language | **done** | `B/flyouts/modules.ts`, `RB/flyouts/language.rs` | [Language panel][language-panel]: installed/IME labels, checked foreground layout and external-app switching. |
| F13 Calendar today/time, prev/next/wheel/Today, localized month and `calendar.firstDay` | **done** | `B/flyouts/calendar.ts` | [Calendar panel][calendar-panel]: NLS month/weekdays, firstDay, paging/wheel/Today and minute-aligned local time. |
| F14 Calendar `zones` (up to four), labels/time/day/offset, Outlook Calendar link | **done** | `B/flyouts/calendar.ts`, `RB/flyouts/config.rs` | [Calendar panel][calendar-panel]: four labeled IANA clocks with DST, day and fractional offset; Outlook Calendar link. |
| F15 Power lock/sleep/sign-out/restart/shutdown, confirm restart/shutdown | **done** | `B/top/modules.ts`, `B/menu.ts`, `RB/system.rs` | [Power panel][power-panel]: real system actions; sign-out/restart/shutdown confirmation. |
| F16 Tray overflow icons, labels and all click types | **done** | `B/flyouts/modules.ts`, `RB/tray_host.rs` | [Tray panel][tray-panel]: real overflow icons/labels, left/right/middle/double callbacks and keyboard invocation. |
| F17 Desktop menu enabled/items/order/hide/custom label+command+args and separator cleanup | **done** | `crates/desktop/src/desktop_menu.rs`, `crates/config/src/desktop_menu.rs` | [Desktop items][desktop-items], [desktop menu][desktop-menu]: config-driven typed menu is ported. |
| F18 Bare-desktop interception; icon/Shift/native fallback; Show more options | **done** | `crates/desktop/src/desktop_hit.rs`, `crates/desktop/src/desktop_menu.rs` | [Desktop hook][desktop-hook], [desktop menu][desktop-menu]: hit checks, replay and fallback remain. |
| F19 Desktop View: icons visible and large/medium/small; Sort name/size/type/date and direction; Refresh | **done** | `crates/desktop/src/desktop_menu.rs`, `crates/desktop/src/desktop_view.rs` | [Desktop items][desktop-items], [desktop view][desktop-view]: Explorer COM actions and checked state remain. |
| F20 Desktop New folder/text + rename, file Paste, Terminal-in-Desktop and custom command execution | **done** | `crates/desktop/src/desktop_menu.rs`, `crates/desktop/src/desktop_view.rs` | [Desktop items][desktop-items]: native shell delegates file semantics to Explorer and retains command arguments. |
| F21 Desktop Studio/wallpaper, widget-edit, Display/Personalize shortcuts | **done** | `crates/desktop/src/desktop_menu.rs` | [Desktop items][desktop-items]: all built-in destinations are present. |
| F22 Desktop menu arrows/Enter/Escape, submenus, outside-close and disabling during fullscreen/lock/display-off | **done** | `crates/desktop/src/menu_model.rs`, `crates/desktop/src/desktop_menu.rs` | [Native menu][ui-menu], [desktop menu][desktop-menu]: keyboard model and hook lifecycle are ported. |
| F23 Desktop/dock menu live glass, matching material and animated presentation | **done** | `crates/desktop/src/menu_ui.rs`, `B/menu.ts` | [Native menu][ui-menu], [flyout look][flyout-look]: desktop/dock/tray menus share glass, tokens and finite motion; long menus scroll. |
| F24 DWM host backdrop implementation | **dropped on purpose** | `RB/blur.rs` | [Plan line 352][losses]: engine plates/overlays replace DWM host backdrop. |
| F25 Menus visible in screenshots/screen sharing | **done** | `RB/capture.rs`, `RB/glass.rs` | [Surface][surface], [overlay capture][overlay-capture]: content remains capture-visible; N3 verified the glass/content in GDI screenshots. |

## Notification center

| ID / feature and settings | Status | Old location | Native evidence and one-line note |
|---|---|---|---|
| N01 Clock/bell/action/Win+N entry points on correct display and toggle-close | **done** | `B/flyouts/index.ts`, `RB/flyouts/winkey.rs` | [Flyouts][flyouts]: clock/bell/action/Win+N open the native center on the selected display and toggle it closed. |
| N02 Real Windows toasts, permission request, loading/denied/empty states and privacy Settings | **done** | `B/flyouts/notifications.ts`, `RB/flyouts/notifications.rs` | [Notification center][notification-center]: shared integration lease, explicit UI-thread permission request, loading/disabled/denied/empty states and Settings. |
| N03 Group by AUMID/package identity, newest-first stacks and expand/collapse | **done** | `B/flyouts/notification-center.ts` | [Notification center][notification-center]: identity-grouped newest-first stacks and expand/collapse controls. |
| N04 App logo resolution, title/body and time; monogram only on a real miss | **done** | `B/flyouts/notification-center.ts`, `RB/flyouts/notifications.rs` | [Notification center][notification-center]: app logos resolve on the STA icon lane; title/body/age and monogram on a real miss. |
| N05 Individual hover-X and swipe dismiss, clear group and Clear all with per-toast fallback | **done** | `B/flyouts/notification-center.ts`, `RB/flyouts/notifications.rs` | [Notification center][notification-center]: card/group/all dismiss and swipe; shared provider attempts every removal even if a card has disappeared. |
| N06 DND header/state and Settings link | **done** | `B/flyouts/notifications.ts` | [Notification center][notification-center]: DND state in the header and notification Settings link. |
| N07 Calendar below notifications and thin themed scrolling | **done** | `B/flyouts/notifications.ts`, `B/flyouts/calendar.ts` | [Notification center][notification-center]: localized month below the independently scrolling card list, with a themed scrollbar and PgUp/PgDn. |
| N08 Four-second refresh while open, stop on close, preserve settled content | **done** | `B/flyouts/notifications.ts`, `RB/flyouts/service.rs` | [Notification center][notification-center]: four-second refresh requests only while open; semantic snapshot/age comparison prevents unchanged redraws. |

## Launcher (Start flyout)

This is separate from the desktop Launcher widget below. The plan drops exactly two of four Start layouts, without identifying which two.

| ID / feature and settings | Status | Old location | Native evidence and one-line note |
|---|---|---|---|
| L01 AppsFolder + user/common Start shortcuts, names/icons/exe keywords; filter uninstallers/readmes/URLs | **done** | `RB/flyouts/index.rs`, `B/flyouts/launcher/launcher.ts` | [Launcher][launcher-panel], [app index][app-index]: native AppsFolder/Start index, real names/icons/keywords and retained filtering. |
| L02 Cached index, delayed background build and Start-folder/install refresh | **done** | `RB/flyouts/index.rs`, `RB/flyouts/service.rs` | [Flyouts][flyouts], [launcher][launcher-panel]: cached index, eight-second initial build, debounced Start-folder watch and ten-minute stale check. |
| L03 Fuzzy ranking: exact/prefix/word/initials/substring/subsequence, separate apps/settings/files results | **done** | `B/flyouts/launcher/fuzzy.ts`, `B/flyouts/launcher/model.ts` | [Launcher][launcher-panel]: exact/prefix/word/initials/substring/subsequence ranking with Apps/Settings/Files headings. |
| L04 About 70 Settings pages, keyword search and `launcher.files` recent-file search | **done** | `B/flyouts/launcher/settings-pages.ts`, `RB/flyouts/index.rs` | [Launcher][launcher-panel]: retained Settings-page vocabulary and optional recent-file search. |
| L05 Usage count/recency ranking, recent/recommended apps/files | **done** | `B/flyouts/launcher/model.ts`, `RB/flyouts/index.rs` | [Launcher][launcher-panel]: usage count plus recency ranking, recent/recommended apps/files. |
| L06 Win11/grid/compact/dashboard layouts: the two retained layouts | **done** | `B/flyouts/launcher/launcher.ts`, `RB/flyouts/config.rs` | [Launcher][launcher-panel], [config][flyout-config]: Win11 and Compact retained; legacy Grid maps to Win11 and Dashboard to Compact. |
| L07 Two additional Start layouts | **dropped on purpose** | `B/flyouts/launcher/launcher.ts` | [Plan lines 350–351][losses]: Win11 and Compact are retained; Grid maps to Win11 and Dashboard maps to Compact. Dashboard-only time/media/stats go with that approved layout cut. |
| L08 Search typing, 2D pin-grid arrows, result arrows, Enter/Escape, IME and focus handback | **done** | `B/flyouts/launcher/launcher.ts`, `RB/bar.rs` | [Launcher][launcher-panel], [text field][text-field]: native RichEdit/IME, 2D pins and result/clipboard arrows, Enter/Escape and focus handback. |
| L09 `launcher.pinned`, dock-pin inheritance; app menu pin/unpin launcher / pin dock | **done** | `B/flyouts/launcher/launcher.ts`, `RB/bridge.rs` | [Launcher][launcher-panel]: explicit pins, omitted-pin dock inheritance and native pin/unpin/pin-dock menu. |
| L10 All apps A–Z, user/footer Settings and confirmed power actions | **done** | `B/flyouts/launcher/launcher.ts` | [Launcher][launcher-panel]: A–Z all apps, user/footer Settings and confirmed power panel. |
| L11 Windows clipboard history, select-to-copy and history-disabled Settings link | **done** | `B/flyouts/launcher/launcher.ts`, `RB/flyouts/clipboard.rs` | [Launcher][launcher-panel]: clipboard history in both retained layouts, copy selection and disabled-history Settings link. |
| L12 Dashboard time/media/stats (if dashboard retained) | **dropped on purpose** | `B/flyouts/launcher/launcher.ts`, `B/flyouts/launcher/model.ts` | [Plan lines 350–351][losses]: Win11 and Compact are retained; Grid maps to Win11 and Dashboard maps to Compact. Dashboard-only time/media/stats go with that approved layout cut. |
| L13 `launcher.winKey`: lone Win, Win+N/Win+A interception only with taskbar hidden | **done** | `RB/flyouts/winkey.rs` | [Flyouts][flyouts], [Win-key provider][winkey]: hooks only while configured with the taskbar hidden. |
| L14 Preserve other Win combinations; fullscreen/hung/no-bar fallback, stale requests and secure-desktop reset | **done** | `RB/flyouts/winkey.rs`, `RB/app.rs` | [Flyouts][flyouts], [Win-key provider][winkey]: other shortcuts pass through, fullscreen/no-bar/hung fallback and stale/secure-desktop reset remain. |

## Widgets: common host, arranging and appearance

All 17 old manifests are represented by compiled [native catalog schemas][widget-catalog]; IDs, settings defaults and replacement aliases remain. Each widget subsection below covers **all** of its manifest settings. The old `size.default/min/max`, `resizable`, `preset`, `instances`, permissions, storage and replacement rules are covered here rather than repeated 17 times.

Widget “done” rows describe implemented behavior. Final production CPU/RAM qualification and the README glass capture are pending the user's deferred desktop window; [widget handoff](../../docs/widgets.md#implementation-handoff-2026-11-05) records measured failures and historical results.

| ID / feature and settings | Status | Old location | Native evidence and one-line note |
|---|---|---|---|
| H01 All 17 widget IDs, defaults, settings validation and old replacement aliases | **done** | `W/*/widget.json`, `RW/manifest.rs`, `RW/registry.rs` | [Registry][widget-registry], [layout][widget-layout], [catalog][widget-catalog]: compiled manifests preserve legacy identifiers. |
| H02 Existing instance IDs/settings, per-instance text/tasks/events/timer/cache and shared activity storage | **done** | `RW/layout.rs`, `RW/storage.rs`, `W/*/main.js` | [Widget model][widget-model], [storage][widget-storage]: reads old keys/JSON shapes without reseeding existing layouts. |
| H03 Monitor/cell/size, icon-grid default, fixed cell/gap/margin, work area, missing-monitor fallback | **done** | `RW/layout.rs`, `RW/display.rs` | [Layout][widget-layout], [widget host][widget-host]: grid, size clamping and display reconciliation remain. |
| H04 Edit toggle from tray/Studio/message and Ctrl+Alt+Shift+W; Done/Escape | **done** | `RW/main.rs`, `W/shell/src/edit.ts` | [Widget control][widget-control], [host][widget-host], [edit chrome][widget-edit]: legacy messages/hotkey and native Done are wired. |
| H05 Add palette, duplicate with storage/picked files, remove and instance limits | **done** | `W/shell/src/edit.ts`, `RW/app.rs` | [Widget host][widget-host]: duplicate rebases settings and storage URLs after checked private-file copy; successful removal cleans storage and picked files. Failed saves retain live state and files. |
| H06 Pointer drag, snapping, min/max resize and save-on-drop | **done** | `W/shell/src/grid.ts`, `W/shell/src/edit.ts` | [Edit model][widget-edit], [host][widget-host]: eight edge/corner handles, opposite-edge anchoring, snapping, manifest min/max and transactional save-on-drop. |
| H07 Overlap resolution by swapping/shifting neighbors; reject impossible layout with feedback | **done** | `W/shell/src/grid.ts` | [Edit model][widget-edit]: swaps equal neighbors, shifts a collision chain into free cells, and refuses impossible layouts atomically with a visible notice. |
| H08 Arrows move; Shift+arrows resize selected widget | **done** | `W/shell/src/edit.ts` | [Widget host][widget-host]: arrows move the selected widget and Shift+arrows resize, through the same checked placement/save path as pointer arranging. |
| H09 Settings and Appearance edited directly on a widget / edit toolbar | **dropped on purpose** | `W/shell/src/settings-form.ts`, `W/shell/src/appearance-form.ts` | [Plan line 352][losses]: edit settings in Studio instead. |
| H10 Topmost overlay preset and `overlay` toggle, including media-pill; below-app desktop preset | **done** | `RW/overlay.rs`, `W/media-pill/widget.json` | [Widget host][widget-host]: overlay presets (including media-pill) use activating topmost surfaces and the retained toggle; arranging moves them into the desktop band and desktop presets remain below applications. |
| H11 Visible-only subscriptions/timers, covered-monitor hiding, pause/battery/lock/display-off | **done** | `RW/app.rs`, `RW/media.rs`, `RW/audio.rs`, `RW/stats.rs` | [Widget host][widget-host]: releases hidden buffers and gates data services and worker fetches. |
| H12 Ignore edit-mode requests while held; leave edit mode on entering hold | **done** | `RW/app.rs`, `docs/widgets.md:457` | [Widget host][widget-host]: held hosts ignore edit requests and cancel arranging when a hold starts. |
| H13 Built-in/theme/global/instance appearance layers; explicit resets and palette accent | **done** | `RW/look.rs`, `W/shell/src/appearance.ts` | [Widget look][widget-look], [host][widget-host]: retained layer/reset merging now resolves auto accent from the active wallpaper palette. |
| H14 Surface none/glass/solid, tint and opacity | **done** | `W/*/base.css`, `RW/look.rs` | [Widget view][widget-view]: none/glass/tint are retained and solid forces opaque fill. |
| H15 Radius, border toggle, text light/dark, accent, font and content scale | **done** | `RW/look.rs`, `W/*/base.css` | [Widget view][widget-view], [host][widget-host]: these non-CSS appearance controls have native consumers. |
| H16 Text shadow setting | **done** | `W/*/base.css`, `RW/look.rs` | [Widget view][widget-view]: optional text shadows cover plain and emphasized text, including clipped content. |
| H17 Non-neon minimal/depth/paper/aero/liquid design styles | **done** | `W/*/base.css`, `RW/look.rs` | [Widget view][widget-view]: minimal/depth/paper/aero/liquid select distinct native treatments; approved CSS/noise/glow losses remain removed. N6 global style synchronization remains C25. |
| H18 Gradient/noise/glow/neon and ambient motion; theme CSS / HTML SDK / custom HTML widget loading | **dropped on purpose** | `W/*/base.css`, `W/shell/`, `packages/widget-sdk/`, `RW/net.rs` | [Plan lines 350–352][losses], [lines 66–67][html-plan]: compiled native built-ins replace HTML/CSS. |
| H19 Live wallpaper glass; appearance blur/opacity plus theme glass intensity/refraction/highlight/tint/blur | **done** | `W/shell/src/plates.ts`, `RW/look.rs`, `crates/theme/src/lib.rs` | [Widget host][widget-host]: publishes Source::Widgets plates with 12 px default blur, appearance opacity/blur and theme tint/intensity/refraction/highlight. Worktree rain-city frost was inspected live; the final README capture is pending. |
| H20 Live edits of active theme appearance/materials | **done** | `RW/theme.rs`, `RW/app.rs` | [Widget host][widget-host]: watches active-theme theme.json and rebinds its watcher when the configured theme changes; bar watcher status remains owned by the bar slice. |
| H21 Invalid layout: retain last good display, refuse writes and show edit-mode error | **done** | `RW/app.rs`, `W/shell/src/edit.ts` | [Layout][widget-layout], [host][widget-host]: parse failures retain the last good display (empty on invalid startup), preserve immutable recovery backups, refuse every write at the file boundary and surface the edit error. |
| H22 Scroll and keyboard access to long content in notes/todo/RSS/calendar/launcher | **done** | `W/*/style.css`, `W/*/main.js` | [Widget view][widget-view], [host][widget-host]: clipped scrolling and hits, wheel/arrows/Page/Home/End, Tab traversal and Enter/Space invoke reach long lists and Markdown previews. |
| H23 JSON-RPC plugin manifests, request/subscription protocol and Studio setting-search calls | **done** | `RW/plugins.rs`, `RW/rpc.rs`, `docs/writing-plugins.md` | [Plugin library][widget-plugins], [widget documentation](../../docs/widgets.md): JSON-RPC request/subscription and Studio setting-search remain supported; native widget provider integrations are explicit Rust consumers, with no arbitrary plugin-generated UI. The supported boundary is documented. |
| H24 File/image pickers: private copied files, 64 MB cap, filters, duplicate URL rebasing | **done** | `RW/pick.rs`, `W/shell/src/settings-form.ts` | [Picker][widget-picker], [host][widget-host]: filters and 64 MB cap remain; duplicates recursively rebase only source-owned URLs and use checked private-file copies. |
| H25 Old `mypaper-widgets --edit/--overlay` executable invocation | **dropped on purpose** | `RW/main.rs` | [Plan line 63][html-plan] deletes the executable; [widget control][widget-control] preserves the edit/overlay outcomes through the old hotkeys and registered messages. |
| H26 Per-widget web permissions/CSP/SDK mock harness | **dropped on purpose** | `RW/net.rs`, `packages/widget-sdk/`, `docs/writing-widgets.md` | [Plan lines 66–67][html-plan], [line 352][losses]: HTML execution and authoring contract is intentionally retired. |

### Clock

| ID / feature and settings | Status | Old location | Native evidence and one-line note |
|---|---|---|---|
| WC01 `variant`: desk digital, flip, analog, words greeting, world | **done** | `W/clock/widget.json`, `W/clock/main.js` | [Widget view][widget-view]: desk, finite value-change flip, analog with numerals/date, greeting with time/date, and world dials are implemented. |
| WC02 `format24`, `showSeconds`, `showDate` | **done** | `W/clock/main.js`, `W/clock/clock-format.js` | [Clock view][widget-view], [widget time][widget-time]: clock formats/date controls retain each face's information; greeting/world tick at minute precision and analog/digital/flip support seconds. |
| WC03 `zones` list (label/tz, up to eight), real time-zone conversion | **done** | `W/clock/widget.json`, `W/clock/zones.js` | [Widget time][widget-time], [view][widget-view]: stored IANA zones are passed to Windows Globalization. |
| WC04 World-city dial, day/night, relative day/UTC offset, invalid-zone explanation and adaptive layout | **done** | `W/clock/zones.js`, `W/clock/main.js` | [Widget view][widget-view]: adaptive world rows include dials, day/night, relative date and UTC offset; invalid zones explain the failure and long city lists scroll. |
| WC05 `name` override / Windows display-name greeting; `font` override | **done** | `W/clock/main.js`, `RW/userinfo.rs` | [Widget view][widget-view], `crates/widgets/src/userinfo.rs`: explicit/default name and custom font are consumed. |
| WC06 Locale-sensitive date/weekday/time-period words | **done** | `W/clock/clock-format.js`, `W/clock/greeting.js` | [Widget time][widget-time]: Windows locale formats weekday/date/day-period text; greetings follow the user language with an English fallback. |
| WC07 Second/minute-aligned ticking, no updates hidden; bounded flip transition | **done** | `W/clock/main.js` | [Widget model][widget-model], [host][widget-host]: second/minute aligned, hidden clocks sleep, flip transitions finish after 280 ms and start only on displayed-value changes. |

### Calendar

| ID / feature and settings | Status | Old location | Native evidence and one-line note |
|---|---|---|---|
| WCA01 `variant` month/agenda/strip and compact responsive layout | **done** | `W/calendar/widget.json`, `W/calendar/main.js` | [Widget view][widget-view]: all three native layouts are present, with compact strip fallback. |
| WCA02 `weekStart` auto/monday/sunday/saturday and `weekNumbers` | **done** | `W/calendar/calendar-logic.js`, `W/calendar/main.js` | [Widget view][widget-view]: month and week strip honor auto/monday/sunday/saturday; month retains week numbers. |
| WCA03 Previous/next month or week, select day, title-click return to today | **done** | `W/calendar/main.js` | [Widget host][widget-host]: previous/next and day selection remain; title invokes Today and resets month/week offset. |
| WCA04 Local events persistence, month event dots, upcoming agenda and delete | **done** | `W/calendar/main.js`, `W/calendar/calendar-logic.js` | [Widget model][widget-model], [view][widget-view], [host][widget-host]: reads old `events`, shows markers/agenda and saves deletions. |
| WCA05 Add event title, chosen date, optional time, color swatch, validation and cancel | **done** | `W/calendar/main.js:276`, `W/calendar/calendar-logic.js` | [Widget host][widget-host], [view][widget-view]: content editor supplies title/date/optional time, six color swatches, validation feedback and Save/Enter/Cancel/Escape; stored wire shape is unchanged. |
| WCA06 Selected day's complete event list and color indicators; agenda scrolling | **done** | `W/calendar/main.js` | [Widget view][widget-view]: selected-day events and agenda display all entries through clipped scrolling, with stored color indicators. |

### Now playing

| ID / feature and settings | Status | Old location | Native evidence and one-line note |
|---|---|---|---|
| WM01 Shared SMTC title/artist/artwork, play/pause/previous/next and live progress | **done** | `W/now-playing/main.js`, `RW/media.rs` | [Widget view][widget-view], [host][widget-host]: current media, artwork and transport use native services. |
| WM02 `variant` card/blur/strip/viz | **done** | `W/now-playing/widget.json`, `W/now-playing/main.js` | [Widget view][widget-view]: card/strip/viz remain and blur uses a bounded downsampled artwork backdrop. |
| WM03 `tint` colors from artwork; album/source-app/elapsed/duration labels | **done** | `W/now-playing/main.js`, `W/now-playing/media.js` | [Widget view][widget-view]: opt-in artwork-derived tint, album/source app and elapsed/duration metadata are rendered. |
| WM04 `viz` bars/mirror/wave and `sensitivity` | **done** | `W/now-playing/spectrum.js`, `W/now-playing/viz.js` | [Widget view][widget-view], [host][widget-host]: opt-in WASAPI renders bars/mirror/wave with sensitivity; no ambient audio polling when unused. |
| WM05 `glow` artwork halo | **dropped on purpose** | `W/now-playing/main.js`, `W/now-playing/style.css` | [Plan line 350][losses]: glow is explicitly cut. |
| WM06 Empty/no-art/live states and only-visible spectrum capture | **done** | `W/now-playing/main.js`, `RW/audio.rs` | [Widget view][widget-view], [host][widget-host]: empty/artwork fallback and visibility gating remain. |

### Media pill

| ID / feature and settings | Status | Old location | Native evidence and one-line note |
|---|---|---|---|
| WMP01 `variant` pill/notch/mini | **done** | `W/media-pill/widget.json`, `W/media-pill/style.css` | [Widget view][widget-view]: pill/notch/mini select actual compact layouts and plate shapes. |
| WMP02 `progress`, artwork/title/artist and previous/play/next | **done** | `W/media-pill/main.js`, `W/media-pill/pill.js` | [Widget view][widget-view], [host][widget-host]: shares the real native media controls and progress line. |
| WMP03 Single-instance overlay preset, always on top and overlay toggle | **done** | `W/media-pill/widget.json`, `RW/overlay.rs` | [Registry][widget-registry], [host][widget-host]: single-instance rule retained; topmost overlay surface and toggle are implemented (H10). |
| WMP04 Overflow title marquee | **dropped on purpose** | `W/media-pill/main.js`, `W/media-pill/style.css` | [Plan line 350][losses]: continuously scrolling widget motion is removed. |
| WMP05 Track/artist/elapsed/duration hover description | **done** | `W/media-pill/main.js` | [Widget view][widget-view], [host][widget-host]: hover describes full track/artist and elapsed/duration. |

### Launcher (desktop widget)

| ID / feature and settings | Status | Old location | Native evidence and one-line note |
|---|---|---|---|
| WL01 `variant` dock/grid/list; `shortcuts` label+target (up to 24) | **done** | `W/launcher/widget.json`, `W/launcher/apps.js` | [Widget view][widget-view], [host][widget-host]: all three layouts launch exe/AUMID/folder/link targets. |
| WL02 `labels`, `iconSize`, real Windows app icons | **done** | `W/launcher/main.js`, `RW/appicon.rs` | [Widget view][widget-view], [host][widget-host]: icon retrieval, size and name controls remain. |
| WL03 `search`, `engine` DuckDuckGo/Google/Bing/Brave/Ecosia/Startpage/custom and `customUrl` | **done** | `W/launcher/main.js`, `W/launcher/url.js` | [Widget host][widget-host]: native search entry builds encoded engine URLs and recognizes direct web addresses. |
| WL04 Dock magnification and launch bounce/error feedback | **done** | `W/launcher/main.js`, `W/launcher/style.css` | [Widget view][widget-view], [host][widget-host]: hover magnification, finite launch bounce and visible launch-failure feedback. |
| WL05 Invalid-custom-URL feedback, Escape clear then blur, long app-grid/list scrolling | **done** | `W/launcher/url.js`, `W/launcher/main.js` | [Widget host][widget-host]: invalid custom URL reports an error; Escape clears then blurs; grids/lists scroll with keyboard access. |

### Quick toggles

| ID / feature and settings | Status | Old location | Native evidence and one-line note |
|---|---|---|---|
| WQT01 `variant` grid/row/list, adaptive tile placement | **done** | `W/quick-toggles/widget.json`, `W/quick-toggles/main.js` | [Widget view][widget-view]: all three native tile arrangements exist. |
| WQT02 Tile visibility: `wifi`, `bluetooth`, `focus`, `airplane`, `nightlight`, `sound`, `display`, `battery` | **done** | `W/quick-toggles/widget.json` | [Widget view][widget-view]: every old tile-enable setting is read. |
| WQT03 Click each tile to open its matching Windows Settings page | **done** | `W/quick-toggles/toggles.js`, `W/quick-toggles/main.js` | [Widget host][widget-host]: retains the old link behavior; the separate quick-settings flyout supplies real toggles in N3. |
| WQT04 Live Wi-Fi on/signal/SSID and battery percent/charging labels and glyphs | **done** | `W/quick-toggles/toggles.js`, `W/quick-toggles/main.js:98` | [Widget view][widget-view]: live Wi-Fi connection/SSID/signal and battery percent/charging drive labels and glyphs. |
| WQT05 Launch failure/pressed feedback | **done** | `W/quick-toggles/main.js:48`, `:76` | [Widget fetcher][widget-data], [host][widget-host]: activation results return to the view; finite pressed state and visible failure notice. |

### Weather

| ID / feature and settings | Status | Old location | Native evidence and one-line note |
|---|---|---|---|
| WW01 `city` auto location / place object, Studio town search; `units` metric/imperial | **done** | `W/weather/widget.json`, `W/weather/main.js` | [Widget fetcher][widget-data], `apps/studio/src/components/inspectors/WidgetSettingControl.tsx`: linked weather service and retained place inspector support both. |
| WW02 `variant` card/compact/forecast, current temperature/place/condition | **done** | `W/weather/main.js`, `W/weather/weather-logic.js` | [Widget view][widget-view]: current/card/compact/forecast include day/night condition illustrations and responsive content. |
| WW03 Hourly forecast temperatures/chance/conditions; daily icons/chance/range bars and responsive seven-day view | **done** | `W/weather/main.js:109`, `W/weather/weather-logic.js` | [Widget view][widget-view]: hourly temperature/chance/condition rows and seven daily icons/chance/range bars scroll instead of truncating. |
| WW04 Location unavailable/denied guidance, privacy Settings action, retry/stale states and ten-minute cache | **done** | `W/weather/main.js`, `plugins/weather/src/service.rs` | [Widget fetcher][widget-data], [view][widget-view]: retained ten-minute service cache, location guidance, privacy Settings action, retry and cached/offline feedback. |
| WW05 `animate` and animated rain/cloud/snow illustration | **dropped on purpose** | `W/weather/main.js`, `W/weather/style.css` | [Plan line 350][losses]: ambient weather motion is intentionally removed. |
| WW06 Click-place inline town picker / automatic-location switch | **dropped on purpose** | `W/weather/main.js:283` | [Plan line 352][losses]: on-widget settings move to Studio; `city` itself remains required. |

### System monitor

| ID / feature and settings | Status | Old location | Native evidence and one-line note |
|---|---|---|---|
| WS01 `cpu`, `ram`, `gpu`, `net`, `disk`, `battery`; `drives` filter | **done** | `W/system-monitor/widget.json`, `W/system-monitor/meters.js` | [Widget view][widget-view], [stats service][widget-stats]: per-metric selection and drive filtering remain. |
| WS02 `variant` rings/bars/spark | **done** | `W/system-monitor/gauge.js`, `W/system-monitor/main.js` | [Widget view][widget-view], [metric model](../../crates/shell/src/widgets/metrics.rs): all displayed metrics have stable trace IDs, including network/disk/battery; hiding earlier rows cannot misassign history. |
| WS03 `interval` 1000/2000/5000 ms | **done** | `W/system-monitor/main.js`, `W/system-monitor/widget.json` | [Widget host][widget-host]: interval 1000/2000/5000 ms selects the shared sampler deadline; default remains the plan's two seconds. Hidden/held consumers stop sampling. |
| WS04 `vary` per-metric color, charging and metric-specific values | **done** | `W/system-monitor/main.js`, `W/system-monitor/meters.js` | [Widget view][widget-view]: color choice, units and battery presence are consumed. |
| WS05 Responsive readable layout, no-data/unsupported metric states | **done** | `W/system-monitor/main.js`, `W/system-monitor/meters.js` | [Widget view][widget-view]: readable row minimums with overflow scrolling, complete labels, no-selection/first-sample/unsupported states. |

### Timer

| ID / feature and settings | Status | Old location | Native evidence and one-line note |
|---|---|---|---|
| WT01 `focusMinutes`, `shortMinutes`, `longMinutes`, `customMinutes`, `sessionsBeforeLong`, `autoAdvance` | **done** | `W/timer/widget.json`, `W/timer/main.js` | [Widget model][widget-model], [host][widget-host]: durations, break selection and near-live automatic advance are implemented. |
| WT02 Focus/short/long/custom selection, start/pause/reset/skip, ring and remaining time | **done** | `W/timer/main.js` | [Widget view][widget-view], [host][widget-host]: all mode and transport hit actions exist. |
| WT03 Wall-clock persistence while closed/hidden; discovered completion waits rather than auto-running | **done** | `W/timer/main.js` | [Widget model][widget-model], [host][widget-host]: uses old `endsAt`/`finishedAt` and the 2.5-second auto-advance guard. |
| WT04 Session-cycle reset after long break, activity credited to actual finish day | **done** | `W/timer/main.js:116`, `W/timer/activity.js` | [Widget host][widget-host], [model][widget-model]: leaving a long break resets the session cycle; delayed focus completion credits the stored finish instant's local date. |
| WT05 Bounded completion glow and its click-to-acknowledge state | **dropped on purpose** | `W/timer/main.js`, `W/timer/style.css` | [Plan line 350][losses]: glow decoration is explicitly cut; native completion text remains in [widget view][widget-view]. |

### Countdown

| ID / feature and settings | Status | Old location | Native evidence and one-line note |
|---|---|---|---|
| WCD01 `title`, `date` (empty = next New Year), `time`, `afterwards` countUp/stop | **done** | `W/countdown/widget.json`, `W/countdown/count.js` | [Widget model][widget-model], [view][widget-view]: local target, elapsed days and done/today states are implemented. |
| WCD02 `style` digits/ring, `startDate` ring origin and `showSeconds` | **done** | `W/countdown/main.js`, `W/countdown/count.js` | [Widget model][widget-model], [view][widget-view]: configurable progress denominator and minute/second boundary updates remain. |
| WCD03 Largest nonzero unit, secondary units/date/progress, narrow/tiny responsive layouts | **done** | `W/countdown/main.js` | [Widget view][widget-view]: adaptive leading unit, secondary units, date and progress fit narrow/tiny layouts. |

### Photo

| ID / feature and settings | Status | Old location | Native evidence and one-line note |
|---|---|---|---|
| WP01 `variant` bleed/matte/polaroid, `fit` cover/contain, `caption` | **done** | `W/photo/widget.json`, `W/photo/main.js` | [Widget view][widget-view]: frame, fit and polaroid caption all have native consumers. |
| WP02 `image1`–`image6`, private picked-file URLs and duplicate-file ownership | **done** | `W/photo/widget.json`, `RW/pick.rs` | [Layout][widget-layout], [picker][widget-picker], [host][widget-host]: duplicated image1–image6 URLs own the copied instance folder and survive source removal; regression-tested. |
| WP03 `interval`, `shuffle` and soft crossfade slideshow | **done** | `W/photo/frame.js`, `W/photo/main.js` | [Widget host][widget-host], [view][widget-view]: deadlines, nonrepeat next-image choice and bounded fades are implemented. |
| WP04 `motion` Ken Burns pan/zoom | **dropped on purpose** | `W/photo/main.js`, `W/photo/frame.js` | [Plan line 350][losses]: ambient photo motion is explicitly out. |
| WP05 Skip broken pictures and continue/recover slideshow | **done** | `W/photo/main.js` | [Widget host][widget-host]: bad decode/ownership failures retain a good current image and schedule bounded advance/retry, including a single broken source. |
| WP06 Built-in demo landscapes/captions when no user image is chosen | **dropped on purpose** | `W/photo/scenes.js` (three generated placeholder landscapes) | [Plan line 352][losses]: classified as the explicitly removed stand-ins; user-photo playback/recovery remains required at WP01–WP05. |

### Notes

| ID / feature and settings | Status | Old location | Native evidence and one-line note |
|---|---|---|---|
| WN01 Click/keyboard edit, multiline text, clipboard/undo/IME, Escape/blur finish | **done** | `W/notes/main.js` | [Widget host][widget-host], [RichEdit field][text-field]: native editor and persistence are wired. |
| WN02 `fontScale`, autosave after 400 ms and flush when hidden/arranging/exiting | **done** | `W/notes/widget.json`, `W/notes/main.js` | [Widget host][widget-host], [view][widget-view]: reads old text and flushes the same storage key. |
| WN03 Markdown headings/bullets/tasks/quotes/rules and inline bold/italic/code/strike | **done** | `W/notes/md.js`, `W/notes/main.js` | [Widget view][widget-view], [Markdown parser](../../crates/shell/src/widgets/markdown.rs): headings/bullets/tasks/quotes/rules and bold/italic/code/strike render as native text operations. |
| WN04 Click task checkboxes, including `* [ ]` and indented forms; scroll long notes | **done** | `W/notes/md.js`, `W/notes/main.js` | [Widget host][widget-host], [view][widget-view]: task offsets handle both bullet kinds and indentation; long previews scroll with clipped checkbox hits. |

### To do

| ID / feature and settings | Status | Old location | Native evidence and one-line note |
|---|---|---|---|
| WTD01 `title` (empty hides), `addPosition` top/bottom, `hideDone` | **done** | `W/todo/widget.json`, `W/todo/main.js` | [Widget view][widget-view], [host][widget-host]: all settings are consumed. |
| WTD02 Add/edit/check/delete, count remaining, persistent text/done and 300-task limit | **done** | `W/todo/main.js` | [Widget model][widget-model], [host][widget-host]: controls and old storage shape are retained. |
| WTD03 Enter/F2 edit, Enter commit, Escape cancel, Delete task, focusout commit | **done** | `W/todo/main.js:278` | [Widget host][widget-host]: Tab/Enter/Space traversal/invoke, Enter/F2 edit, Enter or focusout commit, Escape cancel and Delete removal. |
| WTD04 Completion progress bar, compact auto-hide-done, scrolling and reveal just-added task | **done** | `W/todo/main.js`, `W/todo/style.css` | [Widget view][widget-view], [host][widget-host]: progress, compact hide-done policy, scrolling and reveal of newly added tasks. |
| WTD05 Shared heatmap activity and reversing a session's completion | **done** | `W/todo/activity.js`, `W/todo/main.js:236` | [Widget host][widget-host], [model][widget-model]: insertion/deletion shifts completion-credit indices; undo reverses the correct session/day count. |

### RSS

| ID / feature and settings | Status | Old location | Native evidence and one-line note |
|---|---|---|---|
| WR01 `feeds` (up to eight URLs), `maxItems` 3–30, `refreshMinutes` 15–120 | **done** | `W/rss/widget.json`, `W/rss/main.js`, `plugins/rss/` | [Widget fetcher][widget-data], [host][widget-host]: fetches/parses feeds on a sleeping worker and applies limits. |
| WR02 `variant` list/cards, `showSource`, `showTime`, click safe HTTP(S) article | **done** | `W/rss/main.js` | [Widget view][widget-view], [host][widget-host]: both layouts and link validation are wired. |
| WR03 Merge newest first, global duplicate removal, cached/offline content, per-feed failure information | **done** | `W/rss/main.js`, `plugins/rss/src/` | [Widget fetcher][widget-data], [view][widget-view]: newest-first global dedup, cached offline content and per-feed failure feedback. |
| WR04 Relative age now/minutes/hours/days/date and scroll all selected headlines | **done** | `W/rss/time.js`, `W/rss/main.js` | [Widget view][widget-view], [model][widget-model]: now/minute/hour/day/date ages update at their displayed boundary; every selected headline is scrollable. |

### Crypto

| ID / feature and settings | Status | Old location | Native evidence and one-line note |
|---|---|---|---|
| WCR01 `coins` CoinGecko IDs (up to six), `currency` usd/eur/gbp/jpy, `refreshMinutes` 2–30 | **done** | `W/crypto/widget.json`, `W/crypto/main.js` | [Widget fetcher][widget-data], [host][widget-host]: CoinGecko requests and refresh settings are wired. |
| WCR02 `showChange`, signed 24h percent and positive/negative color | **done** | `W/crypto/main.js`, `W/crypto/logic.js` | [Widget view][widget-view]: visibility and signed change colors remain. |
| WCR03 Currency-aware price precision, narrow layouts, updated timestamp, offline/cached indicators | **done** | `W/crypto/main.js`, `W/crypto/logic.js` | [Widget view][widget-view], [model][widget-model]: currency/value precision (including tiny values), narrow scrollable rows, updated timestamp and cached/offline footer. |

### Activity heatmap

| ID / feature and settings | Status | Old location | Native evidence and one-line note |
|---|---|---|---|
| WH01 `variant` weeks/month/streak; `source` all/todo/timer; shared historical counts | **done** | `W/heatmap/widget.json`, `W/heatmap/heat.js` | [Widget view][widget-view], [host][widget-host]: source aggregation and all three layouts exist. |
| WH02 `weekStart` monday/sunday and `legend` | **done** | `W/heatmap/main.js` | [Widget view][widget-view]: weekday placement and legend toggle are consumed. |
| WH03 `labels` month/weekday labels; per-day hover count/date tooltip | **done** | `W/heatmap/main.js`, `W/heatmap/widget.json` | [Widget view][widget-view], [host][widget-host]: optional month/weekday labels and hover date/count description. |
| WH04 Streak/total summaries, percentile intensity and live updates from task/focus completion | **done** | `W/heatmap/heat.js`, `W/heatmap/main.js` | [Widget view][widget-view], [host][widget-host]: intensity buckets, streak and completion refresh are implemented. |

### Quote

| ID / feature and settings | Status | Old location | Native evidence and one-line note |
|---|---|---|---|
| WQ01 `mode` daily/hourly/click, `source` builtin/custom/both, `custom` text+author (up to 50) | **done** | `W/quote/widget.json`, `W/quote/pick.js` | [Widget view][widget-view], [model][widget-model]: built-in/custom selection and boundary scheduling remain. |
| WQ02 Click/assistive invoke for another quote, text+author and empty-custom state | **done** | `W/quote/main.js`, `W/quote/quotes.js` | [Widget host][widget-host], [view][widget-view]: click advances the offset; empty custom sources show an explicit empty state. |
| WQ03 Fit complete quote/author at small sizes and normal Enter/Space activation | **done** | `W/quote/main.js` | [Widget view][widget-view], [host][widget-host]: measured word-wrap fit includes quote/author; Tab and Enter/Space reach the next-quote action. |

## Wallpaper

| ID / feature and settings | Status | Old location | Native evidence and one-line note |
|---|---|---|---|
| P01 Shader `source`, `fps`, `scale`, hot reload and error handling | **done** | `crates/theme/src/lib.rs`, `crates/wallpaper/src/sources/shader.rs` | [Theme schema][theme-schema], [shader source][shader-source]: retained with image-input support added. |
| P02 Image `source`, fit cover/contain/stretch/center, EXIF orientation and bounded decoding | **done** | `crates/wallpaper/src/sources/image.rs`, `crates/wallpaper/src/sources/wic.rs`, `docs/engine-notes.md:87` | [Image source][image-source], [WIC decoder][wic]: unchanged source and EXIF path remain. |
| P03 Slideshow `folder`, `interval`, `transition`, `shuffle`, `fit`; directory refresh and EXIF | **done** | `crates/theme/src/lib.rs`, `crates/wallpaper/src/sources/slideshow.rs` | [Slideshow source][slideshow-source], [theme schema][theme-schema]: slideshow controls and shared decoder remain. |
| P04 Video `source`, `fit`, `fps`, `speed`, finite looping and hardware/CPU decode | **done** | `crates/wallpaper/src/sources/video.rs`, `crates/wallpaper/src/sources/video_decode.rs` | [Video source][video-source], [video decoder][video-decode]: playback and loop paths remain outside the shell replacement. |
| P05 8/10-bit NV12/P010, HDR10/PQ/HLG to SDR tone mapping, SDR/BT.2020 handling | **done** | `crates/wallpaper/src/sources/video.wgsl`, `docs/engine-notes.md:106` | [Video shader][video-shader], [video decoder][video-decode]: old supported formats remain; no new claim of native HDR display output. |
| P06 Scene GLB/glTF `source`, `fps`, `scale`, `orbit`, `parallax`, `background` | **done** | `crates/theme/src/lib.rs`, `crates/wallpaper/src/sources/scene.rs` | [Scene source][scene-source], [theme schema][theme-schema]: renderer and scene settings remain. |
| P07 Web local/remote `url`, local virtual-origin assets, one environment/multiple monitors | **done** | `crates/wallpaper/src/sources/web.rs`, `docs/web-and-app-wallpapers.md` | [Web source][web-source]: WebView2 wallpaper hosting is retained by explicit decision D-W. |
| P08 Web visibility suspension/resume and theme hot reload; browser UI disabled | **done** | `crates/wallpaper/src/sources/web.rs` | [Web source][web-source], [engine][engine]: source-owned visibility and suspension remain. |
| P09 App `command`, `args`, `{hwnd}`, first-monitor embedding/adoption, no activation, job lifetime | **done** | `crates/wallpaper/src/sources/app.rs`, `docs/web-and-app-wallpapers.md` | [App source][app-source]: same monitor-0/process-lifetime contract remains. |
| P10 App suspension/working-set trim while hidden, resume and delayed adoption | **done** | `crates/wallpaper/src/sources/app.rs` | [App source][app-source]: retained app source is not replaced by a shader-only engine. |
| P11 Web/scene `interactive`: bare-desktop move/buttons/wheel, scene drag orbit, coalescing and hold | **done** | `crates/desktop/src/raw_input.rs`, `crates/wallpaper/src/sources/web.rs`, `docs/desktop-extras.md:223` | [Raw input][raw-input], [web source][web-source], [scene source][scene-source]: old interaction paths remain. |
| P12 User pause, battery/fullscreen policies, lock/display-off hold; per-monitor coverage and display changes | **done** | `crates/desktop/src/hold.rs`, `crates/wallpaper/src/main.rs` | [Holds][holds], [engine main][engine-main]: existing coverage and power policies remain. |
| P13 Live native glass over native shader/image/video/scene wallpaper and moving surfaces | **done** | `crates/wallpaper/src/glass_pass.rs`, `B/liquid.ts`, `W/shell/src/plates.ts` | [Glass transport][glass-transport], [glass pass][glass-pass], [engine][engine]: plates are merged and rendered using the engine's retained frame. |
| P14 Glass over web/app wallpaper (old web snapshot lens, new live capture requirement) | **partial** | `crates/wallpaper/src/sources/web.rs`, `RB/capture.rs`, `W/shell/src/plates.ts` | [Glass transport][glass-transport] only sends plates; [engine][engine] skips plate rendering for native-window sources; [overlay engine][overlay-engine] exists but shell does not request these overlays yet (N5/engine work). |
| P15 Bar changes from wallpaper plate to app-window backdrop while overlapped | **done** | `RB/glass.rs`, `RB/capture.rs`, `B/liquid.ts` | [Bar window][bar-window]: each overlapping bar/island sets Surface glass and omits its wallpaper plate; separate island HWNDs switch independently on window/geometry events. |
| P16 Static sign-in snapshot, per-monitor originals, panic/off/autostart-off restore; keep still on normal quit | **done** | `crates/wallpaper/src/snapshot.rs`, `crates/desktop/src/desktop_wallpaper.rs`, `docs/performance.md:344` | [Snapshot][snapshot], [desktop wallpaper][desktop-wallpaper], [core autostart][autostart]: reversible still-wallpaper behavior remains. |
| P17 Palette-derived auto accent, matching active-theme identity and live styling propagation | **done** | `crates/wallpaper/src/palette.rs`, `crates/core/src/styling.rs` | [Engine palette][engine-palette], [core styling][styling]: native wallpaper/color extraction and Windows application remain. |
| P18 Bundled curated gallery, theme name/hidden, old example-ID/path compatibility, user-theme precedence | **done** | `crates/theme/src/lib.rs`, `themes/catalog.json`, `docs/engine-notes.md:256` | [Theme lookup][theme-schema], `themes/catalog.json`: curated themes and example fallback remain; rain/snow are native shader ports. |

## Windows styling and desktop extras

| ID / feature and settings | Status | Old location | Native evidence and one-line note |
|---|---|---|---|
| X01 `windows.accent` fixed/auto and system accent palette | **done** | `crates/tweaks/src/plan.rs`, `crates/tweaks/src/palette.rs` | [Tweaks plan][tweaks-plan], [styling][styling]: reversible accent application remains. |
| X02 `border` fixed/auto, `caption`, `caption_text` | **done** | `crates/tweaks/src/dwm.rs`, `crates/theme/src/lib.rs` | [DWM styling][dwm], [theme schema][theme-schema]: all window color fields remain. |
| X03 `corners` default/round/small/square; `backdrop` auto/none/mica/acrylic/tabbed | **done** | `crates/tweaks/src/dwm.rs`, `docs/windows-styling.md` | [DWM styling][dwm], [theme schema][theme-schema]: Windows appearance settings are unchanged. |
| X04 `dark_mode` per-window and apps/system light/dark registry state | **done** | `crates/tweaks/src/plan.rs`, `crates/tweaks/src/dwm.rs` | [Tweaks plan][tweaks-plan], [styling][styling]: updates and restores the old system settings. |
| X05 Existing/new windows, public-API allowlist and Microsoft-signature checks | **done** | `crates/core/src/styling.rs`, `crates/inject/src/lib.rs` | [Core styling][styling], `crates/inject/src/lib.rs`: new-window event hook and eligibility checks remain. |
| X06 Original registry values/types/created keys, window reset, quit/theme/panic/crash restore | **done** | `crates/tweaks/src/undo.rs`, `crates/core/src/styling.rs` | [Undo journal][undo], [styling][styling]: old reversible styling contract is preserved. |
| X07 `cursors` built-in mypaper-dark/light (17 roles, multi-DPI, animated busy), named or folder sets | **done** | `crates/tweaks/src/cursors.rs`, `crates/tweaks/src/cursor_art.rs` | [Cursors][cursors], [cursor artwork][cursor-art]: generation, role aliases, fallback and journal remain. |
| X08 `classic_context_menu` opt-in, persistent separate journal, restart-Explorer notice and exact undo | **done** | `crates/tweaks/src/classic_menu.rs`, `crates/core/src/desktop_extras.rs` | [Classic menu][classic-menu], [desktop extras][desktop-extras]: separate lifetime and notice remain. |
| X09 Raised-desktop support, remain behind icons, Explorer restart/reconnect and desktop readiness wait | **done** | `crates/desktop/src/host.rs`, `docs/desktop-extras.md:259` | [Desktop host][desktop-host], [widget host][widget-host]: source implements reconnect and the plan's revised top-level widget placement. |

## Studio

N7 (2026-11-05) replaced the Tauri Studio with the native one (`crates/studio`, on `crates/ui`; `apps/studio` is deleted) and redesigned it ([spec][n7-spec], [how it works][n7-ux]). Old view files below are the Tauri ones, kept as the audit's baseline. **A Studio row is done when native Studio edits every field it lists and previews it with the shell's own views; where a desktop consumer of a field is still missing, the note names the row that tracks it** (those gaps belong to the shell or engine, not to Studio). Golden images of every screen, light and dark: [docs/media/studio][n7-goldens].

| ID / feature and settings | Status | Old location | Native evidence and one-line note |
|---|---|---|---|
| S01 Page navigation, remembered route/selection, sidebar active theme/playing chip | **done** | `S/App.tsx`, `S/lib/pages.ts`, `S/lib/nav.tsx`, `S/components/Sidebar.tsx` | [Frame][n7-view], [app][n7-app]: sidebar of the six places plus Settings, status block (wallpaper, Pause, a stopped part); last page and selection kept in `studio.json`. |
| S02 Ctrl+K search pages/settings/themes/actions, jump/open More/scroll/flash a setting | **done** | `S/components/CommandPalette.tsx`, `S/lib/settingsIndex.ts` | [Palette][n7-search] over the [settings index][n7-settings-index] (every setting, wallpaper, widget, integration, action): Enter opens the page, selects the owner, opens its More, scrolls and flashes; Space flips a focused switch. |
| S03 Undo/redo Ctrl+Z/Ctrl+Shift+Z/Ctrl+Y, merged slider edits, autosave status/errors/toasts | **done** | `S/lib/history.tsx`, `S/lib/useAutosaved.ts`, `S/lib/studio.tsx` | [Document store][n7-docs]: app-wide undo/redo (Ctrl+Z, Ctrl+Shift+Z, Ctrl+Y), a drag is one step, save state in the page bar, toasts with Undo; unreadable files are never written. |
| S04 Desktop monitor picker, stage select wallpaper/widget/bar; active-theme strip, pause/resume, inspector navigation | **done** | `S/views/Desktop.tsx`, `S/components/stage/DesktopScene.tsx` | [Home][n7-home] on the [stage][n7-stage]: display chips, the real desktop (shell views for bars and widgets, engine glass), click the wallpaper, a widget or a bar to edit it; wallpapers strip; Pause in the sidebar. |
| S05 Themes gallery: search, Yours/Built-in/All filter, grouping, cards/thumbnails, apply with Undo | **done** | `S/views/Themes.tsx`, `S/components/ThemeCard.tsx` | [Wallpapers][n7-wallpapers], [library][n7-library]: search (Ctrl+F), All/Yours/Built in, categories, cards with previews; a click selects and **Use** applies with Undo (D4). |
| S06 Invalid-config banner and Reset to defaults | **done** | `S/views/Themes.tsx`, `S/lib/studio.tsx`, `ST/config_edit.rs` | [Store][n7-docs]: a file that does not parse is never written; its pages show a banner with the reason and **Start over** (keeps a backup). |
| S07 Duplicate built-in theme to edit; user theme editing, active state and source credits | **done** | `S/views/ThemeEditor.tsx`, `S/components/inspectors/MakeCopy.tsx`, `S/components/inspectors/DesktopInspectors.tsx` | [Library][n7-library]: Duplicate a built-in in place; your own wallpapers edit their `theme.json` directly (validated); active state and credits on cards and in the inspector. |
| S08 New theme: picture/video/WGSL/scene/file, image folder, URL/local HTML, program and args | **done** | `S/views/NewTheme.tsx`, `ST/source.rs`, `ST/themes.rs` | [Add wallpaper][n7-wallpapers]: picture, video, WGSL, 3D scene, any file, an image folder, a web page (URL or local HTML), a program with arguments (Unity preset). |
| S09 New-theme name, cover/contain/stretch/center fit, source preview, change source, Use it right away | **done** | `S/views/NewTheme.tsx`, `ST/manifest.rs` | [Add wallpaper][n7-wallpapers]: name, fit (cover/contain/stretch/center), source preview, change source, Use it right away. |
| S10 ThemeEditor Wallpaper tab: source metadata/credits, React to mouse for web/scene | **done** | `S/views/ThemeEditor.tsx`, `S/components/inspectors/DesktopInspectors.tsx` | [Wallpaper inspector][n7-wallpapers]: source, credit and license; React to the mouse for web and scene wallpapers; built-ins offer a copy. |
| S11 ThemeEditor Windows tab; Windows page: accent, light/dark, corners, material, border/title/text and cursor picker | **done** | `S/views/Windows.tsx`, `S/components/inspectors/WindowsStyleFields.tsx` | [Look › Windows][n7-look]: accent, apps light/dark, corners, material, edge, title bar and text, pointer. D2: your layer is `config.json` › `look.windows` ([schema][n7-config-look]) over the wallpaper's, applied by [core][n7-styling]; existing theme styling migrates. |
| S12 Windows page Right-click menu: enabled, reorder/hide/add built-ins, custom label/command/args, classic-menu switch | **done** | `S/views/Windows.tsx`, `S/components/inspectors/MenuFields.tsx` | [Settings › Desktop][n7-settings]: right-click menu on/off, items reorder/hide/add built-ins, your own (label, command, arguments), full menus in Explorer. |
| S13 Widgets catalog/search/add; missing-widget placeholder; per-display list, select/remove/duplicate/size and stage arrange | **done** | `S/views/Widgets.tsx`, `S/components/inspectors/WidgetCatalog.tsx`, `S/lib/desktop.ts` | [Widgets][n7-widgets]: Add widget catalog with search, per-display list, select/remove/duplicate/size, drag and resize on the canvas; a widget that is not installed is a placeholder whose inspector removes it. |
| S14 Each widget's manifest setting controls: string/number/boolean/select/slider/font/list/place/file/image, grouped options/reset | **done** | `S/components/inspectors/WidgetSettingControl.tsx`, `S/components/inspectors/WidgetInspector.tsx` | [Widget inspector][n7-widgets]: every manifest control (boolean, number, slider, select, string, font, color, file, image, list, place) in the manifest's groups. Widget runtime gaps stay on their widget rows. |
| S15 Global and instance widget look, inherited/reset-own look, overlay switch, Arrange on desktop | **done** | `S/views/Widgets.tsx`, `S/components/inspectors/LookFields.tsx` | [Look][n7-look] is the look of all widgets (Different for › Widgets); a widget's own look (follows Look, or its own background and tint); Always on top; Arrange on desktop. Host material gaps: H10, H14, H16, H17, H19. |
| S16 Real widget content/settings/appearance preview on stage | **done** | `S/components/stage/WidgetFrame.tsx`, `S/lib/widgetHost.ts` | [Stage][n7-stage]: widgets drawn by the shell's own views ([preview facade][n7-shell-preview]) with their settings and look, glass drawn by the engine ([preview protocol][n7-glass]). |
| S17 ThemeEditor widget/bar appearance fields and widget glass Strength/Bend/Shine/Tint/Blur | **done** | `S/views/ThemeEditor.tsx`, `S/components/inspectors/LookFields.tsx` | [Look][n7-look]'s scope switch edits the look a wallpaper brings (your own wallpapers; built-ins via a copy); Material (clarity, blur, refraction, highlight, tint) replaces the old glass knobs (D7). Shell-side theme-file watching: backlog item 5. |
| S18 Theme CSS choose/replace/remove, bar CSS variable controls and dropped decoration controls | **dropped on purpose** | `S/views/ThemeEditor.tsx`, `S/components/bars/ThemeVarsPanel.tsx`, `S/components/bars/PaintEditor.tsx` | [Plan lines 350–352][losses]: native Studio has no CSS, theme-variable or decoration controls. |
| S19 Bars pairings, independent top/dock enable/design, live thumbnails and Windows-taskbar mode/alignment | **done** | `S/views/Bars.tsx`, `S/components/bars/DesignPicker.tsx`, `S/components/bars/BarPanels.tsx` | [Bars][n7-bars]: presets, each bar on/off and its design (tiles are the shell's real bar views, rebuilt on every change), Windows taskbar mode and alignment. |
| S20 Bars geometry: top shape/edge/height/width/room, dock style/edge/size/room/alignment/button/titles; display selectors | **done** | `S/components/bars/BarSettings.tsx`, `S/components/bars/DockPanel.tsx` | [Bars][n7-bars]: top edge/height/shape/width/margin/padding/spacing; dock style/edge/size/margin/padding/spacing/apps alignment/button length/titles; displays. Shell gap D02. |
| S21 Bars clocks (seconds/format); running/split/counter/previews/delay/bounce/attention/indicators/extras/magnify/autohide | **done** | `S/components/bars/BarSettings.tsx`, `S/components/bars/DockPanel.tsx` | [Bars][n7-bars]: clock seconds and format; running, one per window, count, previews and delay, bounce, attention, indicator, after the apps, magnify (scale, reach), auto-hide. Shell differences D12, D14, D17, D18. |
| S22 Pins: import Windows/default, custom path/name list, order/remove/add | **done** | `S/components/bars/PinnedList.tsx` | [Bars][n7-bars]: Windows' pins or your own (start from Windows'), add, reorder (Alt+arrows), remove. |
| S23 Modules: drag stage/list reorder/add/break/remove; custom module creation/deletion; command/args/interval | **done** | `S/components/bars/ModuleOptions.tsx`, `S/components/bars/BarFrame.tsx`, `S/lib/barsModules.ts` | [Bars][n7-bars]: drag modules on the stage or reorder in lists, add (built-ins, a gap, integrations, your own), remove; your own module's program, arguments and interval; delete it everywhere. |
| S24 Module click/scroll action picker, icon and text/fill/border/padding/radius/size/weight controls | **done** | `S/components/bars/ActionPicker.tsx`, `S/components/bars/ModuleOptions.tsx` | [Module inspector][n7-bars]: click, middle, right, scroll up/down (its own, nothing, an action, a program); icon, text color, background, edge, padding, corners, text size, weight. Shell gaps T20, T22, T24. |
| S25 Bar look: light/dark, background/material, glass knobs, corners, state edges/shadows/buttons and indicator colors | **done** | `S/components/bars/LookPanel.tsx`, `S/components/bars/PaintEditor.tsx` | [A bar › More][n7-bars]: style, light/dark, drawn like, corners, text size; edge line and color, shadow, buttons at rest/pointed/pressed (fill, text, corners); the dock's running-indicator colors; material via [Look][n7-look] (Different for › Bars). Consumers: C06–C12. |
| S26 Bar motion enabled/duration/curve/show style, hover/press/open/magnify/bounce/attention, reduce-motion/performance | **done** | `S/components/bars/MotionPanel.tsx`, `S/components/bars/CurvePicker.tsx` | [A bar › More › Motion][n7-bars]: animations, appearing, curve, duration; pointing, pressing, menus opening; the dock's magnify, launch bounce, attention. Reduce motion and Save power in [Settings][n7-settings]. Consumers: C13–C15. |
| S27 Start layout/files/Win key/pins, audio output switch, calendar zones/first day, tray-host setting | **done** | `S/components/bars/MenusPanel.tsx` | [Bars › Panels][n7-bars]: Start (layout, recent files, the Windows key, its own pins or the dock's), Sound (switch output), Calendar (time zones, first day), Tray (only while the Windows taskbar shows), Notifications. N3's flyouts read them. |
| S28 Icon packs choose/create, per-app image add/remove and previews | **done** | `S/components/bars/IconsPanel.tsx`, `ST/iconpacks.rs` | [Look › Text and icons][n7-look]: choose a pack, start your own, give an app its icon (picture copied into the pack), remove one; the bars reload at once. Bundled-pack lookup in the shell: C16. |
| S29 Per-display overrides list/remove and inherited values | **done** | `S/components/bars/MonitorPicker.tsx`, `S/components/bars/BarPanels.tsx`, `S/lib/barsDoc.ts` | [Bars][n7-bars]: Per display lists the file's overrides (only when there are any) and removes one. |
| S30 Settings: Start with Windows, startup still, pause on battery/fullscreen, start/stop/status, emergency hotkey display | **done** | `S/views/Settings.tsx`, `ST/autostart.rs`, `ST/engine.rs` | [Settings][n7-settings]: Start with Windows, the still while Windows starts, pause on battery and behind fullscreen, graphics card, frame rate; Troubleshoot starts/stops/restarts; the emergency hotkey shown in Keyboard and Troubleshoot. |
| S31 Settings “Parts of mypaper” installed/running components and startup-method/path detail | **done** | `S/views/Settings.tsx`, `ST/engine.rs` | [Troubleshoot][n7-system]: the three parts (core, engine, shell) with running state, memory and Restart; how it starts and where it is installed; diagnostics and logs. |
| S32 External-edit watcher and refresh of config/themes/bars/widgets | **done** | `ST/watch.rs`, `S/lib/hooks.ts` | [Watcher][n7-watch]: config, bars, widgets, integrations and wallpaper folders reload on outside edits; an unreadable edit keeps the last good values on screen. |
| S33 Native Studio renderer, accurate native previews and no Tauri processes | **done** | `apps/studio/` (old implementation); N7 replacement in plan | Native `mypaper-studio` (software Direct2D, one window, no WebView or GPU device; [cost][n7-ux]); previews are the shell's own views and the engine's glass; `apps/studio` and its pnpm/Tauri build are deleted. |

## Tray and hotkeys

| ID / feature and settings | Status | Old location | Native evidence and one-line note |
|---|---|---|---|
| R01 Tray icon, theme-name tooltip, re-add after Explorer restart | **done** | `crates/core/src/tray.rs` | [Core tray][core-tray]: icon/tooltip and TaskbarCreated handling are retained. |
| R02 Pause/Resume, theme picker/current mark, Open Studio, Restart wallpaper, Quit | **done** | `crates/core/src/tray.rs` | [Core tray][core-tray]: same config writes and supervisor actions remain. |
| R03 Edit widgets / show overlay widgets, top-bar/dock toggles, taskbar keep/autohide/hide | **done** | `crates/core/src/tray.rs` | [Core tray][core-tray], [widget host][widget-host]: retained messages/config edits now reach the implemented overlay surfaces and edit controls. |
| R04 Desktop icons visibility/large/medium/small, mypaper menu and classic-menu toggles/checks | **done** | `crates/core/src/tray.rs`, `crates/core/src/desktop_extras.rs` | [Core tray][core-tray], [desktop extras][desktop-extras]: all desktop submenu actions remain. |
| R05 Ctrl+Alt+Shift+Backspace panic: stop children, stock taskbar/wallpaper/style/cursors restore | **done** | `crates/core/src/main.rs`, `crates/core/src/bars.rs`, `crates/core/src/styling.rs` | [Core main][core-main], [core bars][core-bars], [styling][styling]: hotkey stays independent of shell responsiveness. |
| R06 Ctrl+Alt+Shift+W and Ctrl+Alt+Shift+O, registered edit/overlay messages | **done** | `RW/main.rs`, `RW/app.rs` | [Widget control][widget-control]: legacy message window and both global hotkeys are registered by the native shell. |
| R07 Lone Win / Win+N / Win+A with native other-shortcut pass-through | **done** | `RB/flyouts/winkey.rs` | [Flyouts][flyouts], [Win-key provider][winkey]: lone Win and Win+N/Win+A use the native panels with other combinations passed through. |
| R08 Autostart on/off/status CLI, per-user logon task with Run fallback, disable restores still | **done** | `crates/core/src/autostart.rs`, `docs/performance.md:375` | [Autostart][autostart]: original CLI and reversible task setup remain. |
| R09 Supervised restart/backoff, stale-process cleanup, taskbar crash/panic journal restoration | **done** | `crates/core/src/child.rs`, `crates/core/src/bars.rs`, `crates/core/src/restart.rs` | [Core main][core-main], [core bars][core-bars]: shell replaces separate bar/widget children and core replays its taskbar journal after exit. |
| R10 Tray menu matches shell's material/style | **done** | `crates/core/src/tray.rs` (stock menu before rebuild) | [Core tray][core-tray], [shell tray menu][shell-tray-menu]: typed copied model to the shell's glass Menu; bounded acknowledgement and stock fallback when the shell is unavailable. |

## Config and appearance contracts

The authoritative schemas at the old commit are Rust types/validators, not four standalone JSON Schema files: `crates/config/src/lib.rs`, `crates/bars/src/config.rs` + `style.rs` + `flyouts/config.rs`, `crates/widgets/src/layout.rs` + `manifest.rs`, and `crates/theme/src/lib.rs`. Studio's `bars-schema` exports the bar schema. Files named below are user configuration documents.

| ID / feature and settings | Status | Old location | Native evidence and one-line note |
|---|---|---|---|
| C01 `config.json`: theme, paused, pause_on_battery, pause_on_fullscreen, static_snapshot | **done** | `crates/config/src/lib.rs` | [Core config][config-schema], [engine main][engine-main]: all original fields are retained and watched. |
| C02 `config.json`: desktop_menu.enabled/items/custom args and classic_context_menu | **done** | `crates/config/src/desktop_menu.rs`, `crates/config/src/lib.rs` | [Desktop config][desktop-config], [desktop items][desktop-items]: native shell consumes the old menu schema. |
| C03 `bars.json`: version/preset, top/dock geometry/modules/clocks/pins, taskbar mode/alignment | **done** | `RB/config.rs`, `RB/presets.rs` | [Bar schema][bars-schema], [presets][presets]: original validation and defaults remain; runtime exceptions are listed above. |
| C04 Shared look/animation → per-bar settings; role then device overrides and nonoverridable global fields | **done** | `RB/config.rs`, `RB/presets.rs` | [Bar schema][bars-schema], [bar host][bar-host]: layered config and primary/secondary/device precedence remain. |
| C05 Unknown-field/range rejection, last-good bars config, partial/atomic writers preserving unrelated keys | **done** | `RB/config.rs`, `crates/config/src/lib.rs` | [Bar schema][bars-schema], [bar host][bar-host], [core config][config-schema]: typed parsing and bar/config edit helpers remain. |
| C06 Bar appearance surface/tint/opacity/blur/radius/border/text/accent/font/scale/shadow | **done** | `RB/look.rs`, `crates/theme/src/lib.rs` | [Bar tokens][bar-look], [glass transport][glass-transport], [live optics evidence](../../docs/bars.md#finer-glass-controls-2026-11-06): appearance fields reach bars, previews, flyouts and menus, including preset overrides. Blur is projected once into physical pixels at publication/open and the wire accepts 256 px; live 1.2x captures and shared-memory snapshots show 76.8 px accepted. |
| C07 `look.background` none/solid/wallpaper/live blur and `tint` | **done** | `RB/style.rs`, `B/style/look.ts`, `RB/blur.rs` | [Bar tokens][bar-look], [window][bar-window], [live optics evidence](../../docs/bars.md#finer-glass-controls-2026-11-06): none/solid/wallpaper/live blur retain their routing and tint. Cached background brightness now travels in styled plates and after every overlay Open; grey/dark and vivid/bright live captures show 0.55 versus 1.6 brightness. |
| C08 Gradients, image backgrounds, noise/glow, CSS vars/skin, bevel/scoop/notch corner decoration | **dropped on purpose** | `RB/style.rs`, `RB/vars.rs`, `B/style/apply.ts` | [Plan lines 222 and 350–352][style-plan]: old decorative values may parse but have no native renderer. |
| C09 `look.scheme` and Windows accent tokens including light/dark variants | **done** | `B/style/look.ts`, `RB/accent.rs` | [Bar tokens][bar-look], [flyout look][flyout-look]: explicit/auto light and dark schemes reach cached native palettes; all seven Windows accent shades resolve independently. Pure palette tests and live light/dark captures recorded in docs/bars.md. |
| C10 `look.saturation`, round/squircle shape, per-state border/shadow and item rest/hover/active fill/color/border/scale/radius | **done** | `RB/style.rs`, `B/style/apply.ts` | [Bar tokens][bar-look], [view][bar-view], [kit contours](../../crates/ui/src/continuous.rs), [engine evidence](../../docs/glass.md#engine-parity-6-november-2026): saturation and state controls remain consumed. Continuous overlays now use the shader's own coverage with a rectangular compositor bound, matching kit/plate corners without circular trimming or an outer shadow. GPU alpha fixtures retain 18 corner pixels a circular clip would trim; the live continuous menu was inspected. Default lab frames remain bit-identical. |
| C11 `look.glyphs` default/fluent/symbols and menu `family` mono/fluent/macos/material | **done** | `RB/style.rs`, `B/style/glyph-sets.ts` | [Bar icons][bar-icons], [flyout look][flyout-look]: embedded default/symbol SVGs, Fluent font and shared menu families are consumed. |
| C12 Glass style liquid/frosted/none, intensity/refraction/highlight/tint/blur/depth/sheen and flyouts blur/tint | **done** | `RB/style.rs`, `B/liquid.ts`, `B/style/apply.ts` | [Bar tokens][bar-look], [flyout look][flyout-look], [engine evidence](../../docs/glass.md#engine-parity-6-november-2026): existing controls remain wired, including 256 px physical blur. Legacy bar `depth` now resolves to independent `inner_shadow` on plates and overlays; sheen lights the top and the existing engine `depth` keeps its top-rim shade. Zero shadow skips its SDF/blur. GPU fixtures prove bottom-only shading with unchanged top pixels/alpha; coherent live plate/style/extension stamps carry shadow 1 for bars and 0 for widgets. |
| C13 Animation enabled/reduceMotion system/always/never and performance auto/on/off | **done** | `RB/style.rs`, `B/style/motion.ts` | [Bar motion][bar-motion], [flyout look][flyout-look], [glass evidence](../../docs/bars.md#finer-glass-controls-2026-11-06): enabled, reduceMotion system/always/never and performance policy now reach the engine fade too. Cached overlay styles send fade_ms=0 for disabled/reduced motion or presentation none. Resolver tests cover policy precedence, and reduced-motion launcher captures show content without a transition; capture startup latency remains separate. |
| C14 Duration and named/Bézier/spring curves, root/bar/top/dock precedence, slide/fade/pop/morph/scale/none styles | **done** | `B/style/motion.ts`, `RB/style.rs` | [Bar motion][bar-motion], [window][bar-window], [engine evidence](../../docs/glass.md#engine-parity-6-november-2026): the resolved content timing/curve now reaches glass, including 5000 ms, every named curve, custom Béziers and physical springs; reduced motion remains instant. DirectComposition runs bounded polynomial pieces without engine animation frames. Tests compare all named curves with content and sampled pieces within one alpha step; DPI-aware live captures show a full five-second linear menu fade. Glass slides retain stationary fades to avoid capture flashes. Additional live curve/reduced-motion captures were skipped when the foreground safety guard refused input. |
| C15 Hover/press/open/magnify/bounce/attention animation leaves | **done** | `B/style/motion.ts`, `RB/style.rs` | [Bar motion][bar-motion], [window][bar-window], [flyout look][flyout-look]: hover/press, popup/menu open, magnify, launch-bounce and attention use independent resolved leaves. Show/hide has its own timing; interaction state and frame sources stop on settlement. Pure precedence/timing tests cover disabling and performance policy. |
| C16 `icons` pack selection; AUMID/path/exe/extension/key matching, mono/missing fallback and bundled pack | **done** | `RB/icon_pack.rs`, `B/style/icon-pack.ts`, `bars/shell/public/icons/mono/` | [Bar icons][bar-icons]: user-pack ranking/masks plus embedded mono assets and dynamic network/volume/media/battery/trash keys. |
| C17 Launcher/calendar/audio/tray configs accepted with validation and inherited pins | **done** | `RB/flyouts/config.rs` | [Flyout config][flyout-config], [flyouts][flyouts]: native consumers honor validated settings, omitted-pin inheritance, calendar zones, output switching and explicit two-layout migration. |
| C18 `widgets.json` version/grid/instances/overlay and per-instance monitor/cell/size/settings/appearance | **done** | `RW/layout.rs`, `W/shell/src/host.ts` | [Layout][widget-layout], [host][widget-host]: original layout shape retained; overlay surfaces and invalid-file recovery/write refusal are implemented (H10/H21). |
| C19 `widget.json` settings types/defaults/ranges/groups/replaces/size/resizable/instances/preset | **done** | `RW/manifest.rs`, `W/*/widget.json` | [Manifest parser][widget-manifest], [catalog][widget-catalog]: native catalog retains all built-in setting declarations and migration aliases. |
| C20 Instance/shared storage, private picked files and compatible duplicates | **done** | `RW/storage.rs`, `RW/pick.rs`, `RW/app.rs` | [Storage][widget-storage], [picker][widget-picker], [host][widget-host]: compatible instance/shared storage, owned duplicate URLs and post-save removal cleanup; malformed storage is backed up and write-protected even with a warm cache. |
| C21 `theme.json` name/hidden, seven wallpaper kinds and all source settings | **done** | `crates/theme/src/lib.rs` | [Theme schema][theme-schema]: existing source fields survive; exact keys are enumerated in Wallpaper. |
| C22 `theme.json` windows, bars/widgets appearance/glass and partial-layer resets | **done** | `crates/theme/src/lib.rs`, `RW/look.rs`, `RB/look.rs` | [Theme schema][theme-schema], [widget look][widget-look], [bar tokens][bar-look]: Windows absent/null/value layering and widget appearance/glass remain consumed; bars now layer theme appearance/glass under explicit shared/bar/monitor settings. Removing a partial theme layer restores design defaults, covered by resolver/reset tests. |
| C23 Theme folder asset confinement, hot reload, failed reload keeps previous theme | **done** | `crates/theme/src/lib.rs`, `crates/wallpaper/src/main.rs` | [Theme schema][theme-schema], [engine main][engine-main]: wallpaper asset validation/reload remains; shell appearance watcher gap is H20. |
| C24 User-editable config locations, user-first themes/plugins/icons, logs and reversible journals | **done** | `RB/files.rs`, `RW/files.rs`, `crates/config/src/lib.rs`, `docs/*.md` | [Core config][config-schema], [widget files][widget-files], [undo][undo]: existing APPDATA/LOCALAPPDATA contracts remain. |
| C25 Native style selection liquid/mono/solid and planned oneui/fluent plus one-switch Windows token sync | **done** | `crates/theme/src/windows.rs`, `crates/config/src/look.rs`, `crates/tweaks/src/plan.rs`, `crates/core/src/styling.rs` | [Native Studio look projection][n7-look], [bar tokens][bar-look], [Windows sync and live evidence](../../docs/windows-styling.md#verification-2026-11-06): all five styles supply frame/caption/corners/backdrop, shared palette accent, app/system mode and both ColorPrevalence defaults through the undo journal. Explicit Windows fields/nulls win independently; the app styler shares accent/dominant tokens. Ten live cases restored exact config/palette bytes and all eight registry values; native caption pixels and Explorer/Settings captures were inspected. Studio follow-ups expose the new optional accent switches. |
| C26 Integrated/high-performance/auto GPU and FPS globally/per-wallpaper, user-visible Studio controls | **partial** | Old per-source fps/scale; [plan lines 21–25][decisions] (expanded requirement) | [Engine config][engine-config], [theme schema][theme-schema], [engine][engine]: schema/runtime added, but current Studio has no GPU/global-per-wallpaper FPS editor; engine optimization is active. |

The T19 action inventory is: `default`, `none`, `start`, `search`, `notifications`, `quick-settings`, `calendar`, `network`, `mixer`, `native-start`, `native-notifications`, `native-quick-settings`, `task-manager`, `settings`, `show-desktop`, `media-toggle`, `media-next`, `media-previous`, `volume-up`, `volume-down`, `volume-mute`, `workspace-next`, `workspace-previous`, `lock`, `explorer`, `{exec,args}` and `{open}`. Every trigger remains expressible; the native-panel distinction is specifically pending N3.

### Coverage and limits

- `bars/shell/src/**`: inspected the top/dock builders, actions, clock format, drag/magnify/previews, all flyout/launcher/notification views, menu/anchor/region/glass paths and look/motion/icon modules. CSS-only decorations are tracked as deliberate losses, not quietly counted done.
- `crates/bars/**`: covered config/presets/style, appbar/taskbar journal, app/window/icon enumeration, audio/media/network/stats/custom/trash/desktops/accent, tray/UIA/tray-host, all flyout providers and the Win-key hook. Providers surviving in a library are not counted as a finished native flyout.
- `crates/widgets/**` and all 17 `widgets/<id>/widget.json` + `main.js`/logic files: manifest/settings/migration/layout/storage, media/spectrum/stats/network/plugins, pickers, user info, edit/appearance/overlay behavior and native consumers are mapped above.
- `apps/studio/**`: all eight pages, inspector controls, stage preview/rendering, command search/history, settings index, Tauri commands/config/theme/source/widget/icon/plugin/status/watcher paths are covered. The retained controls' runtime gaps are explicitly cross-referenced.
- `crates/core`, `crates/desktop`, `crates/tweaks`: tray/hotkeys/startup/supervision, desktop menu/view/host/raw input/holds, window styling/cursors/classic menu/undo are covered.
- Old `docs/bars.md`, `widgets.md`, `desktop-extras.md`, `windows-styling.md` and `studio-ux.md` provide the interaction/control contracts; `web-and-app-wallpapers.md` provides source limits; `engine-notes.md` adds EXIF, 10-bit/HDR and curated-theme compatibility; `performance.md` adds static snapshots/autostart; `writing-plugins.md` and `writing-widgets.md` cover extension/SDK contracts. Historical measurements and developer-only harness instructions are not additional user settings.
- Pre-existing nonfeatures are **not included in missing counts**: taskbar overlay badges/progress (`ITaskbarList3`, old `docs/bars.md`, Tray host caveats); automatic web/app crash recovery and multi-monitor/input for app wallpapers (old `docs/web-and-app-wallpapers.md`, Limits); styling the interior of Explorer/Settings or arbitrary app chrome (old `docs/windows-styling.md`, also deferred by [plan lines 26–27][decisions]).
- No performance gate, live click behavior, IME scenario, screenshot inclusion or visual fidelity was measured here. Native “done” rows are static source findings. Native UI Automation has primitives in `crates/ui/src/a11y.rs`, but that does not supply missing keyboard traversal or scrolling automatically.

## Prioritized missing and partial work (excluding N3 and intentional losses)

The remaining rows belong to retained appearance, motion and engine/Studio controls; widget behavior rows are implemented. Performance gates are tracked separately in docs/widgets.md.

1. **P1 — Finish glass over web/app wallpapers.** Connect top-level widget/bar surfaces to the existing overlay engine when the wallpaper owns native windows. Covers P14; coordinate with the active engine agent (N5), separate from N3's app-overlap/flyout work.
2. **P2 — Expose the new GPU/FPS settings.** Add global/per-wallpaper GPU and frame-rate controls to Studio while preserving explicit choices. Covers C26; coordinate with active engine optimization, and distinguish web/app self-paced rendering in the UI.

## + more: ten valuable improvements after parity

These are recommendations/inferences from comparable products, not claims that every reference implements the identical feature. Costs are rough focused engineering time **after the native primitives/N3 work is complete**, including focused checks; they are not measured performance or delivery commitments. None requires injection into Explorer.

| Rank | Improvement | Reference and why it fits | One-line cost estimate |
|---|---|---|---|
| 1 | Intelligent dock hide only when an app overlaps, with an optional reveal delay | [StartAllBack](https://www.startallback.com/) explicitly offers hide-when-covered; existing window events and dock springs supply most of the mechanism. | **2–3 days, small:** overlap policy/config and monitor/fullscreen edge cases. |
| 2 | Drop files onto app icons to open them, and drop apps/folders to pin | [StartAllBack](https://www.startallback.com/) advertises taskbar drag/drop; native HWNDs can own the Windows OLE drop target directly. | **3–5 days, medium:** OLE drop target, target activation and clear hover feedback. |
| 3 | Win+number launch/switch and Win+T keyboard task navigation | [Windows keyboard shortcuts](https://support.microsoft.com/en-au/accessibility/windows/keyboard-shortcuts-in-windows) establish this expectation; dock order/window groups already exist. | **2–3 days, small:** extend keyboard routing while preserving other Win combinations. |
| 4 | App Jump Lists for recent/pinned documents and supported tasks | [Windows taskbar policy reference](https://learn.microsoft.com/en-us/windows/configuration/supported-csp-taskbar-windows) documents Jump List items; native typed menus can render them. | **4–7 days, medium:** shell list retrieval/activation and per-app compatibility; do not assume taskbar progress interception is solved. |
| 5 | Launcher actions, calculator and short aliases alongside app/file search | [Tahoe Spotlight](https://www.apple.com/newsroom/2025/06/macos-tahoe-26-makes-the-mac-more-capable-productive-and-intelligent-than-ever/) adds actions/quick keys, and [Seelen](https://seelen.io/) emphasizes command launching. | **3–5 days, medium:** typed action providers and ranking; keep actions explicit and local. |
| 6 | Clipboard search available in either retained launcher layout | [Tahoe Spotlight](https://www.apple.com/newsroom/2025/06/macos-tahoe-26-makes-the-mac-more-capable-productive-and-intelligent-than-ever/) includes clipboard browsing; mypaper already has the Windows history provider. | **1–2 days, small:** shared provider/result view and disabled-history state, independent of dashboard retention. |
| 7 | Editable Control Center tile order, size and favorites pinned into the bar | [Tahoe](https://www.apple.com/newsroom/2025/06/macos-tahoe-26-makes-the-mac-more-capable-productive-and-intelligent-than-ever/) exposes customizable controls/layouts; native bar modules can share the same action descriptors. | **3–5 days, medium:** layout/config editor and drag targets using existing tiles. |
| 8 | Window/workspace actions: move to display, snap left/right and restore a small saved layout | [Seelen's tiling manager](https://seelen.io/) and [Windows Snap layouts](https://learn.microsoft.com/en-us/windows/apps/desktop/modernize/ui/apply-snap-layout-menu) set the multitasking reference; start with supported window actions rather than a full tiler. | **1–2 weeks, medium/large:** geometry/DPI/restore rules and focused multi-monitor validation. |
| 9 | Compact live activity chip for timers and explicitly integrated download/build providers | [Tahoe Live Activities](https://www.apple.com/newsroom/2025/06/macos-tahoe-26-makes-the-mac-more-capable-productive-and-intelligent-than-ever/) validates glanceable ongoing state; existing timer/plugin/event models can feed a shared chip. | **3–5 days plus each provider, medium:** model/lifecycle/expand UI; no promise to read arbitrary apps' taskbar progress. |
| 10 | High-contrast/large-target profile and a searchable shortcut guide shared by every surface | [Windows taskbar and focus accessibility](https://support.microsoft.com/en-au/accessibility/windows/make-it-easier-to-focus-on-tasks) and [Windows shortcuts](https://support.microsoft.com/en-au/accessibility/windows/keyboard-shortcuts-in-windows) establish accessible interaction expectations; shared native tokens make this inexpensive across surfaces. | **3–5 days, medium:** system preference subscription, token profile and guide; baseline missing keyboard controls remain parity work above. |





[app-index]: ../../crates/bars/src/flyouts/index.rs
[app-source]: ../../crates/wallpaper/src/sources/app.rs
[appbar]: ../../crates/bars/src/appbar.rs
[apps]: ../../crates/bars/src/apps.rs
[autostart]: ../../crates/core/src/autostart.rs
[bar-host]: ../../crates/shell/src/bars/mod.rs
[bar-icons]: ../../crates/shell/src/bars/icons.rs
[bar-look]: ../../crates/shell/src/bars/look.rs
[bar-motion]: ../../crates/shell/src/bars/motion.rs
[bar-services]: ../../crates/shell/src/bars/services.rs
[bar-view]: ../../crates/shell/src/bars/view.rs
[bar-window]: ../../crates/shell/src/bars/bar.rs
[bars-schema]: ../../crates/bars/src/config.rs
[bars-style]: ../../crates/bars/src/style.rs
[calendar-ui]: ../../crates/ui/src/calendar.rs
[classic-menu]: ../../crates/tweaks/src/classic_menu.rs
[clipboard-provider]: ../../crates/bars/src/flyouts/clipboard.rs
[clock-format]: ../../crates/bars/src/clock.rs
[config-schema]: ../../crates/config/src/lib.rs
[core-bars]: ../../crates/core/src/bars.rs
[core-main]: ../../crates/core/src/main.rs
[core-tray]: ../../crates/core/src/tray.rs
[cursor-art]: ../../crates/tweaks/src/cursor_art.rs
[cursors]: ../../crates/tweaks/src/cursors.rs
[custom]: ../../crates/bars/src/custom.rs
[desktop-config]: ../../crates/config/src/desktop_menu.rs
[desktop-extras]: ../../crates/core/src/desktop_extras.rs
[desktop-hook]: ../../crates/shell/src/desktop_menu/hook.rs
[desktop-host]: ../../crates/desktop/src/host.rs
[desktop-items]: ../../crates/shell/src/desktop_menu/items.rs
[desktop-menu]: ../../crates/shell/src/desktop_menu/mod.rs
[desktop-view]: ../../crates/desktop/src/desktop_view.rs
[desktop-wallpaper]: ../../crates/desktop/src/desktop_wallpaper.rs
[dock-preview]: ../../crates/shell/src/bars/preview.rs
[dwm]: ../../crates/tweaks/src/dwm.rs
[engine]: ../../crates/wallpaper/src/engine.rs
[engine-config]: ../../crates/config/src/engine.rs
[engine-main]: ../../crates/wallpaper/src/main.rs
[engine-palette]: ../../crates/wallpaper/src/palette.rs
[flyout-config]: ../../crates/bars/src/flyouts/config.rs
[flyout-providers]: ../../crates/bars/src/flyouts/mod.rs
[flyouts]: ../../crates/shell/src/flyouts/mod.rs
[glass-pass]: ../../crates/wallpaper/src/glass_pass.rs
[glass-transport]: ../../crates/shell/src/glass.rs
[holds]: ../../crates/desktop/src/hold.rs
[image-source]: ../../crates/wallpaper/src/sources/image.rs
[language-provider]: ../../crates/bars/src/flyouts/language.rs
[menu-look]: ../../crates/shell/src/look.rs
[mixer-provider]: ../../crates/bars/src/flyouts/mixer.rs
[notifications-provider]: ../../crates/bars/src/flyouts/notifications.rs
[overlay-capture]: ../../crates/wallpaper/src/overlay/capture.rs
[overlay-engine]: ../../crates/wallpaper/src/overlay/mod.rs
[presets]: ../../crates/bars/src/presets.rs
[quick-provider]: ../../crates/bars/src/flyouts/quick.rs
[raw-input]: ../../crates/desktop/src/raw_input.rs
[scene-source]: ../../crates/wallpaper/src/sources/scene.rs
[shader-source]: ../../crates/wallpaper/src/sources/shader.rs
[slideshow-source]: ../../crates/wallpaper/src/sources/slideshow.rs
[snapshot]: ../../crates/wallpaper/src/snapshot.rs
[styling]: ../../crates/core/src/styling.rs
[surface]: ../../crates/ui/src/surface.rs
[text-field]: ../../crates/ui/src/text_field.rs
[theme-schema]: ../../crates/theme/src/lib.rs
[trash]: ../../crates/bars/src/trash.rs
[tray-host]: ../../crates/bars/src/tray_host.rs
[tweaks-plan]: ../../crates/tweaks/src/plan.rs
[ui-menu]: ../../crates/ui/src/menu.rs
[undo]: ../../crates/tweaks/src/undo.rs
[video-decode]: ../../crates/wallpaper/src/sources/video_decode.rs
[video-shader]: ../../crates/wallpaper/src/sources/video.wgsl
[video-source]: ../../crates/wallpaper/src/sources/video.rs
[web-source]: ../../crates/wallpaper/src/sources/web.rs
[wic]: ../../crates/wallpaper/src/sources/wic.rs
[widget-catalog]: ../../crates/widgets/src/catalog
[widget-control]: ../../crates/shell/src/widgets/control.rs
[widget-data]: ../../crates/shell/src/widgets/data.rs
[widget-edit]: ../../crates/shell/src/widgets/edit.rs
[widget-files]: ../../crates/widgets/src/files.rs
[widget-host]: ../../crates/shell/src/widgets/mod.rs
[widget-layout]: ../../crates/widgets/src/layout.rs
[widget-look]: ../../crates/widgets/src/look.rs
[widget-manifest]: ../../crates/widgets/src/manifest.rs
[widget-model]: ../../crates/shell/src/widgets/model.rs
[widget-picker]: ../../crates/widgets/src/pick.rs
[widget-plugins]: ../../crates/widgets/src/plugins.rs
[widget-registry]: ../../crates/widgets/src/registry.rs
[widget-stats]: ../../crates/widgets/src/stats.rs
[widget-storage]: ../../crates/widgets/src/storage.rs
[widget-time]: ../../crates/shell/src/widgets/time.rs
[widget-view]: ../../crates/shell/src/widgets/view.rs
[wifi-provider]: ../../crates/bars/src/flyouts/wifi.rs
[winkey]: ../../crates/bars/src/flyouts/winkey.rs
[capture-plan]: ../council/native-plan.md#losses-taken-on-purpose
[decisions]: ../council/native-plan.md#user-decisions-fixed-not-up-for-review
[html-plan]: ../council/native-plan.md#processes
[losses]: ../council/native-plan.md#losses-taken-on-purpose
[n7-spec]: studio-redesign.md
[n7-ux]: ../../docs/studio-ux.md
[n7-goldens]: ../../docs/media/studio
[n7-app]: ../../crates/studio/src/app.rs
[n7-view]: ../../crates/studio/src/view.rs
[n7-search]: ../../crates/studio/src/search.rs
[n7-settings-index]: ../../crates/studio/src/settings.rs
[n7-docs]: ../../crates/studio/src/docs.rs
[n7-watch]: ../../crates/studio/src/watch.rs
[n7-stage]: ../../crates/studio/src/stage.rs
[n7-library]: ../../crates/studio/src/library.rs
[n7-system]: ../../crates/studio/src/system.rs
[n7-home]: ../../crates/studio/src/pages/home.rs
[n7-wallpapers]: ../../crates/studio/src/pages/wallpapers.rs
[n7-look]: ../../crates/studio/src/pages/look.rs
[n7-widgets]: ../../crates/studio/src/pages/widgets.rs
[n7-bars]: ../../crates/studio/src/pages/bars.rs
[n7-settings]: ../../crates/studio/src/pages/settings.rs
[n7-config-look]: ../../crates/config/src/look.rs
[n7-styling]: ../../crates/core/src/styling.rs
[n7-shell-preview]: ../../crates/shell/src/preview.rs
[n7-glass]: ../../crates/wallpaper/src/preview.rs
[studio-plan]: ../council/native-plan.md#slices-destructive-in-dependency-order
[style-plan]: ../council/native-plan.md#styles

[flyout-look]: ../../crates/shell/src/flyouts/look.rs
[quick-panel]: ../../crates/shell/src/flyouts/quick.rs
[wifi-panel]: ../../crates/shell/src/flyouts/wifi.rs
[mixer-panel]: ../../crates/shell/src/flyouts/mixer.rs
[language-panel]: ../../crates/shell/src/flyouts/language.rs
[calendar-panel]: ../../crates/shell/src/flyouts/calendar.rs
[power-panel]: ../../crates/shell/src/flyouts/power.rs
[tray-panel]: ../../crates/shell/src/flyouts/tray.rs
[notification-center]: ../../crates/shell/src/flyouts/notifications.rs
[launcher-panel]: ../../crates/shell/src/flyouts/launcher.rs
[shell-tray-menu]: ../../crates/shell/src/bars/tray_menu.rs
