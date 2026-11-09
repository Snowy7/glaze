# Desktop package identity

For the end-user zip and per-user installer, see [docs/release.md](../../docs/release.md).
Release uninstall calls `glaze-core --restore` successfully before invoking `unregister.ps1
-WindowsRestored`; that avoids reparsing retained user config just to undo folder routing again.
The default standalone unregister command retains its existing folder-routing restore step.

This is a sparse MSIX: only its manifest and three small identity images are packaged. Core and
shell stay in the external installation folder and keep ordinary Win32 runtime behavior. Their
embedded `<msix>` entries must match `AppxManifest.xml` exactly. No service, background task,
Windows App SDK runtime, or additional steady process is installed.

Run Windows PowerShell 5.1 as the installing user. The Windows SDK's `makeappx.exe` and
`signtool.exe` are required. These scripts do not compile Rust.

```powershell
# Build and sign. The returned value is the absolute .msix path.
$package = & scripts/package/build.ps1
# Stop core/shell under the IMPLEMENTING.md desktop lock before changing registration.
& scripts/package/register.ps1 -PackagePath $package -ExternalLocation C:\path\to\install -Development
# Start core normally from that folder. A fresh process acquires identity.

# The usual deploy takes the lock, inspects process origins, copies a prebuilt release,
# registers identity, and starts core. Repeating it preserves registration and consent.
& scripts/deploy-local.ps1 -From C:\path\to\this-worktree-build\release

# Stop core/shell under the desktop lock, then remove only this user's identity.
& scripts/package/unregister.ps1
```

Registration compares the installed manifest and Windows' effective external location. Identical
registration is a no-op. `deploy-local.ps1` keeps the installed version for identical metadata and
automatically increments its revision when the manifest or external folder changes. For manual
registration changes, build a higher four-part version, e.g.
`build.ps1 -Version 0.1.0.1`, and register that MSIX. Lower/same-version changes fail before touching
the old registration. Updating binaries at the same external location needs no package update.
There is one registration per user: two worktrees cannot simultaneously own the same identity.
The package does not register Studio or the wallpaper engine, which use no identity-only API in
this slice. Native Credential Manager, WinHTTP and local pipes do not require extra capabilities.
Borderless graphics capture remains the existing unpackaged engine flow.

**Windows trust limitation:** on this installation, `Add-AppxPackage -Path <signed.msix>` fails
`0x800B0109` with current-user-only certificate trust. The Appx deployment service requires machine
trust, despite the identity guide's current-user example. Local deploy therefore uses the explicit
`-Development` path: it unpacks the signed MSIX to `%LOCALAPPDATA%\glaze\identity` and registers
that manifest with `-Register -ExternalLocation`. This requires Developer Mode to be **already
enabled**; the scripts never change the policy. The archive stays signed and the development leaf
is trusted by the current user, but Windows treats the unpacked layout as development content,
not as a production signature-enforced installation. For a production-trusted certificate, omit
`-Development` to install the signed archive directly. See Microsoft's
[trust troubleshooting](https://learn.microsoft.com/en-us/windows/msix/msix-troubleshooting-guide)
and [loose-layout registration guidance](https://learn.microsoft.com/en-us/windows/apps/dev-tools/winapp-cli/security).

## Signing

The default signer creates/reuses a non-exportable code-signing key in `CurrentUser\My`, subject
`CN=Glaze Development`, friendly name `Glaze sparse package (development only)`. It exports
only the public `.cer` into the ignored output directory and trusts that leaf in
`CurrentUser\TrustedPeople`. It never changes `LocalMachine`, `Root`, or system sideload policy,
and never writes a PFX/password. Unregistration leaves the development key/trust available for
the next deploy. To remove them deliberately, remove that exact thumbprint from those two user
stores after uninstalling packages signed by it.

For release, replace development signing with the organization's trusted code-signing certificate
or CI signing service. `build.ps1 -CertificateThumbprint <thumbprint> -TimestampUrl <RFC3161 URL>`
signs with an existing `CurrentUser\My` key without creating or trusting a development certificate.
A CI service can instead sign the generated MSIX before distribution. The certificate's subject
must match the publisher in **all three manifests** (package, core, shell); changing publisher
changes the package family and requires fresh user consent/migration. Release installers register
the signed MSIX with their actual install folder and unregister it on uninstall. Private release
keys belong in the signing service, not the repo or installer.

## Consent and snapshot mode

Studio's Notifications Enable setting makes the resident shell initiate `RequestAccessAsync` on
its UI thread. Its completion wakes the UI and reacquires consumers. Allowed/Denied is retained
by Windows; a per-family marker in `%LOCALAPPDATA%\glaze\notification-consent` also prevents
a dismissed first-run prompt from reappearing at every startup. Denial leaves the integration
unavailable and directs the user to Windows Settings > Privacy & security > Notifications.
After dismissing, explicitly grant access in Windows Settings (or remove that family's first-run
marker before enabling again). Consent never runs on the provider's MTA worker.

With no registered identity, or failed event registration, the provider logs one snapshot-mode
line, reads on open/Refresh/dismiss, and installs no timer. `notifications.liveVerified` stays an
explicit qualification switch; enable it only after the local event check passes. Counts cover
retained Windows toasts from every AUMID, including chat applications; they are not unread counts.

## Qualification

Build the integration example from the same worktree, then run this with identity already
registered and access allowed. It takes the desktop lock, embeds the shell's identity in a
temporary copy of the production-provider probe, sends/removes a test toast, measures a disabled
60-second baseline and an enabled 60-second quiet interval, then deletes its AUMID and executable.
The resident shell keeps running. Only test-app IDs/counts/timings and resource measurements are
written; no notification text is recorded.

```powershell
cargo build --release -p glaze-integrations --example probe
& scripts/package/verify-provider.ps1 -InstallDirectory C:\path\to\install `
    -ProbePath C:\path\to\this-worktree-build\release\examples\probe.exe
```

Recorded results and the real bar/center screenshots: [VERIFICATION.md](VERIFICATION.md).

## Primary sources

- [Microsoft: package identity with external location](https://learn.microsoft.com/en-us/windows/apps/desktop/modernize/grant-identity-to-nonpackaged-apps)
- [Microsoft: capability, UI-thread consent, in-memory event and background trigger](https://learn.microsoft.com/en-us/windows/apps/develop/notifications/app-notifications/notification-listener)
- [Microsoft's matching unpackaged `0x80070490` report](https://github.com/microsoft/WindowsAppSDK/issues/6172)
- [The `uap3:userNotificationListener` capability schema](https://learn.microsoft.com/en-us/uwp/schemas/appxpackage/uapmanifestschema/element-uap3-capability-manual)
