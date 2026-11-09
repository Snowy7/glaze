# Glaze 0.1.0: ten-minute second-PC check

Use a second x64 Windows 11 PC or an existing VM as an ordinary user. Do not install Rust,
Visual Studio, or the Windows SDK. Keep Developer Mode off for the ordinary-user pass.
Have the release zip and its independently published SHA256. This checklist makes real
desktop changes; save your work first. Record Windows build, GPU/driver, screen count,
DPI/scaling and whether WebView2 is installed. Do not record private notification content.

1. **0–1 min — Download and trust.** Verify `Get-FileHash .\Glaze-0.1.0-windows-x64.zip`.
   Extract the whole zip. Record any SmartScreen/Smart App Control warning and publisher;
   do not disable protection or policies. Run `install.cmd` without elevation.
2. **1–2 min — Installation.** Confirm `%LOCALAPPDATA%\Programs\Glaze` contains apps,
   CRT DLLs, `themes`, `effects`, `plugins`, identity, docs and uninstaller. Confirm Glaze
   0.1.0 in Settings > Apps and **Glaze Studio** in Start. Record the identity warning,
   `Get-AppxPackage glaze.Desktop`, and `Cert:\CurrentUser\TrustedPeople` leaf added by
   the install. A failed self-signed identity should leave Glaze running with live
   notification access unavailable; it must not request admin or enable Developer Mode.
3. **2–4 min — First run.** Confirm onboarding and sensible defaults with no personal
   configuration bundled. Choose a starter, open Studio, check About says 0.1.0. Reopen
   Studio from Start. Verify `%APPDATA%\glaze\config.json` parses, the selected theme
   resolves inside the installed content/user theme folder, and settings persist after
   closing/reopening Studio. Check logs in `%LOCALAPPDATA%\glaze` for errors/crash loops.
4. **4–5 min — Content and helpers.** Open Files on a new disposable folder. Make one
   text file there and preview it. Add the Hello demo plugin, then a built-in widget.
   Missing Discord/OBS/Docker/Tailscale/T3 must show disconnected/empty states. In Studio,
   preview a shader wallpaper and a stack with an effect; check previews are nonblack.
   Try one bundled video. Test a web theme if WebView2 is present; otherwise record the
   explicit unsupported-runtime behavior. Enable the experimental index only deliberately,
   search the disposable filename, then turn it off.
5. **5–6 min — Display and degraded behavior.** Try the available second display or a
   different scaling value, then restore your original setting. Confirm placement and
   wallpaper sizing. If testing software rendering, set `GLAZE_FORCE_WARP=1`, render a
   preview, and record the adapter/log/result. Remove the variable afterward.
   On a machine without required DX12 capabilities, expect a useful failure/fallback and
   a restored Windows desktop, never a persistent restart loop or hidden taskbar.
6. **6–7 min — Startup and single instance.** Run installed `glaze-core.exe --autostart status`;
   its command must point inside Programs\Glaze. Launch core twice and confirm only one
   shell/engine survives. Sign out/in only if convenient; otherwise mark login untested
   and inspect the per-user Glaze task/HKCU Run value. Do not infer login success from a
   registry entry alone.
7. **7–9 min — Recovery and uninstall.** With Glaze running, run installed
   `glaze-core.exe --restore`; confirm your previous taskbar, wallpaper and managed
   Windows preferences return. Start once more, then uninstall using Settings > Apps.
   Choose the default to retain user data. Confirm the application directory, shortcut,
   startup entry and this install's identity are gone, and `%APPDATA%\glaze` remains.
   Other apps, Explorer and your work must remain running.
8. **9–10 min — Reinstall and optional deletion.** Install again and confirm settings
   survive. Uninstall and explicitly answer `y` to remove Glaze data; confirm only
   `%APPDATA%\glaze` is deleted. Local recovery/cache files are intentionally kept.

Report PASS/FAIL/NOT TESTED per step, exit codes, identity registration HRESULT, adapter
and preview dimensions. Repeat the identity pass on an already Developer-Mode-enabled
machine if development registration needs qualification. A public trusted-signing build
needs its own ordinary-user/no-Developer-Mode install test.

The automated `scripts/release/test-clean-profile.ps1` check uses an extracted payload with
fresh APPDATA/LOCALAPPDATA and a Windows-only PATH. It intentionally skips real identity,
startup and Apps integration, does not launch a second desktop core/shell, and cannot
replace this second-PC checklist.
