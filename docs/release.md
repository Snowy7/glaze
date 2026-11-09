# Glaze 0.1.0 installation and release packaging

Glaze ships as `Glaze-0.1.0-windows-x64.zip`. Extract the whole archive, then double-click
`install.cmd` as your ordinary Windows user. No administrator prompt, Rust, Visual Studio,
Windows SDK, repo checkout, or paid certificate is needed to run the application.
Requirements: Windows 10 2004 (build 19041) or Windows 11, x64. Wallpaper rendering uses
Direct3D 12. Web wallpapers additionally need Microsoft's Evergreen WebView2 Runtime;
it is not bundled or silently downloaded. Shader/image/video wallpapers do not use WebView2.

For software-renderer diagnostics, set `GLAZE_FORCE_WARP=1` in the environment of the
wallpaper process (including `--preview`). This requests the DX12 fallback adapter instead
of selecting a physical display GPU. Remove the variable for normal use. Missing adapters
or required features produce a logged startup/preview error; core's bounded restart policy
stops repeated failures and exits through desktop recovery. Software preview success does
not qualify hardware video decoding, WebView2, HDR or a particular physical GPU driver.

The installer verifies SHA256 hashes of the payload, copies it to
`%LOCALAPPDATA%\Programs\Glaze`, attempts sparse identity registration, sets start-at-login
using `glaze-core --autostart on`, and creates **Glaze Studio** in your Start Menu. Core uses
a per-user, least-privilege logon task, falling back to the HKCU Run value if Task Scheduler
is unavailable. Both point to the installed executable, never a build folder. An HKCU
Uninstall entry makes **Glaze 0.1.0** available in Windows Settings > Apps. Launching after
install opens core and Studio; `install.ps1 -NoLaunch` leaves them stopped.

Quit any running Glaze before installing. The installer refuses to replace an existing
installation or redirect a live development identity. To replace v0.1, uninstall it while
keeping settings, then install again. This first release has no automatic updater.
The extracted zip folder may be deleted after a successful install.

## Uninstall and recovery

Use Settings > Apps > Glaze > Uninstall, or `%LOCALAPPDATA%\Programs\Glaze\uninstall.cmd`.
The uninstaller calls **`glaze-core --restore` first**, through `Invoke-GlazeRestore` in
`release-common.ps1`. That command stops core gracefully and replays Glaze's recovery
journals. Any failure aborts removal and keeps the installation available for recovery.
Then the uninstaller disables autostart, stops only executables from this installation,
unregisters its sparse identity, removes its shortcut, and removes the application files
and HKCU Uninstall entry. It refuses to run while another Glaze installation is active.

Your `%APPDATA%\glaze` settings, themes and plugin data are **kept by default**. The
interactive uninstaller asks before deleting them; only an explicit `y` or `yes` deletes.
`uninstall.ps1 -KeepUserData` skips the question. Local caches and recovery journals under
`%LOCALAPPDATA%\glaze` remain; keeping them also preserves evidence after an interrupted
recovery. No unrelated app, Windows account, system feature or Explorer process is changed.

## Signing: what v0.1 actually promises

The executables and launcher scripts are unsigned. A downloaded unsigned executable can
show **Windows protected your PC** with **Unknown publisher** because it has no established
SmartScreen reputation. Policy may block it completely. The installer does not disable
SmartScreen, Defender, execution policy globally, or device policy. Check the release source
and the separately published zip SHA256 before choosing to run it. The CMD launchers use
PowerShell's process-only `-ExecutionPolicy Bypass` to run these readable scripts.
See [Microsoft's SmartScreen overview](https://learn.microsoft.com/en-us/windows/security/operating-system-security/virus-and-threat-protection/microsoft-defender-smartscreen/).

The sparse MSIX is signed with the development leaf, publisher **CN=Glaze Development**.
Only its public `.cer` is shipped; the signing private key is never bundled. The installer
checks that the certificate matches the MSIX signer and publisher, then imports this exact
leaf into **CurrentUser\TrustedPeople** if necessary. It never imports a root or writes the
LocalMachine certificate store. Uninstall removes the leaf only if this installation added
it and no Glaze identity remains. A failed registration removes a newly imported leaf.
The [sparse identity guide](https://learn.microsoft.com/en-us/windows/apps/desktop/modernize/grant-identity-to-nonpackaged-apps)
describes the CurrentUser trust approach.

**CurrentUser trust is not a universal solution.** On the development PC, signed archive
registration still failed `0x800B0109` with CurrentUser-only trust. If Windows Developer Mode
is already enabled, the installer retries a signed archive's unpacked manifest using
`Add-AppxPackage -Register -ExternalLocation`. It needs no SDK on the installing PC and
does not enable Developer Mode. Windows treats that as development content. On an ordinary
PC without Developer Mode, registration can fail and the installer clearly reports degraded
identity support. Full identity on every ordinary non-admin PC cannot honestly be guaranteed
with this self-signed certificate. Machine-trust provisioning requires an administrator;
this installer deliberately does not request it. Microsoft's [security guidance](https://learn.microsoft.com/en-us/windows/apps/dev-tools/winapp-cli/security)
explains loose layouts and device trust.

Glaze still runs if registration fails or `install.ps1 -Identity Skip` is chosen. Core/shell
handle absent identity; live notification-listener consent/event access is unavailable and
the notification provider falls back to on-demand snapshots where Windows permits them.
There is no background notification polling. Identity-dependent capabilities such as
Windows notification access are not release-qualified until tested on the destination PC.
Ordinary wallpaper rendering, Studio, Files and plugins do not require package identity.

For a public production release, use **Azure Trusted Signing (now Azure Artifact Signing)**
if publisher identity validation is available, or a purchased CA code-signing certificate.
Sign the EXEs/DLLs/scripts as well as the MSIX and timestamp signatures. A trusted signature
establishes the publisher; new downloads can still receive SmartScreen reputation warnings.
The certificate subject must match the publisher in the package, core and shell manifests;
changing publisher changes the package family and requires migration/renewed consent.
See [Microsoft's signing options and current eligibility](https://learn.microsoft.com/en-us/windows/msix/package/signing-package-overview).
Recommendation for this free preview: ship the unsigned portable payload with explicit
identity fallback; obtain trusted signing before promising seamless full Windows integration.

## Reproduce the artifact

From a complete x64 release build, run one command in Windows PowerShell 5.1:

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File scripts/release/build-release.ps1 -From C:\build\release
```

The packager does not compile Rust. The release build must contain all workspace binaries,
including `weather.exe`, `rss.exe` and `glaze_app_styler.dll`; build the WASM guests with
`scripts/release/build-plugins.ps1 -Target C:\build\wasm-guests` first (run it through child
Windows PowerShell `-File`). This also builds the optional TypeScript guest against the
canonical host WIT, avoiding stale generated SDK interface copies. It requires the existing
pnpm SDK dependencies. Missing payload files fail packaging. The Windows SDK's
MakeAppx/SignTool and an installed MSVC x64 CRT redistributable source are needed only on the
packaging machine. `-CrtDirectory` can supply `Microsoft.VC*.CRT` explicitly. The packager
copies those redistributable DLLs app-locally, avoiding dependence on the build PC's PATH.

Output is `dist/Glaze-0.1.0-windows-x64.zip` and `.zip.sha256` (`dist/` is gitignored).
`-CertificateThumbprint` and `-TimestampUrl` select a matching existing signing key instead
of generating/reusing the development key. This switches MSIX signing only; production
binary/script signing must happen before packaging, with matching embedded publishers.

The installer creates `config.json` containing `{}` only when it does not exist. This is
the config crate's canonical default input: Aurora, normal GPU selection, battery/fullscreen
pauses, opt-in experiments off. It never copies the developer's config and never rewrites an
existing configuration, including malformed JSON.

### Runtime asset audit

* `glaze-theme::{locate,theme_roots}` searches `themes/` beside the EXEs (then ancestors),
  and `%APPDATA%\glaze\themes` for user content. The entire bundled theme tree is copied,
  including manifests, previews, media, web JS/CSS/images, model buffers/textures and licenses.
* `glaze-theme::stack::effect_roots` searches adjacent `effects/` and the user effects folder.
  The entire shared effect tree is copied, including WGSL and manifests. Shader preludes are
  also embedded at compile time.
* `glaze-widgets::files::bundled_plugins` searches adjacent `plugins/`. For each manifest,
  the packager copies its `component` WASM and/or native `command` EXE, plus notices. Native
  plugin helpers and the optional app-styler DLL sit next to the apps. No `.cwasm` cache,
  user config, secrets, tokens, or personal integrations are distributed. WASM is compiled
  into this user's machine-local cache at install time and can retry on first use.
* Built-in widget catalogs, bar presets/icons, app icons, UI SVGs, effect/shader preludes,
  styler presets and fonts resolved through Windows are embedded or supplied by Windows;
  none requires a source checkout. Cursor/user icon packs are generated or user-installed.
* MSVC CRT DLLs are bundled. D3D12/DXGI, WIC, Media Foundation, WinHTTP, DirectWrite and the
  remaining system DLLs come from the supported Windows installation. WebView2 is the
  explicit optional runtime above. No GPU model, monitor name, screen count or DPI is
  configured in the release payload.

Run the [second-PC checklist](release-test.md) before publishing. A redirected profile on
this PC is a useful dependency check, not evidence of a clean OS, new GPU or certificate store.
