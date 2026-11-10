# Third-party notices

Glaze includes software and content made by others. Each item keeps its own licence, which this
file and the files it points to reproduce. Glaze's own code is under the MIT licence in
[LICENSE](LICENSE).

| What | Licences | Full notices |
|---|---|---|
| 313 Rust crates in the programs and bundled plugins | Permissive only: MIT, Apache-2.0, BSD, ISC, Zlib, Unicode-3.0, CC0-1.0, CDLA-Permissive-2.0 | [THIRD-PARTY-NOTICES-crates.md](THIRD-PARTY-NOTICES-crates.md) |
| Vendored code: liquid-rust (as stillkit-glass), accesskit_windows | MIT OR Apache-2.0 | [below](#vendored-code), and in the crates file |
| Lucide icons | ISC | [below](#icons-and-fonts) |
| Bundled wallpapers (`themes/`) | CC BY 4.0, CC BY 3.0, MIT, NASA media; **8 themes are CC BY-NC-SA 3.0** | [themes/CREDITS.md](themes/CREDITS.md), [themes/catalog.json](themes/catalog.json), [themes/licenses/](themes/licenses/) |
| Wallpaper effects (`effects/`) | Glaze originals; **`rain` is CC BY-NC-SA 3.0**; preview photos CC BY 3.0, CC BY 4.0, NASA | `effects/<name>/CREDITS.txt` |
| Microsoft Visual C++ runtime DLLs, if included | Microsoft's Visual Studio Distributable Code terms | [below](#microsoft-visual-c-runtime) |

## Rust crates

[THIRD-PARTY-NOTICES-crates.md](THIRD-PARTY-NOTICES-crates.md) lists every crate compiled into the
shipped programs (glaze-core, -wallpaper, -shell, -studio, -files, -index, -plugins, -native-host,
-styler and `glaze_app_styler.dll`, and the `weather` and `rss` plugin commands) and into the bundled
WebAssembly plugins (Discord, Docker, Grok, OBS, T3 Code, Tailscale, Weather). For each crate it gives
the version and licence, followed by the full licence texts. It was generated with cargo-about 0.9.3
from the lockfiles; build-only and test-only dependencies are left out.

Every crate is under a permissive licence. None is GPL, LGPL, AGPL or MPL. Notable ones:

- **wasmtime** and the Cranelift crates (plugin runtime): Apache-2.0 WITH LLVM-exception.
- **wgpu** and **naga** (the engine): MIT OR Apache-2.0.
- **ring** (TLS): Apache-2.0 AND ISC. **rustls**: Apache-2.0 OR ISC OR MIT.
- **webpki-roots** (Mozilla's CA list as data): CDLA-Permissive-2.0.
- **unicode-ident** and the ICU4X crates (`icu_*`, `zerovec` and related): Unicode-3.0. **encoding_rs**: (Apache-2.0 OR MIT) AND BSD-3-Clause.
- **notify** (file watching): CC0-1.0.
- **stillkit**, Glaze's UI library ([Snowy7/stillkit](https://github.com/Snowy7/stillkit)), is a
  separate public project under MIT.

## Vendored code

- **liquid-rust** by al1h3n ([upstream](https://github.com/al1h3n/liquid-rust), crate version
  0.2.5): MIT OR Apache-2.0. It is carried in stillkit as `stillkit-glass` with both licence texts
  (`LICENSE-MIT`, `LICENSE-APACHE`) and a list of local changes (`UPSTREAM-CHANGES.md`). Changes
  are marked in the source. It draws the Liquid Glass. Its shader uses Inigo Quilez's quadratic smooth
  minimum, as upstream does.
- **accesskit_windows** 0.35 by the AccessKit authors ([upstream](https://github.com/AccessKit/accesskit)):
  MIT OR Apache-2.0. It is vendored in stillkit at `crates/stillkit/vendor/accesskit_windows`, with
  both licence texts and the upstream README kept. It provides UI Automation (screen-reader) support.

Both licence texts for each are reproduced in the crates file.

## Icons and fonts

- **Lucide** icons ([lucide.dev](https://lucide.dev)) are built into the UI library. Licence:

  ```text
  ISC License

  Copyright (c) for portions of Lucide are held by Cole Bemis 2013-2022 as part of Feather (MIT). All other copyright (c) for Lucide are held by Lucide Contributors 2022.

  Permission to use, copy, modify, and/or distribute this software for any
  purpose with or without fee is hereby granted, provided that the above
  copyright notice and this permission notice appear in all copies.

  THE SOFTWARE IS PROVIDED "AS IS" AND THE AUTHOR DISCLAIMS ALL WARRANTIES
  WITH REGARD TO THIS SOFTWARE INCLUDING ALL IMPLIED WARRANTIES OF
  MERCHANTABILITY AND FITNESS. IN NO EVENT SHALL THE AUTHOR BE LIABLE FOR
  ANY SPECIAL, DIRECT, INDIRECT, OR CONSEQUENTIAL DAMAGES OR ANY DAMAGES
  WHATSOEVER RESULTING FROM LOSS OF USE, DATA OR PROFITS, WHETHER IN AN
  ACTION OF CONTRACT, NEGLIGENCE OR OTHER TORTIOUS ACTION, ARISING OUT OF
  OR IN CONNECTION WITH THE USE OR PERFORMANCE OF THIS SOFTWARE.
  ```

- **Segoe UI** (including Segoe UI Variable), **Segoe Fluent Icons** and **Segoe MDL2 Assets** are
  Windows system fonts. Glaze uses the copies installed with Windows and does not redistribute them.
- The bar and tray glyphs in `crates/bars/assets/icons` and the Glaze logo in `assets/logo` are
  original Glaze artwork.

## Wallpapers

[themes/CREDITS.md](themes/CREDITS.md) records the author, licence, source and changes for every
bundled file, and [themes/catalog.json](themes/catalog.json) holds the same data in machine-readable
form. Studio reads it to show each wallpaper's author, licence and source, in the wallpaper's
details and on its **About** page. Upstream licence texts are in [themes/licenses/](themes/licenses/).
Keep all three with any copy of the themes.

| Licence | Themes |
|---|---|
| CC BY 4.0 | charge-city, cozy-fireplace, spring-forest (Blender Foundation); milky-way (ESO/C. Malin) |
| MIT | fluids (Pavel Dobryakov); star-nest (Pablo Román Andrioli); vanta-waves (Teng Bao, three.js authors) |
| **CC BY-NC-SA 3.0** | rain-city, rain-city-amber, rain-city-midnight (Dani John; Martijn Steinrucken); seascape, seascape-midnight (Alexander Alekseev); snow-city, snow-forest, snowfall (Andrew Baldwin; Dani John) |

The rain and snow themes use separately credited backgrounds: a Hong Kong photograph by David Iliff
(CC BY 3.0) and a frame from Blender's *Spring* (CC BY 4.0).

**Non-commercial.** The eight CC BY-NC-SA themes may be shared freely, with credit and under the
same licence, but not sold or included in a commercial distribution. Their names say
"non-commercial", and their catalog entries have `noncommercial: true`.

The hidden examples in `themes/examples/` use NASA media (not copyrighted in the US; no NASA
endorsement implied), Khronos glTF sample models (Fox: model CC0 1.0, rig and animation CC BY 4.0;
SimpleSkin: CC0 1.0, with credits in each folder's `UPSTREAM.md`), and original Glaze content.

## Effects

The shaders in `effects/` are original Glaze work, except **`rain`**. Its drop field and refraction
are adapted from "Heartfelt" by Martijn Steinrucken (BigWings), CC BY-NC-SA 3.0, so the rain effect is
non-commercial too. Each effect's preview image is a crop of a photo credited in its `CREDITS.txt`
(David Iliff, CC BY 3.0; Blender Foundation's *Spring*, CC BY 4.0; NASA).

## Plugins and online services

The bundled plugins (Discord, Docker, Grok, OBS, T3 Code, Tailscale, Weather, RSS) are Glaze code;
their Rust dependencies are in the crates file. Discord, Docker, OBS, Tailscale, Grok and T3 Code are
names and trademarks of their owners. Glaze is not affiliated with or endorsed by them.

- **Weather data** comes from [Open-Meteo.com](https://open-meteo.com), which publishes it under
  CC BY 4.0. Place search uses the Open-Meteo geocoding API.
- **Crypto prices** come from the [CoinGecko](https://www.coingecko.com) API.

## Microsoft Visual C++ runtime

Release packages may include Microsoft Visual C++ runtime DLLs (`vcruntime140.dll`,
`vcruntime140_1.dll`, `msvcp140*.dll` and others from `Microsoft.VC145.CRT`) next to the programs
and the native plugin commands. They are © Microsoft Corporation and are not covered by any licence
Glaze grants. They are redistributed unmodified, under the Distributable Code terms of the Microsoft
Visual Studio 2026 licence, as listed in Microsoft's
[Visual Studio redistribution list](https://learn.microsoft.com/visualstudio/releases/2026/redistribution).

## Other runtime components

These parts of Windows are used where they are installed and are not redistributed:

- the Microsoft Edge **WebView2 Runtime**, for `web` wallpapers;
- **Direct2D**, **DirectWrite**, **Direct3D 12**, **Media Foundation** and **WIC**.
