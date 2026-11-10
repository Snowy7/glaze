# Glaze v0.1.0

Glaze is a native Windows desktop customizer written in Rust. It draws live wallpapers (shaders, images, slideshows, video, glTF scenes, web pages and external renderers) on a wgpu engine, puts Liquid Glass under a native top bar, dock, widgets, flyouts and menus, and can restyle Windows' own frames, accent and light or dark mode. Every change it makes to Windows is journaled and undone on quit, on the panic hotkey and after a crash.

v0.0.1 was a prototype whose bars, widgets and Studio ran on WebView2. v0.1.0 rebuilds them natively: the shell and Studio draw with software Direct2D through [stillkit](https://github.com/Snowy7/stillkit), a UI library extracted from this repo, only the wallpaper engine holds a hardware GPU device, and WebView2 remains only for `web` wallpapers. Glaze was formerly called mypaper; to move an existing local install, stop it and run `scripts/migrate-local.ps1` as described in the README.

This is a pre-release, tested on one PC so far.

## Install

Download `Glaze-0.1.0-windows-x64.zip` below, extract all of it, and double-click `install.cmd` (not as administrator). It installs per user to `%LOCALAPPDATA%\Programs\Glaze`, with no admin rights. Uninstall from Settings > Apps; it first undoes every change Glaze made to Windows. The executables and the package identity are not signed by a trusted certificate, so Windows SmartScreen may warn; check the SHA256 below first. Details are in [docs/release.md](https://github.com/Snowy7/glaze/blob/v0.1.0/docs/release.md).

## Highlights

- A native top bar, dock, flyouts, launcher and desktop menu replace the taskbar; on the development machine the shell idled at 32 MB with bars, dock and six widgets.
- Liquid Glass on every surface, drawn by the engine with stillkit-glass (stillkit's copy of liquid-rust), in five styles: Liquid, One UI, Fluent, Solid and Mono.
- A wallpaper editor in Studio stacks 38 effects, images and 3D models over any base, with a shader editor that reloads live.
- The dock groups windows by app and shows live previews and Windows jump lists.
- 17 native widgets, plus WebAssembly plugins with a permission review at install; T3 Code, Discord, Docker, OBS, Tailscale and Grok Bot ship as plugins.
- Glaze Files, an on-demand native file browser with tabs, previews and Windows' own file operations.
- One style switch also styles Windows' frames, corners, accent and light or dark mode, and `glaze-core --restore` undoes every change Glaze made.
- Covered screens release the engine's drawing resources, and games pause background work.
- A redesigned Studio with a first-run setup, editing on the real desktop, and Ctrl+K search.
- Experimental and off by default, with switches in Studio > Settings > General: a native file index (`experimental.file_index`), Files as the folder handler (`experimental.files_default`) and the App styler (`experimental.app_styler`).

The full list of changes is in [CHANGELOG.md](https://github.com/Snowy7/glaze/blob/v0.1.0/CHANGELOG.md).

Some bundled wallpapers (the rain, snow and Seascape selections) and the Rain on glass effect are CC BY-NC-SA 3.0, for non-commercial use only; see [themes/CREDITS.md](https://github.com/Snowy7/glaze/blob/v0.1.0/themes/CREDITS.md). Third-party licences are in [THIRD-PARTY-NOTICES.md](https://github.com/Snowy7/glaze/blob/v0.1.0/THIRD-PARTY-NOTICES.md).

**Known issues:** see the [Known issues](https://github.com/Snowy7/glaze/blob/v0.1.0/CHANGELOG.md#known-issues) section of the changelog.
