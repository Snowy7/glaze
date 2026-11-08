# Bundled wallpaper credits and licenses

Verified 4 November 2026. **Fifteen live themes** occupy the curated gallery; there are no standalone still-image wallpapers in this set. Earlier themes and developer demos live in `examples/` and retain their selected IDs. Nothing in this file grants extra rights to Steam Workshop or gallery uploads. The research and exclusions are documented in [wallpaper-sources.md](../docs/research/wallpaper-sources.md).

**Commercial distribution:** the **eight** rain, snow and Seascape selections (including variants) form a separate **NON-COMMERCIAL** group. Their effects and adaptations are **CC BY-NC-SA 3.0**. Do not sell or commercially distribute the complete bundle without removing that group or obtaining separate permission. Each name includes “non-commercial”; each catalog record has `noncommercial: true`. A free download is not automatically a non-commercial use. [CC BY-NC-SA](https://creativecommons.org/licenses/by-nc-sa/3.0/), [CC FAQ](https://creativecommons.org/faq/).

**ShareAlike:** the NC shader/web adaptations keep **CC BY-NC-SA 3.0**. Their separately credited CC BY backgrounds retain those grants. This covers the art and adaptations, not the independent Glaze engine. KDE, GNOME and elementary stills were researched but removed from the curated gallery after feedback; they are not shipped as new selections. Keep this credits file, [catalog.json](catalog.json), license links and [vendor notices](licenses/) with distributions; do not add DRM or incompatible restrictions to CC content. [NC-SA 3.0](https://creativecommons.org/licenses/by-nc-sa/3.0/).

**NASA / observatories:** the preserved legacy NASA media is generally not copyrighted in the US, with item-specific exceptions; no NASA endorsement is implied. The curated ESO sky video is **CC BY 4.0**, not public domain. Its complete credit is burned into the footage above the taskbar area, and `fit=contain` preserves it across aspect ratios. Logos, third-party music and unrelated site content are not included. [NASA usage](https://www.nasa.gov/nasa-brand-center/images-and-media/), [ESO conditions](https://www.eso.org/public/outreach/copyright/).

New video is **H.264, yuv420p, muted, native 24/25 fps, at most 1920×1080**, CRF 18 with a 6 Mb/s VBV maxrate. Frame-aligned, rotated, eased tail-to-head overlaps have exact discrete endpoint weights; the head leads back into the following source frame without reversing motion. Cinema-ratio sources keep their aspect ratio. Previews are 640×360 JPEGs. [catalog.json](catalog.json) records sizes/hashes, changes, loop recipes and measured frame/motion results; [verify-loops.py](verify-loops.py) reproduces the checks. No full original movies or downloaded executables are committed.

## Curated library

| ID / title | Kind / category | Author | License | Source | Theme bytes |
|---|---|---|---|---|---:|
| `charge-city` — Charge — Rainy Rooftops | video / City & rain | Blender Foundation; director Hjalti Hjálmarsson | [CC-BY-4.0](https://creativecommons.org/licenses/by/4.0/) | [Source](https://studio.blender.org/films/charge/) | 703,712 |
| `cozy-fireplace` — Charge — Cozy Fireplace | video / Cozy | Blender Foundation / Blender Studio | [CC-BY-4.0](https://creativecommons.org/licenses/by/4.0/) | [Source](https://video.blender.org/w/a5e0b25f-219b-4069-a247-fa2b29cb39be) | 7,026,143 |
| `fluids` — Fluids | web / Abstract | Pavel Dobryakov | [MIT](https://opensource.org/license/mit/) | [Source](https://github.com/PavelDoGreat/WebGL-Fluid-Simulation) | 83,499 |
| `milky-way` — Milky Way over Paranal | video / Space | ESO/C. Malin (christophmalin.com) | [CC-BY-4.0](https://creativecommons.org/licenses/by/4.0/) | [Source](https://www.eso.org/public/videos/uhd-cm-vlt2014-3/) | 11,524,113 |
| `rain-city` — Rainy Hong Kong (non-commercial) | shader / City & rain | Dani John (Rocksdanister); Martijn Steinrucken (BigWings) | [CC-BY-NC-SA-3.0](https://creativecommons.org/licenses/by-nc-sa/3.0/) | [Source](https://github.com/rocksdanister/rain) | 939,506 |
| `rain-city-amber` — Amber Downpour (non-commercial) | shader / City & rain | Dani John (Rocksdanister); Martijn Steinrucken (BigWings) | [CC-BY-NC-SA-3.0](https://creativecommons.org/licenses/by-nc-sa/3.0/) | [Source](https://github.com/rocksdanister/rain) | 944,527 |
| `rain-city-midnight` — Midnight Drizzle (non-commercial) | shader / City & rain | Dani John (Rocksdanister); Martijn Steinrucken (BigWings) | [CC-BY-NC-SA-3.0](https://creativecommons.org/licenses/by-nc-sa/3.0/) | [Source](https://github.com/rocksdanister/rain) | 933,746 |
| `seascape` — Seascape (non-commercial) | shader / Nature | Alexander Alekseev (TDM) | [CC-BY-NC-SA-3.0](https://creativecommons.org/licenses/by-nc-sa/3.0/) | [Source](https://github.com/tdmaav/shadertoy/blob/master/Seascape.shader) | 87,095 |
| `seascape-midnight` — Seascape — Midnight (non-commercial) | shader / Nature | Alexander Alekseev (TDM) | [CC-BY-NC-SA-3.0](https://creativecommons.org/licenses/by-nc-sa/3.0/) | [Source](https://github.com/tdmaav/shadertoy/blob/master/Seascape.shader) | 41,509 |
| `snow-city` — Snow over Hong Kong (non-commercial) | shader / City & rain | Andrew Baldwin; Lively adaptation by Dani John | [CC-BY-NC-SA-3.0](https://creativecommons.org/licenses/by-nc-sa/3.0/) | [Source](https://github.com/rocksdanister/snow) | 919,438 |
| `snow-forest` — Snow in the Misty Forest (non-commercial) | shader / Nature | Andrew Baldwin; Lively adaptation by Dani John | [CC-BY-NC-SA-3.0](https://creativecommons.org/licenses/by-nc-sa/3.0/) | [Source](https://github.com/rocksdanister/snow) | 89,820 |
| `snowfall` — Snowfall (non-commercial) | shader / Nature | Andrew Baldwin; Lively adaptation by Dani John | [CC-BY-NC-SA-3.0](https://creativecommons.org/licenses/by-nc-sa/3.0/) | [Source](https://github.com/rocksdanister/snow/blob/main/shaders/snow.frag) | 38,598 |
| `spring-forest` — Spring — Misty Forest | video / Nature | Blender Foundation; director Andy Goralczyk | [CC-BY-4.0](https://creativecommons.org/licenses/by/4.0/) | [Source](https://studio.blender.org/films/spring/) | 689,813 |
| `star-nest` — Star Nest | shader / Space | Pablo Román Andrioli (Kali) | [MIT](https://opensource.org/license/mit/) | [Source](https://defold.com/tutorials/shadertoy/#the-star-nest-shader) | 37,381 |
| `vanta-waves` — Waves | web / Nature | Teng Bao and Vanta contributors | [MIT](https://opensource.org/license/mit/) | [Source](https://github.com/tengbao/vanta) | 653,166 |

**New binary data:** 23,947,981 bytes (22.84 MiB), including the newly added interactive-example preview. **Curated binaries:** 23,945,606 bytes. **All curated files:** 28,053,809 bytes. Existing media moved into `examples/` is not new binary data.


## Redistributable commercial-compatible content


### Charge — Rainy Rooftops

A quiet, muted excerpt prepared as a repeating desktop loop.

| File | Author | License | Original source | Changes |
|---|---|---|---|---|
| `charge-city/loop.mp4` | Blender Foundation; director Hjalti Hjálmarsson | [CC-BY-4.0](https://creativecommons.org/licenses/by/4.0/) | [Source](https://studio.blender.org/films/charge/) | Source 25.5–28.75 s; frame-aligned, rotated 0.75 s smootherstep (quintic) tail-to-head crossfade; native 24 fps; H.264/yuv420p CRF 18, 6 Mb/s VBV, audio removed, width capped at 1920. |
| `charge-city/preview.jpg` | Blender Foundation; director Hjalti Hjálmarsson | [CC-BY-4.0](https://creativecommons.org/licenses/by/4.0/) | [Source](https://studio.blender.org/films/charge/) | 640×360 JPEG preview, center crop of the wallpaper. |
| `charge-city/theme.json` | Glaze contributors | [MIT](https://opensource.org/license/mit/) | [Source](../LICENSE) | Glaze manifest; no external code or media. |

### Charge — Cozy Fireplace

A quiet, muted excerpt prepared as a repeating desktop loop.

| File | Author | License | Original source | Changes |
|---|---|---|---|---|
| `cozy-fireplace/loop.mp4` | Blender Foundation / Blender Studio | [CC-BY-4.0](https://creativecommons.org/licenses/by/4.0/) | [Source](https://video.blender.org/w/a5e0b25f-219b-4069-a247-fa2b29cb39be) | Source 30–55 s; frame-aligned, rotated 2 s smootherstep (quintic) tail-to-head crossfade; native 24 fps; H.264/yuv420p CRF 18, 6 Mb/s VBV, audio removed, width capped at 1920. |
| `cozy-fireplace/preview.jpg` | Blender Foundation / Blender Studio | [CC-BY-4.0](https://creativecommons.org/licenses/by/4.0/) | [Source](https://video.blender.org/w/a5e0b25f-219b-4069-a247-fa2b29cb39be) | 640×360 JPEG preview, center crop of the wallpaper. |
| `cozy-fireplace/theme.json` | Glaze contributors | [MIT](https://opensource.org/license/mit/) | [Source](../LICENSE) | Glaze manifest; no external code or media. |

### Fluids

The widely used WebGL fluid simulation: luminous ink, with gentle automatic splats.

| File | Author | License | Original source | Changes |
|---|---|---|---|---|
| `fluids/LDR_LLL1_0.png` | Pavel Dobryakov | [MIT](https://opensource.org/license/mit/) | [Source](https://github.com/PavelDoGreat/WebGL-Fluid-Simulation) | Unmodified. |
| `fluids/index.html` | Glaze contributors | [MIT](https://opensource.org/license/mit/) | [Source](../LICENSE) | Local wallpaper host, canvas and visible credit; no remote dependencies. |
| `fluids/preview.jpg` | Pavel Dobryakov | [MIT](https://opensource.org/license/mit/) | [Source](https://github.com/PavelDoGreat/WebGL-Fluid-Simulation) | 640×360 JPEG from an actual DX12 render / local WebView2 desktop capture; taskbar excluded. |
| `fluids/script.js` | Pavel Dobryakov | [MIT](https://opensource.org/license/mit/) | [Source](https://github.com/PavelDoGreat/WebGL-Fluid-Simulation) | Removed mobile promotion handlers and GUI startup; dye resolution 512; 30 fps pacing with matching dt cap; original splat routine gets two splats every 2.2 seconds. Removed all Google Analytics calls (including the startup dependency on a global ga function); the local page never loads analytics. |
| `fluids/theme.json` | Glaze contributors | [MIT](https://opensource.org/license/mit/) | [Source](../LICENSE) | Glaze manifest; no external code or media. |

### Milky Way over Paranal

A quiet, muted excerpt prepared as a repeating desktop loop.

| File | Author | License | Original source | Changes |
|---|---|---|---|---|
| `milky-way/loop.mp4` | ESO/C. Malin (christophmalin.com) | [CC-BY-4.0](https://creativecommons.org/licenses/by/4.0/) | [Source](https://www.eso.org/public/videos/uhd-cm-vlt2014-3/) | Source 0–18 s; frame-aligned, rotated 2 s smootherstep (quintic) tail-to-head crossfade; native 25 fps; H.264/yuv420p CRF 18, 6 Mb/s VBV, audio removed, width capped at 1920. Full ESO credit burned in; contain preserves it. |
| `milky-way/preview.jpg` | ESO/C. Malin (christophmalin.com) | [CC-BY-4.0](https://creativecommons.org/licenses/by/4.0/) | [Source](https://www.eso.org/public/videos/uhd-cm-vlt2014-3/) | 640×360 JPEG preview, center crop of the wallpaper. |
| `milky-way/theme.json` | Glaze contributors | [MIT](https://opensource.org/license/mit/) | [Source](../LICENSE) | Glaze manifest; no external code or media. |

### Spring — Misty Forest

A quiet, muted excerpt prepared as a repeating desktop loop.

Film end card at ~458 seconds explicitly states CC BY 4.0; overrides the inconsistent ShareAlike label on PeerTube metadata.

| File | Author | License | Original source | Changes |
|---|---|---|---|---|
| `spring-forest/loop.mp4` | Blender Foundation; director Andy Goralczyk | [CC-BY-4.0](https://creativecommons.org/licenses/by/4.0/) | [Source](https://studio.blender.org/films/spring/) | Source 103.5–107.375 s; frame-aligned, rotated 1 s smootherstep (quintic) tail-to-head crossfade; native 24 fps; H.264/yuv420p CRF 18, 6 Mb/s VBV, audio removed, width capped at 1920. |
| `spring-forest/preview.jpg` | Blender Foundation; director Andy Goralczyk | [CC-BY-4.0](https://creativecommons.org/licenses/by/4.0/) | [Source](https://studio.blender.org/films/spring/) | 640×360 JPEG preview, center crop of the wallpaper. |
| `spring-forest/theme.json` | Glaze contributors | [MIT](https://opensource.org/license/mit/) | [Source](../LICENSE) | Glaze manifest; no external code or media. |

### Star Nest

The classic fractal star field, faithfully ported from the explicitly MIT-licensed source.

| File | Author | License | Original source | Changes |
|---|---|---|---|---|
| `star-nest/preview.jpg` | Pablo Román Andrioli (Kali) | [MIT](https://opensource.org/license/mit/) | [Source](https://defold.com/tutorials/shadertoy/#the-star-nest-shader) | 640×360 JPEG from an actual DX12 render / local WebView2 desktop capture; taskbar excluded. |
| `star-nest/star-nest.wgsl` | Pablo Román Andrioli (Kali) | [MIT](https://opensource.org/license/mit/) | [Source](https://defold.com/tutorials/shadertoy/#the-star-nest-shader) | WGSL/paper port of the licensed Defold variant; aspect correction restored; see header. |
| `star-nest/theme.json` | Glaze contributors | [MIT](https://opensource.org/license/mit/) | [Source](../LICENSE) | Glaze manifest. |
| `star-nest/upstream.glsl` | Pablo Román Andrioli (Kali) | [MIT](https://opensource.org/license/mit/) | [Source](https://defold.com/tutorials/shadertoy/#the-star-nest-shader) | Unmodified. |

### Waves

An established Vanta.js effect, served entirely from local assets.

| File | Author | License | Original source | Changes |
|---|---|---|---|---|
| `vanta-waves/index.html` | Glaze contributors | [MIT](https://opensource.org/license/mit/) | [Source](../LICENSE) | Local host; 30 fps pacing; Waves ocean preset / Birds twilight preset. Upstream effect code unchanged. |
| `vanta-waves/preview.jpg` | Teng Bao and Vanta contributors | [MIT](https://opensource.org/license/mit/) | [Source](https://github.com/tengbao/vanta) | 640×360 JPEG from an actual DX12 render / local WebView2 desktop capture; taskbar excluded. |
| `vanta-waves/theme.json` | Glaze contributors | [MIT](https://opensource.org/license/mit/) | [Source](../LICENSE) | Glaze manifest; no external code or media. |
| `vanta-waves/three.r134.min.js` | Three.js authors | [MIT](https://opensource.org/license/mit/) | [Source](https://github.com/mrdoob/three.js/tree/r134) | Unmodified. |
| `vanta-waves/vanta.min.js` | Teng Bao and Vanta contributors | [MIT](https://opensource.org/license/mit/) | [Source](https://github.com/tengbao/vanta) | Unmodified. |

## NON-COMMERCIAL — keep separate from paid distributions


### Rainy Hong Kong (non-commercial)

Lively’s rainy window / Heartfelt, with a separately licensed Hong Kong night panorama.

| File | Author | License | Original source | Changes |
|---|---|---|---|---|
| `rain-city/media/city.jpg` | David Iliff (Diliff) | [CC-BY-3.0](https://creativecommons.org/licenses/by/3.0/) | [Source](https://commons.wikimedia.org/wiki/File:Hong_Kong_Skyline_Restitch_-_Dec_2007.jpg) | Downscaled to 2560 px wide, JPEG quality 90; replaces the upstream Pexels mountain image. |
| `rain-city/preview.jpg` | Dani John (Rocksdanister); Martijn Steinrucken (BigWings) | [CC-BY-NC-SA-3.0](https://creativecommons.org/licenses/by-nc-sa/3.0/) | [Source](https://github.com/rocksdanister/rain) | 640×360 JPEG from an actual DX12 render / local WebView2 desktop capture; taskbar excluded. |
| `rain-city/shaders/rain.frag` | Dani John (Rocksdanister); Martijn Steinrucken (BigWings) | [CC-BY-NC-SA-3.0](https://creativecommons.org/licenses/by-nc-sa/3.0/) | [Source](https://github.com/rocksdanister/rain) | Unmodified. Line endings normalized to LF. |
| `rain-city/rain.wgsl` | Dani John (Rocksdanister); Martijn Steinrucken (BigWings) | [CC-BY-NC-SA-3.0](https://creativecommons.org/licenses/by-nc-sa/3.0/) | [Source](https://github.com/rocksdanister/rain) | Native WGSL/paper port with image binding, 30 fps and 0.65 scale; original photo, presets and non-commercial ShareAlike terms retained. All 18 photo samples, analytic Heartfelt gradients and a float32 row/column cache. |
| `rain-city/profile.wgsl` | Dani John (Rocksdanister); Martijn Steinrucken (BigWings) | [CC-BY-NC-SA-3.0](https://creativecommons.org/licenses/by-nc-sa/3.0/) | [Source](https://github.com/rocksdanister/rain) | Theme-owned float32 Heartfelt row/column cache generated once on the selected GPU; CC BY-NC-SA adaptation. |
| `rain-city/theme.json` | Glaze contributors | [MIT](https://opensource.org/license/mit/) | [Source](../LICENSE) | Glaze manifest; no external code or media. |

### Amber Downpour (non-commercial)

A weather/palette variant of the credited Lively Rain / Heartfelt effect over the same licensed night city.

| File | Author | License | Original source | Changes |
|---|---|---|---|---|
| `rain-city-amber/media/city.jpg` | David Iliff (Diliff) | [CC-BY-3.0](https://creativecommons.org/licenses/by/3.0/) | [Source](https://commons.wikimedia.org/wiki/File:Hong_Kong_Skyline_Restitch_-_Dec_2007.jpg) | Downscaled to 2560 px wide, JPEG quality 90; replaces the upstream Pexels mountain image. |
| `rain-city-amber/preview.jpg` | Dani John (Rocksdanister); Martijn Steinrucken (BigWings) | [CC-BY-NC-SA-3.0](https://creativecommons.org/licenses/by-nc-sa/3.0/) | [Source](https://github.com/rocksdanister/rain) | 640×360 JPEG from an actual DX12 render / local WebView2 desktop capture; taskbar excluded. |
| `rain-city-amber/shaders/rain.frag` | Dani John (Rocksdanister); Martijn Steinrucken (BigWings) | [CC-BY-NC-SA-3.0](https://creativecommons.org/licenses/by-nc-sa/3.0/) | [Source](https://github.com/rocksdanister/rain) | Original rain algorithm retained; post-processing tint changed to vec3(1.12, .82, .60). Line endings normalized to LF. |
| `rain-city-amber/rain.wgsl` | Dani John (Rocksdanister); Martijn Steinrucken (BigWings) | [CC-BY-NC-SA-3.0](https://creativecommons.org/licenses/by-nc-sa/3.0/) | [Source](https://github.com/rocksdanister/rain) | Native WGSL/paper port with image binding, 30 fps and 0.65 scale; original photo, presets and non-commercial ShareAlike terms retained. All 18 photo samples, analytic Heartfelt gradients and a float32 row/column cache. |
| `rain-city-amber/profile.wgsl` | Dani John (Rocksdanister); Martijn Steinrucken (BigWings) | [CC-BY-NC-SA-3.0](https://creativecommons.org/licenses/by-nc-sa/3.0/) | [Source](https://github.com/rocksdanister/rain) | Theme-owned float32 Heartfelt row/column cache generated once on the selected GPU; CC BY-NC-SA adaptation. |
| `rain-city-amber/theme.json` | Glaze contributors | [MIT](https://opensource.org/license/mit/) | [Source](../LICENSE) | Glaze manifest; no external code or media. |

### Midnight Drizzle (non-commercial)

A weather/palette variant of the credited Lively Rain / Heartfelt effect over the same licensed night city.

| File | Author | License | Original source | Changes |
|---|---|---|---|---|
| `rain-city-midnight/media/city.jpg` | David Iliff (Diliff) | [CC-BY-3.0](https://creativecommons.org/licenses/by/3.0/) | [Source](https://commons.wikimedia.org/wiki/File:Hong_Kong_Skyline_Restitch_-_Dec_2007.jpg) | Downscaled to 2560 px wide, JPEG quality 90; replaces the upstream Pexels mountain image. |
| `rain-city-midnight/preview.jpg` | Dani John (Rocksdanister); Martijn Steinrucken (BigWings) | [CC-BY-NC-SA-3.0](https://creativecommons.org/licenses/by-nc-sa/3.0/) | [Source](https://github.com/rocksdanister/rain) | 640×360 JPEG from an actual DX12 render / local WebView2 desktop capture; taskbar excluded. |
| `rain-city-midnight/shaders/rain.frag` | Dani John (Rocksdanister); Martijn Steinrucken (BigWings) | [CC-BY-NC-SA-3.0](https://creativecommons.org/licenses/by-nc-sa/3.0/) | [Source](https://github.com/rocksdanister/rain) | Original rain algorithm retained; post-processing tint changed to vec3(.65, .85, 1.25). Line endings normalized to LF. |
| `rain-city-midnight/rain.wgsl` | Dani John (Rocksdanister); Martijn Steinrucken (BigWings) | [CC-BY-NC-SA-3.0](https://creativecommons.org/licenses/by-nc-sa/3.0/) | [Source](https://github.com/rocksdanister/rain) | Native WGSL/paper port with image binding, 30 fps and 0.65 scale; original photo, presets and non-commercial ShareAlike terms retained. All 18 photo samples, analytic Heartfelt gradients and a float32 row/column cache. |
| `rain-city-midnight/profile.wgsl` | Dani John (Rocksdanister); Martijn Steinrucken (BigWings) | [CC-BY-NC-SA-3.0](https://creativecommons.org/licenses/by-nc-sa/3.0/) | [Source](https://github.com/rocksdanister/rain) | Theme-owned float32 Heartfelt row/column cache generated once on the selected GPU; CC BY-NC-SA adaptation. |
| `rain-city-midnight/theme.json` | Glaze contributors | [MIT](https://opensource.org/license/mit/) | [Source](../LICENSE) | Glaze manifest; no external code or media. |

### Seascape (non-commercial)

A faithful port of an existing licensed GLSL effect.

| File | Author | License | Original source | Changes |
|---|---|---|---|---|
| `seascape/preview.jpg` | Alexander Alekseev (TDM) | [CC-BY-NC-SA-3.0](https://creativecommons.org/licenses/by-nc-sa/3.0/) | [Source](https://github.com/tdmaav/shadertoy/blob/master/Seascape.shader) | 640×360 JPEG from an actual DX12 render / local WebView2 desktop capture; taskbar excluded. |
| `seascape/seascape.wgsl` | Alexander Alekseev (TDM) | [CC-BY-NC-SA-3.0](https://creativecommons.org/licenses/by-nc-sa/3.0/) | [Source](https://github.com/tdmaav/shadertoy/blob/master/Seascape.shader) | WGSL port, paper uniforms and Y-coordinate conversion; see the file header for adaptation details. |
| `seascape/theme.json` | Glaze contributors | [MIT](https://opensource.org/license/mit/) | [Source](../LICENSE) | Glaze manifest; no external code or media. |
| `seascape/upstream.glsl` | Alexander Alekseev (TDM) | [CC-BY-NC-SA-3.0](https://creativecommons.org/licenses/by-nc-sa/3.0/) | [Source](https://github.com/tdmaav/shadertoy/blob/master/Seascape.shader) | Unmodified. |

### Seascape — Midnight (non-commercial)

TDM’s Seascape with a restrained midnight colour grade; the wave, lighting and camera algorithms are unchanged.

| File | Author | License | Original source | Changes |
|---|---|---|---|---|
| `seascape-midnight/preview.jpg` | Alexander Alekseev (TDM) | [CC-BY-NC-SA-3.0](https://creativecommons.org/licenses/by-nc-sa/3.0/) | [Source](https://github.com/tdmaav/shadertoy/blob/master/Seascape.shader) | 640×360 JPEG from an actual DX12 render / local WebView2 desktop capture; taskbar excluded. |
| `seascape-midnight/seascape.wgsl` | Alexander Alekseev (TDM) | [CC-BY-NC-SA-3.0](https://creativecommons.org/licenses/by-nc-sa/3.0/) | [Source](https://github.com/tdmaav/shadertoy/blob/master/Seascape.shader) | WGSL port, paper uniforms and Y-coordinate conversion; see the file header for adaptation details. Fixed default iMouse; midnight output grade (0.07, 0.16, 0.30). No wave/camera algorithm changes. |
| `seascape-midnight/theme.json` | Glaze contributors | [MIT](https://opensource.org/license/mit/) | [Source](../LICENSE) | Glaze manifest; no external code or media. |
| `seascape-midnight/upstream.glsl` | Alexander Alekseev (TDM) | [CC-BY-NC-SA-3.0](https://creativecommons.org/licenses/by-nc-sa/3.0/) | [Source](https://github.com/tdmaav/shadertoy/blob/master/Seascape.shader) | Unmodified. |

### Snow over Hong Kong (non-commercial)

The existing Lively snow effect over a separately licensed atmospheric background.

| File | Author | License | Original source | Changes |
|---|---|---|---|---|
| `snow-city/media/image.jpg` | David Iliff (Diliff) | [CC-BY-3.0](https://creativecommons.org/licenses/by/3.0/) | [Source](https://commons.wikimedia.org/wiki/File:Hong_Kong_Skyline_Restitch_-_Dec_2007.jpg) | Licensed city backdrop downscaled to 2560 px wide and JPEG-encoded; replaces the upstream Pexels image. |
| `snow-city/preview.jpg` | Andrew Baldwin; Lively adaptation by Dani John | [CC-BY-NC-SA-3.0](https://creativecommons.org/licenses/by-nc-sa/3.0/) | [Source](https://github.com/rocksdanister/snow) | 640×360 JPEG from an actual DX12 render / local WebView2 desktop capture; taskbar excluded. |
| `snow-city/shaders/snow.frag` | Andrew Baldwin; Lively adaptation by Dani John | [CC-BY-NC-SA-3.0](https://creativecommons.org/licenses/by-nc-sa/3.0/) | [Source](https://github.com/rocksdanister/snow) | Unmodified. Line endings normalized to LF. |
| `snow-city/snow.wgsl` | Andrew Baldwin; Lively adaptation by Dani John | [CC-BY-NC-SA-3.0](https://creativecommons.org/licenses/by-nc-sa/3.0/) | [Source](https://github.com/rocksdanister/snow) | Native WGSL/paper port with image binding, 30 fps and 0.65 scale; original photo, presets and non-commercial ShareAlike terms retained. Preserves the original 50 layers and CSS-resolution crop/density. |
| `snow-city/theme.json` | Glaze contributors | [MIT](https://opensource.org/license/mit/) | [Source](../LICENSE) | Glaze manifest. |

### Snow in the Misty Forest (non-commercial)

The existing Lively snow effect over a separately licensed atmospheric background.

| File | Author | License | Original source | Changes |
|---|---|---|---|---|
| `snow-forest/media/image.jpg` | Blender Foundation; director Andy Goralczyk | [CC-BY-4.0](https://creativecommons.org/licenses/by/4.0/) | [Source](https://studio.blender.org/films/spring/) | Licensed backdrop downscaled/encoded to JPEG; forest frame at 105 s. Replaces the upstream Pexels image. |
| `snow-forest/preview.jpg` | Andrew Baldwin; Lively adaptation by Dani John | [CC-BY-NC-SA-3.0](https://creativecommons.org/licenses/by-nc-sa/3.0/) | [Source](https://github.com/rocksdanister/snow) | 640×360 JPEG from an actual DX12 render / local WebView2 desktop capture; taskbar excluded. |
| `snow-forest/shaders/snow.frag` | Andrew Baldwin; Lively adaptation by Dani John | [CC-BY-NC-SA-3.0](https://creativecommons.org/licenses/by-nc-sa/3.0/) | [Source](https://github.com/rocksdanister/snow) | Unmodified. Line endings normalized to LF. |
| `snow-forest/snow.wgsl` | Andrew Baldwin; Lively adaptation by Dani John | [CC-BY-NC-SA-3.0](https://creativecommons.org/licenses/by-nc-sa/3.0/) | [Source](https://github.com/rocksdanister/snow) | Native WGSL/paper port with image binding, 30 fps and 0.65 scale; original photo, presets and non-commercial ShareAlike terms retained. Preserves the original 50 layers and CSS-resolution crop/density. |
| `snow-forest/theme.json` | Glaze contributors | [MIT](https://opensource.org/license/mit/) | [Source](../LICENSE) | Glaze manifest. |

### Snowfall (non-commercial)

A faithful port of an existing licensed GLSL effect.

| File | Author | License | Original source | Changes |
|---|---|---|---|---|
| `snowfall/preview.jpg` | Andrew Baldwin; Lively adaptation by Dani John | [CC-BY-NC-SA-3.0](https://creativecommons.org/licenses/by-nc-sa/3.0/) | [Source](https://github.com/rocksdanister/snow/blob/main/shaders/snow.frag) | 640×360 JPEG from an actual DX12 render / local WebView2 desktop capture; taskbar excluded. |
| `snowfall/snowfall.wgsl` | Andrew Baldwin; Lively adaptation by Dani John | [CC-BY-NC-SA-3.0](https://creativecommons.org/licenses/by-nc-sa/3.0/) | [Source](https://github.com/rocksdanister/snow/blob/main/shaders/snow.frag) | WGSL port, paper uniforms and Y-coordinate conversion; see the file header for adaptation details. |
| `snowfall/theme.json` | Glaze contributors | [MIT](https://opensource.org/license/mit/) | [Source](../LICENSE) | Glaze manifest; no external code or media. |
| `snowfall/upstream.glsl` | Andrew Baldwin; Lively adaptation by Dani John | [CC-BY-NC-SA-3.0](https://creativecommons.org/licenses/by-nc-sa/3.0/) | [Source](https://github.com/rocksdanister/snow/blob/main/shaders/snow.frag) | Unmodified. |

## Shared vendor permission notices

Full notices below are shipped alongside the assets. The dat.gui file is **Apache-2.0**, not MIT. Vanta uses Three.js r134; Rain uses r150. Star Nest has its own MIT grant; Shadertoy’s default NC license must not be applied over its explicit license.

| File | Author | License | Original source | Changes |
|---|---|---|---|---|
| `licenses/fluids-MIT.txt` | Pavel Dobryakov | [MIT](https://github.com/PavelDoGreat/WebGL-Fluid-Simulation/blob/master/LICENSE) | [Source](https://github.com/PavelDoGreat/WebGL-Fluid-Simulation/blob/master/LICENSE) | Full upstream license/copyright notice preserved. |
| `licenses/vanta-MIT.txt` | Teng Bao | [MIT](https://github.com/tengbao/vanta/blob/master/LICENSE.md) | [Source](https://github.com/tengbao/vanta/blob/master/LICENSE.md) | Full upstream license/copyright notice preserved. |
| `licenses/three-r134-MIT.txt` | Three.js authors | [MIT](https://github.com/mrdoob/three.js/blob/r134/LICENSE) | [Source](https://github.com/mrdoob/three.js/blob/r134/LICENSE) | Full upstream license/copyright notice preserved. |
| `licenses/three-r150-MIT.txt` | Three.js authors | [MIT](https://github.com/mrdoob/three.js/blob/r150/LICENSE) | [Source](https://github.com/mrdoob/three.js/blob/r150/LICENSE) | Full upstream license/copyright notice preserved. |
| `licenses/dat-gui-Apache-2.0.txt` | Data Arts Team, Google Creative Lab | [Apache-2.0](https://github.com/dataarts/dat.gui/blob/master/LICENSE) | [Source](https://github.com/dataarts/dat.gui/blob/master/LICENSE) | Full upstream license/copyright notice preserved. |
| `licenses/star-nest-MIT.txt` | Pablo Román Andrioli (Kali) | [MIT](https://opensource.org/license/mit/) | [Source](https://defold.com/tutorials/shadertoy/#the-star-nest-shader) | Full MIT permission notice supplied with the author credit preserved by the licensed source. |
| `.gitattributes` | Glaze contributors | [MIT](https://opensource.org/license/mit/) | [Source](../LICENSE) | Canonical LF for attributed text; media marked binary. |
| `verify-loops.py` | Glaze contributors | [MIT](https://opensource.org/license/mit/) | [Source](../LICENSE) | Read-only FFmpeg, SSIM/PSNR, per-pixel, registration and optical-flow measurement tool. |

## Preserved legacy themes and examples

These are excluded from the curated filesystem gallery, but old IDs and old `themes/<id>` paths resolve. Existing shaders were preserved, not replaced by the stopped agent’s handcrafted versions. The `app-example` folder remains a documented template without a manifest or bundled executable.


### `examples/app-example`

| File | Author | License | Original source | Changes |
|---|---|---|---|---|
| `examples/app-example/preview.jpg` | Glaze contributors | [MIT](https://opensource.org/license/mit/) | [Source](../LICENSE) | Existing Glaze legacy/example asset preserved; moved under examples/. |
| `examples/app-example/README.md` | Glaze contributors | [MIT](https://opensource.org/license/mit/) | [Source](../LICENSE) | Existing Glaze legacy/example asset preserved; moved under examples/. |

### `examples/aurora`

| File | Author | License | Original source | Changes |
|---|---|---|---|---|
| `examples/aurora/aurora.wgsl` | Glaze contributors | [MIT](https://opensource.org/license/mit/) | [Source](../LICENSE) | Existing Glaze legacy/example asset preserved; moved under examples/. |
| `examples/aurora/preview.jpg` | Glaze contributors | [MIT](https://opensource.org/license/mit/) | [Source](../LICENSE) | Existing Glaze legacy/example asset preserved; moved under examples/. |
| `examples/aurora/theme.json` | Glaze contributors | [MIT](https://opensource.org/license/mit/) | [Source](../LICENSE) | Existing manifest moved under examples/; hidden=true added. Wallpaper content and selected IDs preserved. |

### `examples/earth-orbit`

| File | Author | License | Original source | Changes |
|---|---|---|---|---|
| `examples/earth-orbit/earth-orbit.mp4` | NASA (JSC) | [NASA-PD](https://www.nasa.gov/nasa-brand-center/images-and-media/) | [Source](https://images.nasa.gov/details/jsc2021m000138_4K_Earth_Views_Extended_Cut_for_Earth_Day_%202021_210422-4KMP4) | Existing 2560px-or-smaller JPEG / 1080p muted video preserved; moved under examples/. |
| `examples/earth-orbit/preview.jpg` | NASA (JSC) | [NASA-PD](https://www.nasa.gov/nasa-brand-center/images-and-media/) | [Source](https://images.nasa.gov/details/jsc2021m000138_4K_Earth_Views_Extended_Cut_for_Earth_Day_%202021_210422-4KMP4) | Existing 640×360 preview derived from the credited NASA media; moved under examples/. |
| `examples/earth-orbit/README.md` | Glaze contributors | [MIT](https://opensource.org/license/mit/) | [Source](../LICENSE) | Existing Glaze legacy/example asset preserved; moved under examples/. |
| `examples/earth-orbit/theme.json` | Glaze contributors | [MIT](https://opensource.org/license/mit/) | [Source](../LICENSE) | Existing manifest moved under examples/; hidden=true added. Wallpaper content and selected IDs preserved. |

### `examples/gallery`

| File | Author | License | Original source | Changes |
|---|---|---|---|---|
| `examples/gallery/images/01-aurora-over-earth.jpg` | NASA (ISS Expedition 66, JSC) | [NASA-PD](https://www.nasa.gov/nasa-brand-center/images-and-media/) | [Source](https://images.nasa.gov/details/iss066e146457) | Existing 2560px-or-smaller JPEG / 1080p muted video preserved; moved under examples/. |
| `examples/gallery/images/02-aurora-glow.jpg` | NASA (ISS Expedition 62, JSC) | [NASA-PD](https://www.nasa.gov/nasa-brand-center/images-and-media/) | [Source](https://images.nasa.gov/details/iss062e148363) | Existing 2560px-or-smaller JPEG / 1080p muted video preserved; moved under examples/. |
| `examples/gallery/images/03-rio-at-night.jpg` | NASA (ISS Expedition 67, JSC) | [NASA-PD](https://www.nasa.gov/nasa-brand-center/images-and-media/) | [Source](https://images.nasa.gov/details/iss067e176807) | Existing 2560px-or-smaller JPEG / 1080p muted video preserved; moved under examples/. |
| `examples/gallery/images/04-istanbul-at-night.jpg` | NASA (ISS Expedition 32, JSC) | [NASA-PD](https://www.nasa.gov/nasa-brand-center/images-and-media/) | [Source](https://images.nasa.gov/details/iss032e017547) | Existing 2560px-or-smaller JPEG / 1080p muted video preserved; moved under examples/. |
| `examples/gallery/images/05-orbital-sunrise.jpg` | NASA (ISS Expedition 28, JSC) | [NASA-PD](https://www.nasa.gov/nasa-brand-center/images-and-media/) | [Source](https://images.nasa.gov/details/iss028e007274) | Existing 2560px-or-smaller JPEG / 1080p muted video preserved; moved under examples/. |
| `examples/gallery/images/06-andromeda-galaxy.jpg` | NASA/JPL-Caltech | [NASA-PD](https://www.nasa.gov/nasa-brand-center/images-and-media/) | [Source](https://images.nasa.gov/details/PIA26276) | Existing 2560px-or-smaller JPEG / 1080p muted video preserved; moved under examples/. |
| `examples/gallery/images/07-tarantula-nebula.jpg` | NASA/JPL-Caltech | [NASA-PD](https://www.nasa.gov/nasa-brand-center/images-and-media/) | [Source](https://images.nasa.gov/details/PIA23646) | Existing 2560px-or-smaller JPEG / 1080p muted video preserved; moved under examples/. |
| `examples/gallery/preview.jpg` | NASA (ISS Expedition 66, JSC) | [NASA-PD](https://www.nasa.gov/nasa-brand-center/images-and-media/) | [Source](https://images.nasa.gov/details/iss066e146457) | Existing 640×360 preview derived from the credited NASA media; moved under examples/. Collage uses the seven individually credited gallery images below. |
| `examples/gallery/theme.json` | Glaze contributors | [MIT](https://opensource.org/license/mit/) | [Source](../LICENSE) | Existing manifest moved under examples/; hidden=true added. Wallpaper content and selected IDs preserved. |

### `examples/interactive`

| File | Author | License | Original source | Changes |
|---|---|---|---|---|
| `examples/interactive/index.html` | Glaze contributors | [MIT](https://opensource.org/license/mit/) | [Source](../LICENSE) | Existing Glaze legacy/example asset preserved; moved under examples/. |
| `examples/interactive/preview.jpg` | Glaze contributors | [MIT](https://opensource.org/license/mit/) | [Source](../LICENSE) | New 640×360 preview from the existing interactive example running in WebView2; taskbar cropped out. |
| `examples/interactive/theme.json` | Glaze contributors | [MIT](https://opensource.org/license/mit/) | [Source](../LICENSE) | Existing manifest moved under examples/; hidden=true added. Wallpaper content and selected IDs preserved. |

### `examples/islands`

| File | Author | License | Original source | Changes |
|---|---|---|---|---|
| `examples/islands/islands.glb` | Glaze contributors | [MIT](https://opensource.org/license/mit/) | [Source](../LICENSE) | Existing Glaze legacy/example asset preserved; moved under examples/. |
| `examples/islands/preview.jpg` | Glaze contributors | [MIT](https://opensource.org/license/mit/) | [Source](../LICENSE) | Existing Glaze legacy/example asset preserved; moved under examples/. |
| `examples/islands/theme.json` | Glaze contributors | [MIT](https://opensource.org/license/mit/) | [Source](../LICENSE) | Existing manifest moved under examples/; hidden=true added. Wallpaper content and selected IDs preserved. |

### `examples/midnight`

| File | Author | License | Original source | Changes |
|---|---|---|---|---|
| `examples/midnight/bars.css` | Glaze contributors | [MIT](https://opensource.org/license/mit/) | [Source](../LICENSE) | Existing Glaze legacy/example asset preserved; moved under examples/. |
| `examples/midnight/midnight.wgsl` | Glaze contributors | [MIT](https://opensource.org/license/mit/) | [Source](../LICENSE) | Existing Glaze legacy/example asset preserved; moved under examples/. |
| `examples/midnight/preview.jpg` | Glaze contributors | [MIT](https://opensource.org/license/mit/) | [Source](../LICENSE) | Existing Glaze legacy/example asset preserved; moved under examples/. |
| `examples/midnight/theme.json` | Glaze contributors | [MIT](https://opensource.org/license/mit/) | [Source](../LICENSE) | Existing manifest moved under examples/; hidden=true added. Wallpaper content and selected IDs preserved. |
| `examples/midnight/widgets.css` | Glaze contributors | [MIT](https://opensource.org/license/mit/) | [Source](../LICENSE) | Existing Glaze legacy/example asset preserved; moved under examples/. |

### `examples/builder-demo`

| File | Author | License | Original source | Changes |
|---|---|---|---|---|
| `examples/builder-demo/media/city.jpg` | David Iliff (Diliff) | [CC-BY-3.0](https://creativecommons.org/licenses/by/3.0/) | [Source](https://commons.wikimedia.org/wiki/File:Hong_Kong_Skyline_Restitch_-_Dec_2007.jpg) | Same file as rain-city/media/city.jpg (downscaled to 2560 px wide, JPEG quality 90). |
| `examples/builder-demo/theme.json` | Glaze contributors | [MIT](https://opensource.org/license/mit/) | [Source](../LICENSE) | Original stack manifest (docs/wallpaper-builder.md); hidden. |

### `examples/photo`

| File | Author | License | Original source | Changes |
|---|---|---|---|---|
| `examples/photo/iss-aurora.jpg` | NASA (ISS Expedition 65, JSC) | [NASA-PD](https://www.nasa.gov/nasa-brand-center/images-and-media/) | [Source](https://images.nasa.gov/details/iss065e214221) | Existing 2560px-or-smaller JPEG / 1080p muted video preserved; moved under examples/. |
| `examples/photo/preview.jpg` | NASA (ISS Expedition 65, JSC) | [NASA-PD](https://www.nasa.gov/nasa-brand-center/images-and-media/) | [Source](https://images.nasa.gov/details/iss065e214221) | Existing 640×360 preview derived from the credited NASA media; moved under examples/. |
| `examples/photo/theme.json` | Glaze contributors | [MIT](https://opensource.org/license/mit/) | [Source](../LICENSE) | Existing manifest moved under examples/; hidden=true added. Wallpaper content and selected IDs preserved. |

### `examples/webdemo`

| File | Author | License | Original source | Changes |
|---|---|---|---|---|
| `examples/webdemo/index.html` | Glaze contributors | [MIT](https://opensource.org/license/mit/) | [Source](../LICENSE) | Existing Glaze legacy/example asset preserved; moved under examples/. |
| `examples/webdemo/preview.jpg` | Glaze contributors | [MIT](https://opensource.org/license/mit/) | [Source](../LICENSE) | Existing Glaze legacy/example asset preserved; moved under examples/. |
| `examples/webdemo/theme.json` | Glaze contributors | [MIT](https://opensource.org/license/mit/) | [Source](../LICENSE) | Existing manifest moved under examples/; hidden=true added. Wallpaper content and selected IDs preserved. |

The manifests, local adapter HTML and original Glaze example code use the repository [MIT license](../LICENSE). This document and `catalog.json` are original Glaze attribution/metadata files under the same license; their factual credit records do not relicense any referenced content.


### 3D builder samples

| File | Author | License | Original source | Changes |
|---|---|---|---|---|
| `examples/ringed-planet/planet.glb`, `examples/ringed-planet/generate.py`, `examples/ringed-planet/README.md` | Glaze contributors (2026) | [CC0 1.0](https://creativecommons.org/publicdomain/zero/1.0/) | Original procedural asset; reproducible with the included generator | Original geometry, PBR maps, translucent rings, node and morph clips. |
| `examples/ringed-planet/theme.json` | Glaze contributors | [MIT](https://opensource.org/license/mit/) | [Source](../LICENSE) | Original example manifest. |
| `examples/fox-model/Fox.glb`, `examples/fox-model/UPSTREAM.md` | PixelMannen (model, 2014); tomkranis (rig/animation, 2014); AsoboStudio and scurest (glTF conversion, 2017) | Model: [CC0 1.0](https://creativecommons.org/publicdomain/zero/1.0/); rig/animation/conversion: [CC BY 4.0](https://creativecommons.org/licenses/by/4.0/) | [Khronos Fox](https://github.com/KhronosGroup/glTF-Sample-Assets/tree/main/Models/Fox) | Unmodified GLB. Full upstream attribution retained beside the model. |
| `examples/simple-skin/SimpleSkin.gltf`, `examples/simple-skin/SimpleSkin_animation.bin`, `examples/simple-skin/SimpleSkin_geometry.bin`, `examples/simple-skin/SimpleSkin_inverseBindMatrices.bin`, `examples/simple-skin/SimpleSkin_skinningData.bin`, `examples/simple-skin/UPSTREAM.md` | Marco Hutter (2017) | [CC0 1.0](https://creativecommons.org/publicdomain/zero/1.0/) | [Khronos SimpleSkin](https://github.com/KhronosGroup/glTF-Sample-Assets/tree/main/Models/SimpleSkin) | Unmodified glTF and buffers. Full upstream attribution retained beside the model. |
| `examples/fox-model/README.md`, `examples/simple-skin/README.md` | Glaze contributors | [MIT](https://opensource.org/license/mit/) | [Source](../LICENSE) | Original sample notes. |