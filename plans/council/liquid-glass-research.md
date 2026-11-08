# Liquid Glass: what Apple's material does, and how to reproduce it

Research for Glaze's Liquid Glass style (bars, flyouts, desktop widgets), November 2026. Every
claim cites a source from the [list at the end](#sources). Apple's own words come first ([A1]–[A13]).
Third-party reimplementations come after ([O1]–[O5]); one of them ([O5]) is reverse-engineered from
macOS and is marked unverified wherever it is used. What Glaze builds from this is in
[What Glaze takes from this](#what-glaze-takes-from-this).

## The material in Apple's words

- **What it is.** Liquid Glass is "a new digital meta-material that dynamically bends and shapes
  light", not a simulation of real glass [A1, Intro]. It "reflects and refracts its surroundings"
  [A12], and controls "refract the content behind them — while reflecting content and the user's
  wallpaper from around them" [A12]. It "uses real-time rendering and dynamically reacts to
  movement with specular highlights" [A12].
- **Where it goes.** It is the functional layer: controls and navigation floating above content,
  never the content layer itself [A4, "Liquid Glass"; A2, Structure]. Avoid glass on glass [A1,
  Principles; A10]. Use it sparingly [A4; A10].
- **Shapes.** Rounded, floating forms that nest in the device's curves [A1, Dynamics]. Three
  shape types: fixed radius, capsules (radius = half the height) and concentric shapes (the
  parent's radius minus the padding) [A2, Design Language]. SwiftUI's default shape is a capsule
  [A5]. Continuous (squircle) corners are what Core Animation uses for glass [O5, unverified].

### Lensing, not scattering

- "The primary way Liquid Glass visually defines itself is through something called Lensing."
  [A1, Dynamics]
- "Where as previous materials scattered light, this new set of materials dynamically bends,
  shapes, and concentrates light in real time." [A1, Dynamics]
- **Where the bending happens.** Apple does not give a profile [A1; noted by the transcript
  summary]. Every reimplementation that matches Apple's look bends light in a band just inside the
  edge and leaves the middle clear: a convex bevel, strongest at the rim, flat by the middle
  [O1 §2–3; O3 `RoundedRectRefractionShaderString`; O5, `edgeOffset`, unverified]. A convex
  profile bends rays inward, so the rim shows content from further inside the shape. A concave one
  would need pixels from outside the shape [O1 §2]. On macOS the measured amounts scale with the
  glass's short side S: an outer refraction of 0.2 S over a band of 0.125 S [O5, unverified].
- **Bigger glass is thicker glass.** "When glass flexes and morphs to larger sizes ... its
  material characteristics change to simulate a thicker, more substantial material. It casts
  deeper, richer shadows, has more pronounced lensing and refraction effects, and a softer
  scattering of light." [A1, Adaptivity]

### Specular highlights: light on geometry, not a uniform outline

- "Light sources inside of this environment shine on the material producing highlights that
  respond to geometry just as you'd expect. On interactions ... these lights move in space,
  causing light to travel around the material, defining its silhouette. And in some cases, the
  lighting responds to device motion." [A1, Principles]
- Apple gives no angle or width [A1]. The reimplementations agree on the shape of it:
  - It is a thin line along the rim (about 1–1.2 pt), brightest on the two walls facing along
    the light and fading around the corners in between [O2, "Glint"; O3
    `DefaultHighlightShaderString`, `pow(abs(dot(normal, light)), falloff)`].
  - macOS draws it as two cones of about 160°, key and fill at opposite angles, so the top-left
    and bottom-right arcs light up while the other two corners go dark [O5, rim highlight,
    unverified]. The [Tahoe crop](#reference-pictures) shows exactly that.
  - It takes on the colour of what's behind instead of plain white: "glass over color glints in
    that color" [O2, "Glint"].
- So it is **not** a uniform lit border. A constant 1 px outline, or a gradient border brightest
  along the top, reads as a plastic frame.

### Adaptivity: tint, luminance and light/dark

- "Unlike previous materials that had a fixed light or dark appearance, each layer continuously
  adapts based on what's behind it." [A1, Adaptivity]
- "The amount of tint and the dynamic range shift to always ensure buttons remain legible, while
  letting as much of the content through as possible." [A1, Adaptivity]
- Small elements (nav bars, tab bars) flip between light and dark with the background; big ones
  (menus, sidebars) adapt but don't flip, since "transitions like these would be distracting"
  [A1, Principles]. Glyphs on regular glass flip with the glass [A1, Principles].
- **Tinting.** "Selecting a color generates a range of tones that are mapped to content
  brightness underneath the tinted element", like coloured glass whose hue, brightness and
  saturation follow what's behind [A1, Principles]. A solid fill instead "breaks the visual
  character of Liquid Glass" [A1, Principles]. Tint conveys meaning, never decoration [A3; A1].
- How macOS does the luminance part (unverified): the backdrop's luma is remapped into a narrower
  `[black, white]` range, saturated, then composited under a white or black fill. For example,
  regular light glass uses black 0.5, white 1.03 and a fill of 0.4; regular dark uses black 0.2,
  white 0.6 [O5, `qcRecipe`]. This is "the dynamic range shift" of [A1].

### Variants and blur

- **Regular** "blurs and adjusts the luminosity of background content to maintain legibility",
  works at any size over any content, and is what most system components use [A4; A1,
  Principles].
- **Clear** "is highly translucent", for components over media [A4]. It "does not have adaptive
  behaviors" and needs a dimming layer for legibility [A1, Principles]. Over bright content, add "a
  dark dimming layer of 35% opacity"; over dark content, none [A4]. The API example puts
  `.black.opacity(0.3)` beneath it [A6, `clear`].
- Apple publishes no blur radius [A1; A4]. On macOS (unverified), the backdrop is captured at 1/4
  of the resolution for regular glass and at 1/2 for clear, and the blur radius is
  `1.6 × input × backing scale` px, varying a little with the distance to the edge without ever
  going to zero [O5].
- Reduce Transparency makes the glass "frostier", Increase Contrast makes it black or white with
  a contrasting border, and Reduce Motion "decreases the intensity of some effects and disables
  any elastic properties" [A1, Principles].
- A window that loses focus on the Mac "visually recedes" [A1, Principles]. On macOS that's a
  flatter recipe with no outer refraction, bleed or shadow [O5, unverified].

### Shadows: yes, soft and adaptive

Liquid Glass has a shadow, but not a fixed drop shadow:

- "Shadows also play an important role in helping elements feel grounded and defined ... The
  element is aware of what's behind it and increases the opacity of its shadow when it is over
  text. Conversely, it lowers the opacity of its shadow when it is over a solid light background."
  [A1, Principles]
- "As text scrolls underneath, shadows become more prominent to create additional separation."
  [A1, Adaptivity]
- Larger glass "casts deeper, richer shadows", and light from nearby content "bleeds into the
  shadow as well" [A1, Adaptivity]: the shadow is coloured by the content, not plain black.
- On macOS (unverified): regular glass has a shadow of opacity 0.25–0.5, about 40 pt of blur and
  offset slightly downward, darker on small elements. **Clear glass has none** [O5, `qcRecipe`:
  `qShadowOpacity: clear ? 0 : ...`].
- What the references show: the Tahoe clear dock and widgets have no visible shadow, and the
  Control Center modules only a faint one ([Reference pictures](#reference-pictures)).

### Motion

- **Materialize.** "Instead of fading, Liquid Glass objects materialize in and out by gradually
  modulating the light bending and lensing." [A1, Dynamics]. SwiftUI's `materialize` transition is
  for effects farther apart than the container's spacing [A8].
- **Morphing and merging.** Glass "dynamically morphs between the controls in each context ...
  When showing a menu, the bubble simply pops open to reveal the content contained within." [A1,
  Dynamics]. Buttons "fluidly morph into menus and popovers" [A10]. In a `GlassEffectContainer`,
  "as shapes near one another, their paths start to blend into one another. The higher the
  spacing, the sooner blending begins" [A7]. Effects inside a container's spacing transition with
  `matchedGeometry` [A8]. `glassEffectUnion` makes several views one shape even at rest [A9].
  Reimplementations do the blending as a smooth minimum of the shapes' signed distance fields
  [O2, "Blending shapes"; O5, "Second glass", unverified].
- **Press.** "Liquid Glass responds to interaction by instantly flexing and energizing with light"
  and has "gel-like flexibility" [A1, Dynamics]. "The material illuminates from within as a form of
  feedback. Starting right under your fingertips, the glow spreads throughout the element and onto
  any Liquid Glass elements nearby." [A1, Principles]. Interactive glass reacts "by scaling,
  bouncing, and shimmering" [A3, 19:21]. `interactive()` reacts to touch and pointer [A8; A11,
  `effectIsInteractive`].
- **Sampling.** Glass samples "content from an area larger than itself", and "glass can not sample
  other glass", so nearby glass belongs in one container [A3, 19:51]. Combining effects in a
  container is also how they render efficiently [A7; A8, "Optimize performance"; A10].

## How others reproduce it

| Implementation | Refraction | Highlight | Blur | Merging | Cost |
|---|---|---|---|---|---|
| kube.io, CSS + SVG [O1] | Snell's law (n = 1.5) traced along one radius of a convex squircle bevel, `y = (1 − (1 − x)⁴)^¼`, baked into a displacement map, applied with `feDisplacementMap` | a rim light by the normal's angle to the light (−60°), as a second `feImage` | an SVG blur | no | Every size change rebuilds the whole map. Animating `scale` is cheap. `backdrop-filter: url()` is Chromium-only. |
| shuding/liquid-glass [O4] | a per-pixel JS `fragment()` (rounded-rect SDF, `smoothStep`) drawn to a canvas, then `backdrop-filter: url(#filter)` | none (CSS `contrast`/`brightness`) | `blur(0.25px)` | no | A CPU map per element. Chromium-only. |
| Kyant0/AndroidLiquidGlass [O3] | an AGSL shader: rounded-rect SDF, `circleMap(1 − depth/height) × amount` along the normal (computed with a 1.5× radius for smooth corners), optional 7-tap dispersion | `pow(abs(dot(normal, light)), falloff)` at 45°, additive | a platform blur | no | One shader pass per element over a backdrop the platform already captured. |
| flutter liquid_glass_renderer [O2] | an SDF geometry pass plus a final render pass, fitted to iOS 27 captures | a 1.2 pt glint on the two walls facing the light, recolouring rather than adding white | power-of-two downsampled blur | smooth union in a `LiquidGlassBlendGroup` within `blend` px (default 20) | "The unit of cost is the backdrop copy." Keep layers small and few, don't animate blend geometry every frame, keep shadows few. |
| spacedouut playground (WebGL2) [O5] | macOS 26's `glass_background` recipe transcribed from QuartzCore (unverified): circular-cap `edgeOffset` along the SDF gradient, size-driven amounts | two 160° cones, key at −45°, fill at +135°, a 1 pt band | mip-LOD blur from a 1/4 (regular) or 1/2 (clear) capture | polynomial smooth min | one full-screen pass |

What carries over:
- The look comes from an **SDF**: refraction along its gradient with a convex profile, the
  highlight from the angle between that gradient and the light, merging as a smooth minimum.
- **The cost is getting the pixels behind**, not the shader. Once a backdrop exists on the GPU,
  one pass per element is cheap [O2, "Performance"; O3].
- The web versions pay on the CPU: they rebuild maps on resize and run SVG filters in the
  compositor, and `backdrop-filter: url()` only works in Chromium [O1 §6–7; O4].

## Reference pictures

Saved outside the repository (they are Apple's images) in `C:\glaze-targets\refs\liquid-glass\`,
with each source URL in a `.source.txt` next to the file. Crops are in `crops\`.

| File | Shows | Source |
|---|---|---|
| `press-liquid-glass\...\Apple-WWDC25-Liquid-Glass-Home-Screen-clear-look-250609.jpg` | macOS Tahoe, **clear look**: glass desktop widgets and dock over a light photo | [A12] press kit, `DL-Images-of-Apple-WWDC25-Liquid-Glass-250609.zip` |
| `crops\tahoe-clear-widget-edge.png` | the calendar widget's rim, 3× | crop of the above |
| `crops\tahoe-clear-dock.png` | the clear dock | crop of the above |
| `apple-macOS-Tahoe-26-Control-Center-250609_big.jpg`, `crops\cc-wifi-edge.png` | Control Center modules (regular glass) over the Tahoe wallpaper | [A13] |
| `press-liquid-glass\...\Apple-WWDC25-Liquid-Glass-hero-250609.jpg`, `crops\hero-ipad-dock.png`, `crops\hero-mac-dock.png` | iPadOS and macOS docks over blue waves | [A12] press kit |
| `press-liquid-glass\...\Apple-WWDC25-Liquid-Glass-Home-Screen-dark-tint-250609.jpg` | the dark-tinted look | [A12] press kit |
| `apple-Liquid-Glass-details-250609.jpg` | thick glass numerals over dark leaves: a bright rim on the walls facing the light | [A12] video poster |
| `apple-macOS-Tahoe-26-hero-250609_big.jpg`, `apple-macOS-Tahoe-26-Safari-250609_big.jpg`, `apple-macOS-Tahoe-26-Apple-Music-250609_big.jpg` | macOS Tahoe desktops, the transparent menu bar | [A13] |

What they show, beyond the text:
- **Tahoe clear widgets** are a dimmed copy of the wallpaper: darker than their surroundings,
  slightly blurred. A lighter band sits a few points inside the edge, and a hairline rim is
  brightest at the top-left and bottom-right corners and fades on the top-right and bottom-left.
  There's no visible drop shadow.
- **The clear dock** is the same: a hairline rim and a slightly lifted wallpaper, no shadow.
- **Control Center modules** (regular) show a blurred, lifted backdrop, the same two-corner rim,
  and a soft inner brightening at the bottom-right where the bevel meets the light.
- **The menu bar has no glass**: "The menu bar is now completely transparent" [A13].

## What Glaze takes from this

The spec the implementation follows (docs/bars.md, "Liquid glass", and docs/widgets.md):

1. **Lens.** A convex bevel in a band about 0.1–0.2 of the short side, strongest at the rim
   (a circular-cap profile) and clear in the middle, sampling inward along the SDF gradient, on
   continuous corners.
2. **Highlight.** A thin rim (~1 pt) lit by two opposite cones (key from the top left), tinted
   by what's behind, plus a faint wide glow where the bevel faces the light. Never a uniform
   outline.
3. **Adaptive face.** The backdrop's luma is remapped into a narrower range, with a white or
   black fill chosen from the backdrop's own brightness (small panes flip, big ones don't).
4. **Variants.** Regular (blurred, adaptive, for flyouts and anything with text) and clear (a
   light blur and a dimming layer, for the dock and widgets over the wallpaper).
5. **Shadow.** Regular only: soft, low, coloured by the content, stronger over busy content and
   weaker over flat light content. None on clear glass and none on panes flush with the screen's
   edge.
6. **Motion.** Appear by ramping the lensing (materialize), merge nearby panes with a smooth
   minimum (a container's spacing), and respond to a press with a glow from the pointer and a small
   flex.
7. **Cost.** Get the pixels behind once onto the GPU and run one pass per pane, only when
   something changes.

## Sources

Apple:
- [A1] WWDC25 session 219, "Meet Liquid Glass".
  https://developer.apple.com/videos/play/wwdc2025/219/ (transcript; chapters Intro 0:00, Dynamics
  1:29, Adaptivity 6:00, Principles 10:31).
- [A2] WWDC25 session 356, "Get to know the new design system".
  https://developer.apple.com/videos/play/wwdc2025/356/
- [A3] WWDC25 session 323, "Build a SwiftUI app with the new design".
  https://developer.apple.com/videos/play/wwdc2025/323/
- [A4] Human Interface Guidelines, "Materials" (updated September 9, 2025).
  https://developer.apple.com/design/human-interface-guidelines/materials
- [A5] SwiftUI, `glassEffect(_:in:)`. https://developer.apple.com/documentation/swiftui/view/glasseffect(_:in:)
- [A6] SwiftUI, `Glass` (`regular`, `clear`, `identity`, `tint(_:)`, `interactive(_:)`).
  https://developer.apple.com/documentation/swiftui/glass, …/glass/clear
- [A7] SwiftUI, `GlassEffectContainer`. https://developer.apple.com/documentation/swiftui/glasseffectcontainer
- [A8] "Applying Liquid Glass to custom views".
  https://developer.apple.com/documentation/swiftui/applying-liquid-glass-to-custom-views
- [A9] SwiftUI, `glassEffectUnion(id:namespace:)`.
  https://developer.apple.com/documentation/swiftui/view/glasseffectunion(id:namespace:)
- [A10] "Adopting Liquid Glass". https://developer.apple.com/documentation/technologyoverviews/adopting-liquid-glass
- [A11] AppKit, `NSGlassEffectView`. https://developer.apple.com/documentation/appkit/nsglasseffectview
- [A12] Apple Newsroom, "Apple introduces a delightful and elegant new software design" (June 9,
  2025), with its press images.
  https://www.apple.com/newsroom/2025/06/apple-introduces-a-delightful-and-elegant-new-software-design/
- [A13] Apple Newsroom, "macOS Tahoe 26 makes the Mac more capable, productive, and intelligent
  than ever" (June 9, 2025).
  https://www.apple.com/newsroom/2025/06/macos-tahoe-26-makes-the-mac-more-capable-productive-and-intelligent-than-ever/

Others:
- [O1] Chris Feijoo, "Liquid Glass in the browser: refraction with CSS and SVG", kube.io.
  https://kube.io/blog/liquid-glass-css-svg/
- [O2] whynotmake-it, `liquid_glass_renderer` (Flutter): README ("Glint", "Blending shapes",
  "Performance") and `example/tool/apple_match/MODEL_DESIGN.md` (fitted against iOS 27 captures).
  https://github.com/whynotmake-it/flutter_liquid_glass
- [O3] Kyant0, AndroidLiquidGlass (Apache-2.0), `backdrop/.../internal/Shaders.kt` and
  `highlight/HighlightStyle.kt`. https://github.com/Kyant0/AndroidLiquidGlass
- [O4] Shu Ding, liquid-glass (SVG filter). https://github.com/shuding/liquid-glass
- [O5] spacedouut, liquid-glass, a WebGL2 playground of macOS 26's glass shaders "transcribed line
  for line" from QuartzCore, with inputs read off a live `NSGlassEffectView`. **Unverified and
  unlicensed**: used here only as a cross-check on numbers Apple doesn't publish, never copied.
  https://github.com/spacedouut/liquid-glass (README, `app.js` `qcRecipe`).
