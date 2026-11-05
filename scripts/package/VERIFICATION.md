# Notification package identity — 2026-11-05

Live notification delivery is verified on this Windows desktop. The in-process
`UserNotificationListener.NotificationChanged` event works with sparse package identity while
another application is foreground. A background trigger and another steady process are unnecessary
on this installation. Counts represent retained Windows toasts, not unread messages.

## Cause and registration

The same registration probe, using the real Windows API and MTA initialization, produces:

| Context | `GetAccessStatus` | `NotificationChanged` |
|---|---|---|
| Unpackaged | Allowed (1) | `0x80070490`, Element not found |
| `mypaper.Desktop` identity | Allowed (1) | Registered successfully |

Raw results: [unpackaged](evidence/unpackaged-registration.jsonl),
[packaged](evidence/packaged-registration.jsonl).

Both actual exes were inspected through `GetPackageFullName`: core and shell had
`mypaper.Desktop_0.1.0.0_neutral__21v47vsar82ra`. The engine remained unpackaged (`15700`,
`APPMODEL_ERROR_NO_PACKAGE`), retaining its existing borderless-capture flow. The external location
is this worktree's `target\release`, populated from its own build at
`C:\mypaper-targets\notification-identity\release`.

Windows already reported notification access Allowed, including immediately after the first
package registration. No consent prompt or privacy-setting change was needed. The newly added
first-run UI-thread request path and Denied branch compile, but a fresh-user consent dialog and
actual denial were **not** exercised: existing permission was preserved rather than revoked.

### Development signing limitation

The MSIX is signed with the non-exportable development key in `CurrentUser\My`, with its public
certificate in `CurrentUser\TrustedPeople`. Direct signed-archive installation failed
`0x80073CF0` / `0x800B0109`: Appx deployment requires machine trust on this machine. No machine or
Root trust was installed. Local deploy therefore uses explicit `-Development` registration of the
signed MSIX's unpacked manifest with `-ExternalLocation`, using **already-enabled** Developer Mode.
This is development-content registration, not a production signature-enforced installation.
Release signing/installation remains untested without a production-trusted certificate. See the
[signing instructions](README.md#signing).

## Measured notification gates

All CPU numbers are percent of one core; memory is private MiB. Each provider sample has its own
60-second disabled baseline, a finite notification add/remove watch, a 2-second settling grace,
and 60 seconds enabled/quiet. The probe uses the production `Hub`, `Lease` and notification provider.
It is a temporary exe with the shell's embedded identity, outside the final steady process set.

| Measurement | Measured | Gate | Result |
|---|---:|---:|---|
| Event → retained test-app count, sample 1 | add 92.363 ms; remove 73.070 ms | ≤1,000 ms | Pass |
| Event → retained test-app count, sample 2 | add 92.114 ms; remove 68.840 ms | ≤1,000 ms | Pass |
| Real bar and center card captured after addition | ≤475 ms | ≤1,000 ms | Pass |
| Real bar and center card captured after removal | ≤451 ms | ≤1,000 ms | Pass |
| Notification provider idle CPU, two 60-second samples | 0.00%, 0.00% | 0% | Pass |
| Idle callbacks / snapshot changes / reconnect attempts | 0 / 0 / 0 in both samples | 0 | Pass |
| Added provider private memory | 1.625, 1.648 MiB | ≤5 MiB | Pass |
| New steady helper processes | 0 | 0 | Pass |

Disabled CPU was 0.00% and 0.02604%; these are individual samples, not a median-of-three fixture.
The first wrapper incorrectly treated PowerShell's missing exited-process `ExitCode` as a failure,
despite complete successful data. Retaining the process handle fixed the wrapper; the second full
run passed, including its process-exit check. Both raw samples are retained:
[sample 1](evidence/provider-live-1.jsonl), [sample 2](evidence/provider-live.jsonl),
[latency 1](evidence/event-latency-1.json), [latency 2](evidence/event-latency.json).

For the real UI check, UI Automation invoked the existing notification button on the left monitor;
no widget file/config was changed. PowerShell registered a unique temporary AUMID, sent one silent
test toast, and removed it through Windows' toast history API. The actual bar changed **5 → 6 → 5**;
the notification center added and removed the generic test-app card. Windows' foreground window
belonged to another application throughout. UI Automation also confirmed the test-app card.
The capture bounds include the changed badge and card; the addition image crops the panel's lower
calendar after its height grows. The ≤475/451 ms figures are upper bounds at capture completion,
not estimates of first paint. [UI result](evidence/ui-result.json).

Visually inspected:

- [Badge before](evidence/bar-before.png), [added](evidence/bar-added.png),
  [removed](evidence/bar-removed.png).
- [Center with test card](evidence/center-added.png), [after removal](evidence/center-removed.png).

### Full desktop sample — not full-product qualification

The required `mypaper-tools\measure.ps1 -Label notification-identity -Seconds 60` was also run.
It measured the current user layout, active media and rain-city, not the plan's paused-media,
six-widget, median-of-three comparison. Its real values must not be confused with the isolated
notification provider's zero idle CPU:

| Current desktop, single 60-second sample | Measured | Native-plan gate | Result |
|---|---:|---:|---|
| Steady mypaper processes | 3 | 3 | Pass |
| Core CPU / private memory | 0.00% / 2 MiB | ~0% / ≤8 MiB | Pass in this sample |
| Shell CPU / private memory | 3.26% / 31 MiB | ≤0.1% / ≤40 MiB | CPU fails; memory passes |
| Engine CPU / private memory | 4.30% / 311 MiB | ≤4% / ≤272.4 MiB | Fails |
| Whole product private memory | 344 MiB | ≤320.4 MiB | Fails |

These are unresolved full-product gates, not attributed notification costs; no disabled/full-shell
comparison was taken. Widget/engine optimization is outside this slice's ownership. No GPU-ms or
DWM-baseline claim is made. Raw full-desktop metadata is in `evidence/full-desktop.json`.

## Unregistration, fallback and cleanup

- Unregistered the real package twice: second call was a no-op.
- Started the actual core/shell without registration: both ran without identity, the bar remained
  usable, and exactly **one** `notifications: snapshot mode` line was logged. The production
  provider reported `Cached` with `liveVerified=true`, no callbacks, and no polling.
  [Unregistration evidence](evidence/unregistration.json),
  [snapshot probe](evidence/unpackaged-fallback.jsonl) (a short functional check, not an idle gate).
- Re-registered, repeated registration successfully as a no-op, and restored this worktree's
  packaged core/shell. Package/exe identity and current external location were checked.
- Removed the temporary AUMID registrations, test toasts and copied probe exes. No test shortcut,
  task, background registration, service, machine certificate, or helper process remains.
- Every desktop start/restart, toast/UI check and sample ran under the prescribed directory lock.
  The origin of running processes was inspected before restarts; lock cleanup uses `finally`.

## Checks and handoff

- `cargo check -p mypaper-integrations -p mypaper-shell -p mypaper-core`: passed.
- `cargo test -p mypaper-integrations`: **10 passed**.
- `cargo test -p mypaper-shell`: **45 passed, 1 existing ignored** (user widget-layout fixture).
- Strict Clippy passed for integrations (including all targets), shell and core.
- Windows SDK MakeAppx/SignTool, PowerShell parsing, actual registration/no-op/unregistration,
  packaged/unpackaged API probes, live UI and provider latency/idle checks passed as described above.

Only source edit outside assigned ownership: `crates/shell/src/integrations/mod.rs`, to initiate
consent from Studio's enabled-setting transition on the shell UI thread and wake/reacquire leases
on completion. `crates/shell/src/widgets/**` was not edited.

Public API added:

```rust
notifications::request_access_once(completed: impl Fn() + Send + Sync + 'static)
    -> anyhow::Result<()>;
```

Existing explicit-consent API is unchanged:

```rust
notifications::request_access()
    -> windows::core::Result<windows_future::IAsyncOperation<UserNotificationListenerAccessStatus>>;
```

Completion uses the existing UI wake (`WM_APP + 146`) and integration generation invalidation.
No new IPC message or configuration schema was added. The existing
`notifications::Snapshot { cards: Vec<Card>, counts: BTreeMap<String, usize> }`,
`Data::Notifications`, and `Availability::{Live, Cached, Unavailable}` remain the consumer contract.
The local `notifications.enabled` and qualified `liveVerified` settings are left true; text remains
opted out. Release signing and fresh-user Allow/Denied UI are the remaining deployment-specific
checks; native-plan full-product performance gates remain separate and unresolved.

Primary sources: Microsoft's [listener capability/consent/event guide](https://learn.microsoft.com/en-us/windows/apps/develop/notifications/app-notifications/notification-listener),
[matching unpackaged registration report](https://github.com/microsoft/WindowsAppSDK/issues/6172),
[external-location identity guide](https://learn.microsoft.com/en-us/windows/apps/desktop/modernize/grant-identity-to-nonpackaged-apps),
[machine-trust limitation](https://learn.microsoft.com/en-us/windows/msix/msix-troubleshooting-guide),
and [development-layout registration](https://learn.microsoft.com/en-us/windows/apps/dev-tools/winapp-cli/security).
