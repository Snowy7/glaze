# Studio settings runtime audit

Audited 2026-11-06 against Studio in this shared checkout. The regex `/"(\/[^"\r\n]*)"/g` finds **117** setting-table paths (the brief counted 106 in an earlier revision). The custom-control table below inventories every distinct slash-prefixed literal in `pages/*.rs`, including virtual paths and draft state, so those cannot be mistaken for dead runtime settings. Regenerate with `node crates/shell/scripts/audit-settings.cjs` after a Studio merge.

## Findings

**Three controls were wholly dead for the native shell:** `/look/shape/edge`, `/look/bars/style`, `/look/menus/style`. **Six more were partial/indirect:** `/look/shape/corners`, `/look/text/font`, `/look/text/size`, `/look/text/shadow`, `/look/text/tone`, `/look/text/icons`. Thus **nine controls needed runtime wiring**. For those six, Studio already projected values to bars.json/widgets.json; saying that no runtime ever applied them would be inaccurate. Direct edits to config.json nevertheless had no shell consumer. Font names were discarded/fell back inconsistently; Fluent/One UI and strip/pill shapes clamped or replaced requested radii; widgets ignored the edge and icon controls; menus/panels inherited bars rather than their own family.

All nine now have direct consumers. Existing style/material/appearance/accent/icon-pack controls also resolve directly from config.json on reload. The resolver reads config once per reconciliation; views do no file IO, polling, or recurring restyle work. Specific per-bar/per-monitor radius, font, and border fields and per-widget appearance win. The resolved bar shapes and widget plate_radius feed both rendering/hit masks and glass transport. Bare wallpaper text alone receives tone and text shadow. Text size is the existing 0.5–2 content scale; a projected scale is replaced, never multiplied twice. Lucide uses the kit vectors; Fluent uses the installed system icon font. Missing families fall back to an installed system family.

Baseline labels below refer to the code before this task. **PARTIAL** means a Studio projection or subset of surfaces worked. **DEAD** means no native-shell consumer applied it. Runtime references describe the resulting implementation. Windows styling is owned by the concurrent tweaks/core task; this audit cites its resolver and application boundary without modifying it.

## Settings table

| Document | Path | Baseline | Runtime consumer |
|---|---|---|---|
| config.json | `/static_snapshot` | Live | [crates/wallpaper/src/engine.rs:240](../crates/wallpaper/src/engine.rs#L240) |
| studio.json | `/appearance` | Live | [crates/studio/src/app.rs:1177](../crates/studio/src/app.rs#L1177) |
| config.json | `/engine/fps` | Live | [crates/wallpaper/src/control.rs:106](../crates/wallpaper/src/control.rs#L106); [crates/config/src/engine.rs:89](../crates/config/src/engine.rs#L89) |
| config.json | `/pause_on_battery` | Live | [crates/wallpaper/src/control.rs:108](../crates/wallpaper/src/control.rs#L108) |
| config.json | `/pause_on_fullscreen` | Live | [crates/wallpaper/src/control.rs:108](../crates/wallpaper/src/control.rs#L108) |
| bars.json | `/performance` | Live | [crates/shell/src/bars/motion.rs:61](../crates/shell/src/bars/motion.rs#L61); [crates/shell/src/bars/mod.rs:1898](../crates/shell/src/bars/mod.rs#L1898) |
| bars.json | `/animation/reduceMotion` | Live | [crates/shell/src/bars/motion.rs:61](../crates/shell/src/bars/motion.rs#L61); [crates/shell/src/bars/mod.rs:1898](../crates/shell/src/bars/mod.rs#L1898) |
| config.json | `/desktop_menu/enabled` | Live | [crates/shell/src/main.rs:475](../crates/shell/src/main.rs#L475); [crates/shell/src/desktop_menu/items.rs:145](../crates/shell/src/desktop_menu/items.rs#L145) |
| config.json | `/classic_context_menu` | Live | [crates/core/src/desktop_extras.rs:33](../crates/core/src/desktop_extras.rs#L33) |
| config.json | `/look/material/clarity` | Live | [crates/shell/src/look_settings.rs:58](../crates/shell/src/look_settings.rs#L58); [crates/shell/src/bars/look.rs:126](../crates/shell/src/bars/look.rs#L126); [crates/shell/src/widgets/mod.rs:1164](../crates/shell/src/widgets/mod.rs#L1164) |
| config.json | `/look/material/blur` | Live | [crates/shell/src/look_settings.rs:58](../crates/shell/src/look_settings.rs#L58); [crates/shell/src/bars/look.rs:126](../crates/shell/src/bars/look.rs#L126); [crates/shell/src/widgets/mod.rs:1164](../crates/shell/src/widgets/mod.rs#L1164) |
| config.json | `/look/material/refraction` | Live | [crates/shell/src/look_settings.rs:58](../crates/shell/src/look_settings.rs#L58); [crates/shell/src/bars/look.rs:126](../crates/shell/src/bars/look.rs#L126); [crates/shell/src/widgets/mod.rs:1164](../crates/shell/src/widgets/mod.rs#L1164) |
| config.json | `/look/material/highlight` | Live | [crates/shell/src/look_settings.rs:58](../crates/shell/src/look_settings.rs#L58); [crates/shell/src/bars/look.rs:126](../crates/shell/src/bars/look.rs#L126); [crates/shell/src/widgets/mod.rs:1164](../crates/shell/src/widgets/mod.rs#L1164) |
| config.json | `/look/appearance` | Live | [crates/shell/src/look_settings.rs:58](../crates/shell/src/look_settings.rs#L58); [crates/shell/src/bars/look.rs:126](../crates/shell/src/bars/look.rs#L126); [crates/shell/src/widgets/mod.rs:1164](../crates/shell/src/widgets/mod.rs#L1164) |
| config.json | `/look/text/font` | **PARTIAL** | [crates/shell/src/look_settings.rs:58](../crates/shell/src/look_settings.rs#L58); [crates/shell/src/bars/look.rs:126](../crates/shell/src/bars/look.rs#L126); [crates/shell/src/widgets/mod.rs:1164](../crates/shell/src/widgets/mod.rs#L1164) |
| config.json | `/look/text/size` | **PARTIAL** | [crates/shell/src/look_settings.rs:58](../crates/shell/src/look_settings.rs#L58); [crates/shell/src/bars/look.rs:126](../crates/shell/src/bars/look.rs#L126); [crates/shell/src/widgets/mod.rs:1164](../crates/shell/src/widgets/mod.rs#L1164) |
| config.json | `/look/text/shadow` | **PARTIAL** | [crates/shell/src/look_settings.rs:58](../crates/shell/src/look_settings.rs#L58); [crates/shell/src/bars/look.rs:126](../crates/shell/src/bars/look.rs#L126); [crates/shell/src/widgets/mod.rs:1164](../crates/shell/src/widgets/mod.rs#L1164) |
| config.json | `/look/text/tone` | **PARTIAL** | [crates/shell/src/look_settings.rs:58](../crates/shell/src/look_settings.rs#L58); [crates/shell/src/bars/look.rs:126](../crates/shell/src/bars/look.rs#L126); [crates/shell/src/widgets/mod.rs:1164](../crates/shell/src/widgets/mod.rs#L1164) |
| config.json | `/look/text/icons` | **PARTIAL** | [crates/shell/src/look_settings.rs:58](../crates/shell/src/look_settings.rs#L58); [crates/shell/src/bars/look.rs:126](../crates/shell/src/bars/look.rs#L126); [crates/shell/src/widgets/mod.rs:1164](../crates/shell/src/widgets/mod.rs#L1164) |
| config.json | `/look/shape/corners` | **PARTIAL** | [crates/shell/src/look_settings.rs:58](../crates/shell/src/look_settings.rs#L58); [crates/shell/src/bars/look.rs:126](../crates/shell/src/bars/look.rs#L126); [crates/shell/src/widgets/mod.rs:1164](../crates/shell/src/widgets/mod.rs#L1164) |
| config.json | `/look/shape/edge` | **DEAD** | [crates/shell/src/look_settings.rs:58](../crates/shell/src/look_settings.rs#L58); [crates/shell/src/bars/look.rs:126](../crates/shell/src/bars/look.rs#L126); [crates/shell/src/widgets/mod.rs:1164](../crates/shell/src/widgets/mod.rs#L1164) |
| config.json | `/look/windows/apps` | Live | [crates/theme/src/windows.rs:28](../crates/theme/src/windows.rs#L28); applied by [crates/core/src/styling.rs:133](../crates/core/src/styling.rs#L133) |
| config.json | `/look/windows/corners` | Live | [crates/theme/src/windows.rs:28](../crates/theme/src/windows.rs#L28); applied by [crates/core/src/styling.rs:133](../crates/core/src/styling.rs#L133) |
| config.json | `/look/windows/backdrop` | Live | [crates/theme/src/windows.rs:28](../crates/theme/src/windows.rs#L28); applied by [crates/core/src/styling.rs:133](../crates/core/src/styling.rs#L133) |
| bars.json | `/top/enabled` | Live | [crates/shell/src/bars/mod.rs:1898](../crates/shell/src/bars/mod.rs#L1898) |
| bars.json | `/dock/enabled` | Live | [crates/shell/src/bars/mod.rs:1898](../crates/shell/src/bars/mod.rs#L1898) |
| bars.json | `/taskbar/mode` | Live | [crates/shell/src/bars/mod.rs:2189](../crates/shell/src/bars/mod.rs#L2189) |
| bars.json | `/taskbar/alignment` | Live | [crates/shell/src/bars/mod.rs:2189](../crates/shell/src/bars/mod.rs#L2189) |
| bars.json | `/top/hide_when_maximized` | Live | [crates/shell/src/bars/bar.rs:463](../crates/shell/src/bars/bar.rs#L463) |
| bars.json | `/top/position` | Live | [crates/shell/src/bars/view.rs:857](../crates/shell/src/bars/view.rs#L857) |
| bars.json | `/top/height` | Live | [crates/shell/src/bars/view.rs:857](../crates/shell/src/bars/view.rs#L857) |
| bars.json | `/top/layout` | Live | [crates/shell/src/bars/view.rs:857](../crates/shell/src/bars/view.rs#L857) |
| bars.json | `/top/margin` | Live | [crates/shell/src/bars/view.rs:857](../crates/shell/src/bars/view.rs#L857) |
| bars.json | `/top/padding` | Live | [crates/shell/src/bars/view.rs:857](../crates/shell/src/bars/view.rs#L857) |
| bars.json | `/top/spacing` | Live | [crates/shell/src/bars/view.rs:857](../crates/shell/src/bars/view.rs#L857) |
| bars.json | `/top/clock/seconds` | Live | [crates/bars/src/clock.rs:46](../crates/bars/src/clock.rs#L46); [crates/shell/src/bars/view.rs:857](../crates/shell/src/bars/view.rs#L857) |
| bars.json | `/top/clock/format` | Live | [crates/bars/src/clock.rs:46](../crates/bars/src/clock.rs#L46); [crates/shell/src/bars/view.rs:857](../crates/shell/src/bars/view.rs#L857) |
| bars.json | `/top/monitors` | Live | [crates/shell/src/bars/mod.rs:1898](../crates/shell/src/bars/mod.rs#L1898) |
| bars.json | `/top/look/style` | Live | [crates/shell/src/bars/look.rs:126](../crates/shell/src/bars/look.rs#L126); [crates/shell/src/look_settings.rs:105](../crates/shell/src/look_settings.rs#L105) |
| bars.json | `/top/look/scheme` | Live | [crates/shell/src/bars/look.rs:126](../crates/shell/src/bars/look.rs#L126); [crates/shell/src/look_settings.rs:105](../crates/shell/src/look_settings.rs#L105) |
| bars.json | `/top/look/radius` | Live | [crates/shell/src/bars/look.rs:126](../crates/shell/src/bars/look.rs#L126); [crates/shell/src/look_settings.rs:105](../crates/shell/src/look_settings.rs#L105) |
| bars.json | `/top/look/font/size` | Live | [crates/shell/src/bars/look.rs:126](../crates/shell/src/bars/look.rs#L126); [crates/shell/src/look_settings.rs:105](../crates/shell/src/look_settings.rs#L105) |
| bars.json | `/top/animation/enabled` | Live | [crates/shell/src/bars/motion.rs:61](../crates/shell/src/bars/motion.rs#L61) |
| bars.json | `/top/animation/style` | Live | [crates/shell/src/bars/motion.rs:61](../crates/shell/src/bars/motion.rs#L61) |
| bars.json | `/top/animation/curve` | Live | [crates/shell/src/bars/motion.rs:61](../crates/shell/src/bars/motion.rs#L61) |
| bars.json | `/top/animation/duration` | Live | [crates/shell/src/bars/motion.rs:61](../crates/shell/src/bars/motion.rs#L61) |
| bars.json | `/top/look/family` | Live | [crates/shell/src/bars/look.rs:126](../crates/shell/src/bars/look.rs#L126); [crates/shell/src/look_settings.rs:105](../crates/shell/src/look_settings.rs#L105) |
| bars.json | `/top/look/border/rest/width` | Live | [crates/shell/src/bars/styling.rs:39](../crates/shell/src/bars/styling.rs#L39); [crates/shell/src/bars/view.rs:993](../crates/shell/src/bars/view.rs#L993) |
| bars.json | `/top/look/border/rest/color` | Live | [crates/shell/src/bars/styling.rs:39](../crates/shell/src/bars/styling.rs#L39); [crates/shell/src/bars/view.rs:993](../crates/shell/src/bars/view.rs#L993) |
| bars.json | `/top/look/shadow/rest` | Live | [crates/shell/src/bars/styling.rs:39](../crates/shell/src/bars/styling.rs#L39); [crates/shell/src/bars/view.rs:993](../crates/shell/src/bars/view.rs#L993) |
| bars.json | `/top/look/items/radius` | Live | [crates/shell/src/bars/styling.rs:39](../crates/shell/src/bars/styling.rs#L39); [crates/shell/src/bars/view.rs:993](../crates/shell/src/bars/view.rs#L993) |
| bars.json | `/top/look/items/rest/background` | Live | [crates/shell/src/bars/styling.rs:39](../crates/shell/src/bars/styling.rs#L39); [crates/shell/src/bars/view.rs:993](../crates/shell/src/bars/view.rs#L993) |
| bars.json | `/top/look/items/rest/color` | Live | [crates/shell/src/bars/styling.rs:39](../crates/shell/src/bars/styling.rs#L39); [crates/shell/src/bars/view.rs:993](../crates/shell/src/bars/view.rs#L993) |
| bars.json | `/top/look/items/hover/background` | Live | [crates/shell/src/bars/styling.rs:39](../crates/shell/src/bars/styling.rs#L39); [crates/shell/src/bars/view.rs:993](../crates/shell/src/bars/view.rs#L993) |
| bars.json | `/top/look/items/hover/color` | Live | [crates/shell/src/bars/styling.rs:39](../crates/shell/src/bars/styling.rs#L39); [crates/shell/src/bars/view.rs:993](../crates/shell/src/bars/view.rs#L993) |
| bars.json | `/top/look/items/active/background` | Live | [crates/shell/src/bars/styling.rs:39](../crates/shell/src/bars/styling.rs#L39); [crates/shell/src/bars/view.rs:993](../crates/shell/src/bars/view.rs#L993) |
| bars.json | `/top/look/items/active/color` | Live | [crates/shell/src/bars/styling.rs:39](../crates/shell/src/bars/styling.rs#L39); [crates/shell/src/bars/view.rs:993](../crates/shell/src/bars/view.rs#L993) |
| bars.json | `/top/animation/hover/duration` | Live | [crates/shell/src/bars/motion.rs:61](../crates/shell/src/bars/motion.rs#L61) |
| bars.json | `/top/animation/press/duration` | Live | [crates/shell/src/bars/motion.rs:61](../crates/shell/src/bars/motion.rs#L61) |
| bars.json | `/top/animation/open/duration` | Live | [crates/shell/src/bars/motion.rs:61](../crates/shell/src/bars/motion.rs#L61) |
| bars.json | `/dock/look/family` | Live | [crates/shell/src/bars/look.rs:126](../crates/shell/src/bars/look.rs#L126); [crates/shell/src/look_settings.rs:105](../crates/shell/src/look_settings.rs#L105) |
| bars.json | `/dock/look/border/rest/width` | Live | [crates/shell/src/bars/styling.rs:39](../crates/shell/src/bars/styling.rs#L39); [crates/shell/src/bars/view.rs:993](../crates/shell/src/bars/view.rs#L993) |
| bars.json | `/dock/look/border/rest/color` | Live | [crates/shell/src/bars/styling.rs:39](../crates/shell/src/bars/styling.rs#L39); [crates/shell/src/bars/view.rs:993](../crates/shell/src/bars/view.rs#L993) |
| bars.json | `/dock/look/shadow/rest` | Live | [crates/shell/src/bars/styling.rs:39](../crates/shell/src/bars/styling.rs#L39); [crates/shell/src/bars/view.rs:993](../crates/shell/src/bars/view.rs#L993) |
| bars.json | `/dock/look/items/radius` | Live | [crates/shell/src/bars/styling.rs:39](../crates/shell/src/bars/styling.rs#L39); [crates/shell/src/bars/view.rs:993](../crates/shell/src/bars/view.rs#L993) |
| bars.json | `/dock/look/items/rest/background` | Live | [crates/shell/src/bars/styling.rs:39](../crates/shell/src/bars/styling.rs#L39); [crates/shell/src/bars/view.rs:993](../crates/shell/src/bars/view.rs#L993) |
| bars.json | `/dock/look/items/rest/color` | Live | [crates/shell/src/bars/styling.rs:39](../crates/shell/src/bars/styling.rs#L39); [crates/shell/src/bars/view.rs:993](../crates/shell/src/bars/view.rs#L993) |
| bars.json | `/dock/look/items/hover/background` | Live | [crates/shell/src/bars/styling.rs:39](../crates/shell/src/bars/styling.rs#L39); [crates/shell/src/bars/view.rs:993](../crates/shell/src/bars/view.rs#L993) |
| bars.json | `/dock/look/items/hover/color` | Live | [crates/shell/src/bars/styling.rs:39](../crates/shell/src/bars/styling.rs#L39); [crates/shell/src/bars/view.rs:993](../crates/shell/src/bars/view.rs#L993) |
| bars.json | `/dock/look/items/active/background` | Live | [crates/shell/src/bars/styling.rs:39](../crates/shell/src/bars/styling.rs#L39); [crates/shell/src/bars/view.rs:993](../crates/shell/src/bars/view.rs#L993) |
| bars.json | `/dock/look/items/active/color` | Live | [crates/shell/src/bars/styling.rs:39](../crates/shell/src/bars/styling.rs#L39); [crates/shell/src/bars/view.rs:993](../crates/shell/src/bars/view.rs#L993) |
| bars.json | `/dock/look/indicator/running` | Live | [crates/shell/src/bars/view.rs:1363](../crates/shell/src/bars/view.rs#L1363) |
| bars.json | `/dock/look/indicator/active` | Live | [crates/shell/src/bars/view.rs:1363](../crates/shell/src/bars/view.rs#L1363) |
| bars.json | `/dock/look/indicator/attention` | Live | [crates/shell/src/bars/view.rs:1363](../crates/shell/src/bars/view.rs#L1363) |
| bars.json | `/dock/animation/curve` | Live | [crates/shell/src/bars/motion.rs:61](../crates/shell/src/bars/motion.rs#L61) |
| bars.json | `/dock/animation/duration` | Live | [crates/shell/src/bars/motion.rs:61](../crates/shell/src/bars/motion.rs#L61) |
| bars.json | `/dock/animation/hover/duration` | Live | [crates/shell/src/bars/motion.rs:61](../crates/shell/src/bars/motion.rs#L61) |
| bars.json | `/dock/animation/press/duration` | Live | [crates/shell/src/bars/motion.rs:61](../crates/shell/src/bars/motion.rs#L61) |
| bars.json | `/dock/animation/open/duration` | Live | [crates/shell/src/bars/motion.rs:61](../crates/shell/src/bars/motion.rs#L61) |
| bars.json | `/dock/animation/bar/dock/magnify/duration` | Live | [crates/shell/src/bars/motion.rs:61](../crates/shell/src/bars/motion.rs#L61) |
| bars.json | `/dock/animation/bar/dock/bounce/duration` | Live | [crates/shell/src/bars/motion.rs:61](../crates/shell/src/bars/motion.rs#L61) |
| bars.json | `/dock/animation/bar/dock/attention/duration` | Live | [crates/shell/src/bars/motion.rs:61](../crates/shell/src/bars/motion.rs#L61) |
| bars.json | `/dock/style` | Live | [crates/shell/src/bars/view.rs:993](../crates/shell/src/bars/view.rs#L993) |
| bars.json | `/dock/position` | Live | [crates/shell/src/bars/view.rs:993](../crates/shell/src/bars/view.rs#L993) |
| bars.json | `/dock/size` | Live | [crates/shell/src/bars/view.rs:993](../crates/shell/src/bars/view.rs#L993) |
| bars.json | `/dock/autohide` | Live | [crates/shell/src/bars/bar.rs:469](../crates/shell/src/bars/bar.rs#L469) |
| bars.json | `/dock/hide_when_maximized` | Live | [crates/shell/src/bars/bar.rs:463](../crates/shell/src/bars/bar.rs#L463) |
| bars.json | `/dock/magnify/enabled` | Live | [crates/shell/src/bars/mod.rs:1898](../crates/shell/src/bars/mod.rs#L1898) |
| bars.json | `/dock/margin` | Live | [crates/shell/src/bars/view.rs:993](../crates/shell/src/bars/view.rs#L993) |
| bars.json | `/dock/padding` | Live | [crates/shell/src/bars/view.rs:993](../crates/shell/src/bars/view.rs#L993) |
| bars.json | `/dock/spacing` | Live | [crates/shell/src/bars/view.rs:993](../crates/shell/src/bars/view.rs#L993) |
| bars.json | `/dock/magnify/scale` | Live | [crates/shell/src/bars/view.rs:993](../crates/shell/src/bars/view.rs#L993) |
| bars.json | `/dock/magnify/range` | Live | [crates/shell/src/bars/view.rs:993](../crates/shell/src/bars/view.rs#L993) |
| bars.json | `/dock/align` | Live | [crates/shell/src/bars/view.rs:993](../crates/shell/src/bars/view.rs#L993) |
| bars.json | `/dock/titles` | Live | [crates/shell/src/bars/view.rs:993](../crates/shell/src/bars/view.rs#L993) |
| bars.json | `/dock/button` | Live | [crates/shell/src/bars/view.rs:993](../crates/shell/src/bars/view.rs#L993) |
| bars.json | `/dock/clock/seconds` | Live | [crates/bars/src/clock.rs:46](../crates/bars/src/clock.rs#L46); [crates/shell/src/bars/view.rs:857](../crates/shell/src/bars/view.rs#L857) |
| bars.json | `/dock/running` | Live | [crates/shell/src/bars/services.rs:488](../crates/shell/src/bars/services.rs#L488) |
| bars.json | `/dock/split` | Live | [crates/shell/src/bars/services.rs:488](../crates/shell/src/bars/services.rs#L488) |
| bars.json | `/dock/counter` | Live | [crates/shell/src/bars/view.rs:993](../crates/shell/src/bars/view.rs#L993) |
| bars.json | `/dock/preview/enabled` | Live | [crates/shell/src/bars/mod.rs:1290](../crates/shell/src/bars/mod.rs#L1290) |
| bars.json | `/dock/preview/delay` | Live | [crates/shell/src/bars/mod.rs:1290](../crates/shell/src/bars/mod.rs#L1290) |
| bars.json | `/dock/bounce` | Live | [crates/shell/src/bars/bar.rs:627](../crates/shell/src/bars/bar.rs#L627) |
| bars.json | `/dock/attention` | Live | [crates/shell/src/bars/bar.rs:627](../crates/shell/src/bars/bar.rs#L627) |
| bars.json | `/dock/indicator` | Live | [crates/shell/src/bars/view.rs:993](../crates/shell/src/bars/view.rs#L993) |
| bars.json | `/dock/monitors` | Live | [crates/shell/src/bars/mod.rs:1898](../crates/shell/src/bars/mod.rs#L1898) |
| bars.json | `/dock/look/style` | Live | [crates/shell/src/bars/look.rs:126](../crates/shell/src/bars/look.rs#L126); [crates/shell/src/look_settings.rs:105](../crates/shell/src/look_settings.rs#L105) |
| bars.json | `/dock/look/radius` | Live | [crates/shell/src/bars/look.rs:126](../crates/shell/src/bars/look.rs#L126); [crates/shell/src/look_settings.rs:105](../crates/shell/src/look_settings.rs#L105) |
| bars.json | `/dock/animation/enabled` | Live | [crates/shell/src/bars/motion.rs:61](../crates/shell/src/bars/motion.rs#L61) |
| bars.json | `/dock/animation/style` | Live | [crates/shell/src/bars/motion.rs:61](../crates/shell/src/bars/motion.rs#L61) |
| bars.json | `/launcher/layout` | Live | [crates/shell/src/flyouts/launcher.rs:280](../crates/shell/src/flyouts/launcher.rs#L280); [crates/shell/src/flyouts/mod.rs:588](../crates/shell/src/flyouts/mod.rs#L588) |
| bars.json | `/launcher/files` | Live | [crates/shell/src/flyouts/launcher.rs:280](../crates/shell/src/flyouts/launcher.rs#L280); [crates/shell/src/flyouts/mod.rs:588](../crates/shell/src/flyouts/mod.rs#L588) |
| bars.json | `/launcher/winKey` | Live | [crates/shell/src/flyouts/launcher.rs:280](../crates/shell/src/flyouts/launcher.rs#L280); [crates/shell/src/flyouts/mod.rs:588](../crates/shell/src/flyouts/mod.rs#L588) |
| bars.json | `/calendar/firstDay` | Live | [crates/shell/src/flyouts/calendar.rs:38](../crates/shell/src/flyouts/calendar.rs#L38) |
| bars.json | `/audio/switchOutput` | Live | [crates/shell/src/flyouts/mixer.rs:59](../crates/shell/src/flyouts/mixer.rs#L59) |
| bars.json | `/tray/host` | Live | [crates/shell/src/bars/mod.rs:2159](../crates/shell/src/bars/mod.rs#L2159) |
| widgets.json | `/overlay` | Live | [crates/shell/src/widgets/mod.rs:1113](../crates/shell/src/widgets/mod.rs#L1113); [crates/shell/src/widgets/views/mod.rs:212](../crates/shell/src/widgets/views/mod.rs#L212) |

## Custom controls and virtual paths

| Source | Literal/template | Consumer or disposition |
|---|---|---|
| bars.rs:141 | `/{}/design` | [crates/shell/src/bars/mod.rs:1898](../crates/shell/src/bars/mod.rs#L1898); [crates/shell/src/bars/view.rs:857](../crates/shell/src/bars/view.rs#L857) (template expanded by Studio) |
| bars.rs:143 | `/{}/enabled` | [crates/shell/src/bars/mod.rs:1898](../crates/shell/src/bars/mod.rs#L1898); [crates/shell/src/bars/view.rs:857](../crates/shell/src/bars/view.rs#L857) (template expanded by Studio) |
| bars.rs:152 | `/dock/style` | [crates/shell/src/bars/view.rs:993](../crates/shell/src/bars/view.rs#L993) |
| bars.rs:153 | `/dock/magnify/enabled` | [crates/shell/src/bars/mod.rs:1898](../crates/shell/src/bars/mod.rs#L1898) |
| bars.rs:160 | `/preset` | [crates/bars/src/config.rs:1064](../crates/bars/src/config.rs#L1064); [crates/shell/src/bars/mod.rs:1898](../crates/shell/src/bars/mod.rs#L1898) |
| bars.rs:250 | `/launcher/layout` | [crates/shell/src/flyouts/launcher.rs:280](../crates/shell/src/flyouts/launcher.rs#L280); [crates/shell/src/flyouts/mod.rs:588](../crates/shell/src/flyouts/mod.rs#L588) |
| bars.rs:261 | `/overrides` | [crates/bars/src/config.rs:1105](../crates/bars/src/config.rs#L1105); [crates/shell/src/bars/mod.rs:1898](../crates/shell/src/bars/mod.rs#L1898) |
| bars.rs:268 | `/overrides/{}` | [crates/bars/src/config.rs:1105](../crates/bars/src/config.rs#L1105); [crates/shell/src/bars/mod.rs:1898](../crates/shell/src/bars/mod.rs#L1898) |
| bars.rs:421 | `/{}/look/transparent` | [crates/shell/src/bars/look.rs:126](../crates/shell/src/bars/look.rs#L126); [crates/shell/src/look_settings.rs:105](../crates/shell/src/look_settings.rs#L105) (top/dock) |
| bars.rs:439 | `/dock/extras` | [crates/shell/src/bars/view.rs:993](../crates/shell/src/bars/view.rs#L993) |
| bars.rs:565 | `/modules/{}` | [crates/shell/src/bars/view.rs:361](../crates/shell/src/bars/view.rs#L361); [crates/shell/src/bars/plugin_modules.rs:51](../crates/shell/src/bars/plugin_modules.rs#L51) |
| bars.rs:614 | `/module_args/{}` | [crates/shell/src/bars/view.rs:361](../crates/shell/src/bars/view.rs#L361); [crates/shell/src/bars/plugin_modules.rs:51](../crates/shell/src/bars/plugin_modules.rs#L51) |
| bars.rs:697 | `/calendar/zones` | [crates/shell/src/flyouts/calendar.rs:38](../crates/shell/src/flyouts/calendar.rs#L38) |
| bars.rs:709 | `/new_zone` | Studio local state/action or forwarding alias; **not a runtime config path** |
| bars.rs:750 | `/{bar}/{field}` | [crates/shell/src/bars/mod.rs:1898](../crates/shell/src/bars/mod.rs#L1898); [crates/shell/src/bars/view.rs:857](../crates/shell/src/bars/view.rs#L857) (template expanded by Studio) |
| bars.rs:765 | `/{}/{list}` | [crates/shell/src/bars/mod.rs:1898](../crates/shell/src/bars/mod.rs#L1898); [crates/shell/src/bars/view.rs:857](../crates/shell/src/bars/view.rs#L857) (template expanded by Studio) |
| bars.rs:774 | `/{}/{}` | [crates/shell/src/bars/mod.rs:1898](../crates/shell/src/bars/mod.rs#L1898); [crates/shell/src/bars/view.rs:857](../crates/shell/src/bars/view.rs#L857) (template expanded by Studio) |
| bars.rs:806 | `/{}/look` | [crates/shell/src/bars/mod.rs:1898](../crates/shell/src/bars/mod.rs#L1898); [crates/shell/src/bars/view.rs:857](../crates/shell/src/bars/view.rs#L857) (template expanded by Studio) |
| bars.rs:807 | `/{}/animation` | [crates/shell/src/bars/mod.rs:1898](../crates/shell/src/bars/mod.rs#L1898); [crates/shell/src/bars/view.rs:857](../crates/shell/src/bars/view.rs#L857) (template expanded by Studio) |
| bars.rs:939 | `/add_module_list` | Studio local state/action or forwarding alias; **not a runtime config path** |
| bars.rs:990 | `/new_custom` | Studio local state/action or forwarding alias; **not a runtime config path** |
| first_run.rs:244 | `/first_run_done` | [crates/studio/src/app.rs:1646](../crates/studio/src/app.rs#L1646) (Studio onboarding state) |
| first_run.rs:265 | `/look/style` | [crates/shell/src/look_settings.rs:58](../crates/shell/src/look_settings.rs#L58); [crates/shell/src/bars/look.rs:126](../crates/shell/src/bars/look.rs#L126); [crates/shell/src/widgets/mod.rs:1164](../crates/shell/src/widgets/mod.rs#L1164) |
| first_run.rs:266 | `/preset` | [crates/bars/src/config.rs:1064](../crates/bars/src/config.rs#L1064); [crates/shell/src/bars/mod.rs:1898](../crates/shell/src/bars/mod.rs#L1898) |
| first_run.rs:267 | `/instances` | [crates/shell/src/widgets/mod.rs:1113](../crates/shell/src/widgets/mod.rs#L1113); [crates/shell/src/widgets/views/mod.rs:212](../crates/shell/src/widgets/views/mod.rs#L212) |
| first_run.rs:271 | `/{bar}/{field}` | [crates/shell/src/bars/mod.rs:1898](../crates/shell/src/bars/mod.rs#L1898); [crates/shell/src/bars/view.rs:857](../crates/shell/src/bars/view.rs#L857) (template expanded by Studio) |
| first_run.rs:275 | `/dock/enabled` | [crates/shell/src/bars/mod.rs:1898](../crates/shell/src/bars/mod.rs#L1898) |
| first_run.rs:279 | `/{bar}/enabled` | [crates/shell/src/bars/mod.rs:1898](../crates/shell/src/bars/mod.rs#L1898); [crates/shell/src/bars/view.rs:857](../crates/shell/src/bars/view.rs#L857) (template expanded by Studio) |
| first_run.rs:282 | `/taskbar/mode` | [crates/shell/src/bars/mod.rs:2189](../crates/shell/src/bars/mod.rs#L2189) |
| first_run.rs:284 | `/theme` | [crates/wallpaper/src/control.rs:107](../crates/wallpaper/src/control.rs#L107) |
| home.rs:29 | `/paused` | [crates/wallpaper/src/control.rs:108](../crates/wallpaper/src/control.rs#L108) |
| home.rs:136 | `/recent_wallpapers` | [crates/studio/src/pages/wallpapers.rs:471](../crates/studio/src/pages/wallpapers.rs#L471) (Studio library history) |
| home.rs:186 | `/engine/gpu` | [crates/wallpaper/src/control.rs:106](../crates/wallpaper/src/control.rs#L106); [crates/config/src/engine.rs:89](../crates/config/src/engine.rs#L89) |
| home.rs:191 | `/engine/fps` | [crates/wallpaper/src/control.rs:106](../crates/wallpaper/src/control.rs#L106); [crates/config/src/engine.rs:89](../crates/config/src/engine.rs#L89) |
| home.rs:225 | `/desktop_menu/enabled` | [crates/shell/src/main.rs:475](../crates/shell/src/main.rs#L475); [crates/shell/src/desktop_menu/items.rs:145](../crates/shell/src/desktop_menu/items.rs#L145) |
| integrations.rs:65 | `/notifications/showText` | [crates/shell/src/integrations/mod.rs:266](../crates/shell/src/integrations/mod.rs#L266) |
| integrations.rs:66 | `/notifications/liveVerified` | [crates/shell/src/integrations/mod.rs:266](../crates/shell/src/integrations/mod.rs#L266) |
| integrations.rs:99 | `/{s}/enabled` | [crates/shell/src/bars/mod.rs:1898](../crates/shell/src/bars/mod.rs#L1898); [crates/shell/src/bars/view.rs:857](../crates/shell/src/bars/view.rs#L857) (template expanded by Studio) |
| integrations.rs:116 | `/top/{l}` | [crates/shell/src/bars/view.rs:857](../crates/shell/src/bars/view.rs#L857) |
| integrations.rs:248 | `/integration_off/{i}` | Studio local state/action or forwarding alias; **not a runtime config path** |
| integrations.rs:250 | `/integration_on/{i}` | Studio local state/action or forwarding alias; **not a runtime config path** |
| integrations.rs:266 | `/integration_module/{i}` | Studio local state/action or forwarding alias; **not a runtime config path** |
| integrations.rs:281 | `/integration_secret/{i}` | Studio local state/action or forwarding alias; **not a runtime config path** |
| integrations.rs:284 | `/integration_forget/{i}` | Studio local state/action or forwarding alias; **not a runtime config path** |
| integrations.rs:292 | `/integration_access` | Studio local state/action or forwarding alias; **not a runtime config path** |
| integrations.rs:323 | `/integration_on/` | Studio local state/action or forwarding alias; **not a runtime config path** |
| integrations.rs:325 | `/integration_off/` | Studio local state/action or forwarding alias; **not a runtime config path** |
| integrations.rs:327 | `/integration_module/` | Studio local state/action or forwarding alias; **not a runtime config path** |
| integrations.rs:331 | `/integration_secret/` | Studio local state/action or forwarding alias; **not a runtime config path** |
| integrations.rs:334 | `/integration_forget/` | Studio local state/action or forwarding alias; **not a runtime config path** |
| integrations.rs:352 | `/top/{list}` | [crates/shell/src/bars/view.rs:857](../crates/shell/src/bars/view.rs#L857) |
| integrations.rs:372 | `/{section}/enabled` | [crates/shell/src/bars/mod.rs:1898](../crates/shell/src/bars/mod.rs#L1898); [crates/shell/src/bars/view.rs:857](../crates/shell/src/bars/view.rs#L857) (template expanded by Studio) |
| look.rs:42 | `/look/style` | [crates/shell/src/look_settings.rs:58](../crates/shell/src/look_settings.rs#L58); [crates/shell/src/bars/look.rs:126](../crates/shell/src/bars/look.rs#L126); [crates/shell/src/widgets/mod.rs:1164](../crates/shell/src/widgets/mod.rs#L1164) |
| look.rs:53 | `/look/appearance` | [crates/shell/src/look_settings.rs:58](../crates/shell/src/look_settings.rs#L58); [crates/shell/src/bars/look.rs:126](../crates/shell/src/bars/look.rs#L126); [crates/shell/src/widgets/mod.rs:1164](../crates/shell/src/widgets/mod.rs#L1164) |
| look.rs:58 | `/look/accent` | [crates/shell/src/look_settings.rs:58](../crates/shell/src/look_settings.rs#L58); [crates/shell/src/bars/look.rs:126](../crates/shell/src/bars/look.rs#L126); [crates/shell/src/widgets/mod.rs:1164](../crates/shell/src/widgets/mod.rs#L1164) |
| look.rs:83 | `/look_scope` | Studio local state/action or forwarding alias; **not a runtime config path** |
| look.rs:163 | `/look/material/tint` | [crates/shell/src/look_settings.rs:58](../crates/shell/src/look_settings.rs#L58); [crates/shell/src/bars/look.rs:126](../crates/shell/src/bars/look.rs#L126); [crates/shell/src/widgets/mod.rs:1164](../crates/shell/src/widgets/mod.rs#L1164) |
| look.rs:196 | `/look/{family}/style` | [crates/shell/src/look_settings.rs:58](../crates/shell/src/look_settings.rs#L58); [crates/shell/src/bars/look.rs:126](../crates/shell/src/bars/look.rs#L126); [crates/shell/src/widgets/mod.rs:1164](../crates/shell/src/widgets/mod.rs#L1164) |
| look.rs:219 | `/look/text/icon_pack` | [crates/shell/src/bars/mod.rs:2210](../crates/shell/src/bars/mod.rs#L2210) |
| look.rs:303 | `/palette/accent` | Studio local state/action or forwarding alias; **not a runtime config path** |
| look.rs:350 | `/look/windows` | [crates/theme/src/windows.rs:28](../crates/theme/src/windows.rs#L28); applied by [crates/core/src/styling.rs:133](../crates/core/src/styling.rs#L133) |
| look.rs:373 | `/look/windows/accent` | [crates/theme/src/windows.rs:28](../crates/theme/src/windows.rs#L28); applied by [crates/core/src/styling.rs:133](../crates/core/src/styling.rs#L133) |
| look.rs:382 | `/windows_null/accent` | Studio local state/action or forwarding alias; **not a runtime config path** |
| look.rs:400 | `/look/windows/cursors` | [crates/theme/src/windows.rs:28](../crates/theme/src/windows.rs#L28); applied by [crates/core/src/styling.rs:133](../crates/core/src/styling.rs#L133) |
| look.rs:402 | `/windows_null/cursors` | Studio local state/action or forwarding alias; **not a runtime config path** |
| look.rs:419 | `/look/windows/border` | [crates/theme/src/windows.rs:28](../crates/theme/src/windows.rs#L28); applied by [crates/core/src/styling.rs:133](../crates/core/src/styling.rs#L133) |
| look.rs:420 | `/look/windows/caption` | [crates/theme/src/windows.rs:28](../crates/theme/src/windows.rs#L28); applied by [crates/core/src/styling.rs:133](../crates/core/src/styling.rs#L133) |
| look.rs:421 | `/look/windows/caption_text` | [crates/theme/src/windows.rs:28](../crates/theme/src/windows.rs#L28); applied by [crates/core/src/styling.rs:133](../crates/core/src/styling.rs#L133) |
| look.rs:435 | `/windows_null/{}` | Studio local state/action or forwarding alias; **not a runtime config path** |
| look.rs:510 | `/theme:{dir}{path}` | [crates/theme/src/lib.rs:493](../crates/theme/src/lib.rs#L493); [crates/shell/src/look_settings.rs:58](../crates/shell/src/look_settings.rs#L58) (theme aliases expand in Studio before writing) |
| look.rs:517 | `/windows/corners` | [crates/theme/src/windows.rs:28](../crates/theme/src/windows.rs#L28); applied by [crates/core/src/styling.rs:133](../crates/core/src/styling.rs#L133) |
| look.rs:518 | `/windows/backdrop` | [crates/theme/src/windows.rs:28](../crates/theme/src/windows.rs#L28); applied by [crates/core/src/styling.rs:133](../crates/core/src/styling.rs#L133) |
| look.rs:519 | `/windows/dark_mode` | [crates/theme/src/windows.rs:28](../crates/theme/src/windows.rs#L28); applied by [crates/core/src/styling.rs:133](../crates/core/src/styling.rs#L133) |
| look.rs:520 | `/windows/accent` | [crates/theme/src/windows.rs:28](../crates/theme/src/windows.rs#L28); applied by [crates/core/src/styling.rs:133](../crates/core/src/styling.rs#L133) |
| look.rs:528 | `/widgets/glass/tint` | [crates/theme/src/lib.rs:493](../crates/theme/src/lib.rs#L493); [crates/shell/src/look_settings.rs:58](../crates/shell/src/look_settings.rs#L58) (theme aliases expand in Studio before writing) |
| look.rs:529 | `/@both/glass/clarity` | [crates/theme/src/lib.rs:493](../crates/theme/src/lib.rs#L493); [crates/shell/src/look_settings.rs:58](../crates/shell/src/look_settings.rs#L58) (theme aliases expand in Studio before writing) |
| look.rs:530 | `/widgets/glass/blur` | [crates/theme/src/lib.rs:493](../crates/theme/src/lib.rs#L493); [crates/shell/src/look_settings.rs:58](../crates/shell/src/look_settings.rs#L58) (theme aliases expand in Studio before writing) |
| look.rs:531 | `/@both/glass/blur` | [crates/theme/src/lib.rs:493](../crates/theme/src/lib.rs#L493); [crates/shell/src/look_settings.rs:58](../crates/shell/src/look_settings.rs#L58) (theme aliases expand in Studio before writing) |
| look.rs:532 | `/widgets/glass/refraction` | [crates/theme/src/lib.rs:493](../crates/theme/src/lib.rs#L493); [crates/shell/src/look_settings.rs:58](../crates/shell/src/look_settings.rs#L58) (theme aliases expand in Studio before writing) |
| look.rs:533 | `/@both/glass/refraction` | [crates/theme/src/lib.rs:493](../crates/theme/src/lib.rs#L493); [crates/shell/src/look_settings.rs:58](../crates/shell/src/look_settings.rs#L58) (theme aliases expand in Studio before writing) |
| look.rs:534 | `/widgets/glass/highlight` | [crates/theme/src/lib.rs:493](../crates/theme/src/lib.rs#L493); [crates/shell/src/look_settings.rs:58](../crates/shell/src/look_settings.rs#L58) (theme aliases expand in Studio before writing) |
| look.rs:535 | `/@both/glass/highlight` | [crates/theme/src/lib.rs:493](../crates/theme/src/lib.rs#L493); [crates/shell/src/look_settings.rs:58](../crates/shell/src/look_settings.rs#L58) (theme aliases expand in Studio before writing) |
| look.rs:538 | `/widgets/appearance/text` | [crates/theme/src/lib.rs:493](../crates/theme/src/lib.rs#L493); [crates/shell/src/look_settings.rs:58](../crates/shell/src/look_settings.rs#L58) (theme aliases expand in Studio before writing) |
| look.rs:538 | `/@both/appearance/text` | [crates/theme/src/lib.rs:493](../crates/theme/src/lib.rs#L493); [crates/shell/src/look_settings.rs:58](../crates/shell/src/look_settings.rs#L58) (theme aliases expand in Studio before writing) |
| look.rs:542 | `/@both/appearance/accent` | [crates/theme/src/lib.rs:493](../crates/theme/src/lib.rs#L493); [crates/shell/src/look_settings.rs:58](../crates/shell/src/look_settings.rs#L58) (theme aliases expand in Studio before writing) |
| look.rs:542 | `/widgets/appearance/accent` | [crates/theme/src/lib.rs:493](../crates/theme/src/lib.rs#L493); [crates/shell/src/look_settings.rs:58](../crates/shell/src/look_settings.rs#L58) (theme aliases expand in Studio before writing) |
| look.rs:545 | `/@both/appearance/tint` | [crates/theme/src/lib.rs:493](../crates/theme/src/lib.rs#L493); [crates/shell/src/look_settings.rs:58](../crates/shell/src/look_settings.rs#L58) (theme aliases expand in Studio before writing) |
| look.rs:545 | `/widgets/appearance/tint` | [crates/theme/src/lib.rs:493](../crates/theme/src/lib.rs#L493); [crates/shell/src/look_settings.rs:58](../crates/shell/src/look_settings.rs#L58) (theme aliases expand in Studio before writing) |
| look.rs:626 | `/look` | [crates/shell/src/look_settings.rs:58](../crates/shell/src/look_settings.rs#L58) (reset removes the runtime layer) |
| look.rs:647 | `/look/material/{field}` | [crates/shell/src/look_settings.rs:58](../crates/shell/src/look_settings.rs#L58); [crates/shell/src/bars/look.rs:126](../crates/shell/src/bars/look.rs#L126); [crates/shell/src/widgets/mod.rs:1164](../crates/shell/src/widgets/mod.rs#L1164) |
| look.rs:649 | `/look/shape/corners` | [crates/shell/src/look_settings.rs:58](../crates/shell/src/look_settings.rs#L58); [crates/shell/src/bars/look.rs:126](../crates/shell/src/bars/look.rs#L126); [crates/shell/src/widgets/mod.rs:1164](../crates/shell/src/widgets/mod.rs#L1164) |
| look.rs:656 | `/windows_null/` | Studio local state/action or forwarding alias; **not a runtime config path** |
| look.rs:657 | `/look/windows/{field}` | [crates/theme/src/windows.rs:28](../crates/theme/src/windows.rs#L28); applied by [crates/core/src/styling.rs:133](../crates/core/src/styling.rs#L133) |
| mod.rs:149 | `/plugins/` | Studio local state/action or forwarding alias; **not a runtime config path** |
| mod.rs:152 | `/theme:` | [crates/theme/src/lib.rs:493](../crates/theme/src/lib.rs#L493); [crates/shell/src/look_settings.rs:58](../crates/shell/src/look_settings.rs#L58) (theme aliases expand in Studio before writing) |
| mod.rs:154 | `/` | Studio local state/action or forwarding alias; **not a runtime config path** |
| mod.rs:155 | `/{pointer}` | [crates/shell/src/bars/mod.rs:1898](../crates/shell/src/bars/mod.rs#L1898); [crates/shell/src/bars/view.rs:857](../crates/shell/src/bars/view.rs#L857) (template expanded by Studio) |
| mod.rs:165 | `/add_module_list` | Studio local state/action or forwarding alias; **not a runtime config path** |
| mod.rs:165 | `/new_custom` | Studio local state/action or forwarding alias; **not a runtime config path** |
| mod.rs:165 | `/menu_label` | Studio local state/action or forwarding alias; **not a runtime config path** |
| mod.rs:165 | `/menu_command` | Studio local state/action or forwarding alias; **not a runtime config path** |
| mod.rs:168 | `/list_new` | Studio local state/action or forwarding alias; **not a runtime config path** |
| mod.rs:171 | `/new_zone` | Studio local state/action or forwarding alias; **not a runtime config path** |
| mod.rs:173 | `/calendar/zones` | [crates/shell/src/flyouts/calendar.rs:38](../crates/shell/src/flyouts/calendar.rs#L38) |
| mod.rs:178 | `/look_scope` | Studio local state/action or forwarding alias; **not a runtime config path** |
| mod.rs:181 | `/add_fit` | Studio local state/action or forwarding alias; **not a runtime config path** |
| mod.rs:184 | `/add_args_preset` | Studio local state/action or forwarding alias; **not a runtime config path** |
| mod.rs:187 | `/module_args/` | [crates/shell/src/bars/view.rs:361](../crates/shell/src/bars/view.rs#L361); [crates/shell/src/bars/plugin_modules.rs:51](../crates/shell/src/bars/plugin_modules.rs#L51) |
| mod.rs:192 | `/modules/{module}/args` | [crates/shell/src/bars/view.rs:361](../crates/shell/src/bars/view.rs#L361); [crates/shell/src/bars/plugin_modules.rs:51](../crates/shell/src/bars/plugin_modules.rs#L51) |
| mod.rs:194 | `/app_args/` | Studio local state/action or forwarding alias; **not a runtime config path** |
| mod.rs:198 | `/theme:{}/wallpaper/args` | [crates/theme/src/lib.rs:493](../crates/theme/src/lib.rs#L493); [crates/shell/src/look_settings.rs:58](../crates/shell/src/look_settings.rs#L58) (theme aliases expand in Studio before writing) |
| plugins.rs:76 | `/plugins/tab` | Studio local state/action or forwarding alias; **not a runtime config path** |
| plugins.rs:84 | `/plugins/file` | Studio local state/action or forwarding alias; **not a runtime config path** |
| plugins.rs:85 | `/plugins/folder` | Studio local state/action or forwarding alias; **not a runtime config path** |
| plugins.rs:192 | `/plugins/select` | Studio local state/action or forwarding alias; **not a runtime config path** |
| plugins.rs:222 | `/plugins/enable` | Studio local state/action or forwarding alias; **not a runtime config path** |
| plugins.rs:222 | `/plugins/disable` | Studio local state/action or forwarding alias; **not a runtime config path** |
| plugins.rs:224 | `/plugins/uninstall` | Studio local state/action or forwarding alias; **not a runtime config path** |
| plugins.rs:301 | `/plugins/setting/{}/{}` | [crates/plugins/src/main.rs:48](../crates/plugins/src/main.rs#L48) (plugin-owned settings and watcher) |
| plugins.rs:475 | `/plugins/consent` | Studio local state/action or forwarding alias; **not a runtime config path** |
| plugins.rs:506 | `/plugins/uninstall-confirmed` | Studio local state/action or forwarding alias; **not a runtime config path** |
| plugins.rs:526 | `/plugins/` | Studio local state/action or forwarding alias; **not a runtime config path** |
| plugins.rs:613 | `/` | Studio local state/action or forwarding alias; **not a runtime config path** |
| plugins.rs:709 | `/plugins/setting/t3/endpoint` | [crates/plugins/src/main.rs:48](../crates/plugins/src/main.rs#L48) (plugin-owned settings and watcher) |
| settings.rs:118 | `/engine/gpu` | [crates/wallpaper/src/control.rs:106](../crates/wallpaper/src/control.rs#L106); [crates/config/src/engine.rs:89](../crates/config/src/engine.rs#L89) |
| settings.rs:153 | `/engine/fps` | [crates/wallpaper/src/control.rs:106](../crates/wallpaper/src/control.rs#L106); [crates/config/src/engine.rs:89](../crates/config/src/engine.rs#L89) |
| settings.rs:172 | `/engine/wallpapers` | [crates/wallpaper/src/control.rs:106](../crates/wallpaper/src/control.rs#L106); [crates/config/src/engine.rs:89](../crates/config/src/engine.rs#L89) |
| settings.rs:189 | `/desktop_menu/enabled` | [crates/shell/src/main.rs:475](../crates/shell/src/main.rs#L475); [crates/shell/src/desktop_menu/items.rs:145](../crates/shell/src/desktop_menu/items.rs#L145) |
| settings.rs:263 | `/menu_label` | Studio local state/action or forwarding alias; **not a runtime config path** |
| settings.rs:264 | `/menu_command` | Studio local state/action or forwarding alias; **not a runtime config path** |
| settings.rs:309 | `/desktop_menu/items` | [crates/shell/src/main.rs:475](../crates/shell/src/main.rs#L475); [crates/shell/src/desktop_menu/items.rs:145](../crates/shell/src/desktop_menu/items.rs#L145) |
| settings.rs:502 | `/look` | [crates/shell/src/look_settings.rs:58](../crates/shell/src/look_settings.rs#L58) (reset removes the runtime layer) |
| wallpapers.rs:53 | `/engine/wallpapers/{}` | [crates/wallpaper/src/control.rs:106](../crates/wallpaper/src/control.rs#L106); [crates/config/src/engine.rs:89](../crates/config/src/engine.rs#L89) |
| wallpapers.rs:247 | `/engine/gpu` | [crates/wallpaper/src/control.rs:106](../crates/wallpaper/src/control.rs#L106); [crates/config/src/engine.rs:89](../crates/config/src/engine.rs#L89) |
| wallpapers.rs:285 | `/wallpaper/fit` | [crates/theme/src/lib.rs:493](../crates/theme/src/lib.rs#L493); [crates/shell/src/look_settings.rs:58](../crates/shell/src/look_settings.rs#L58) (theme aliases expand in Studio before writing) |
| wallpapers.rs:304 | `/wallpaper/interactive` | [crates/theme/src/lib.rs:493](../crates/theme/src/lib.rs#L493); [crates/shell/src/look_settings.rs:58](../crates/shell/src/look_settings.rs#L58) (theme aliases expand in Studio before writing) |
| wallpapers.rs:312 | `/wallpaper/args` | [crates/theme/src/lib.rs:493](../crates/theme/src/lib.rs#L493); [crates/shell/src/look_settings.rs:58](../crates/shell/src/look_settings.rs#L58) (theme aliases expand in Studio before writing) |
| wallpapers.rs:319 | `/app_args/{}` | Studio local state/action or forwarding alias; **not a runtime config path** |
| wallpapers.rs:408 | `/theme:{}{path}` | [crates/theme/src/lib.rs:493](../crates/theme/src/lib.rs#L493); [crates/shell/src/look_settings.rs:58](../crates/shell/src/look_settings.rs#L58) (theme aliases expand in Studio before writing) |
| wallpapers.rs:470 | `/theme` | [crates/wallpaper/src/control.rs:107](../crates/wallpaper/src/control.rs#L107) |
| wallpapers.rs:471 | `/recent_wallpapers` | [crates/studio/src/pages/wallpapers.rs:471](../crates/studio/src/pages/wallpapers.rs#L471) (Studio library history) |
| wallpapers.rs:497 | `/add_fit` | Studio local state/action or forwarding alias; **not a runtime config path** |
| wallpapers.rs:669 | `/add_args_preset` | Studio local state/action or forwarding alias; **not a runtime config path** |
| widgets.rs:210 | `/instances/{index}` | [crates/shell/src/widgets/mod.rs:1113](../crates/shell/src/widgets/mod.rs#L1113); [crates/shell/src/widgets/views/mod.rs:212](../crates/shell/src/widgets/views/mod.rs#L212) |
| widgets.rs:403 | `/list_new{}` | Studio local state/action or forwarding alias; **not a runtime config path** |
| widgets.rs:579 | `/instances/{n}` | [crates/shell/src/widgets/mod.rs:1113](../crates/shell/src/widgets/mod.rs#L1113); [crates/shell/src/widgets/views/mod.rs:212](../crates/shell/src/widgets/views/mod.rs#L212) |
| widgets.rs:586 | `/list_new{path}` | Studio local state/action or forwarding alias; **not a runtime config path** |
| widgets.rs:660 | `/instances/{}` | [crates/shell/src/widgets/mod.rs:1113](../crates/shell/src/widgets/mod.rs#L1113); [crates/shell/src/widgets/views/mod.rs:212](../crates/shell/src/widgets/views/mod.rs#L212) |
| widgets.rs:686 | `/` | Studio local state/action or forwarding alias; **not a runtime config path** |
| widgets.rs:700 | `/instances/{n}/settings/{}` | [crates/shell/src/widgets/mod.rs:1113](../crates/shell/src/widgets/mod.rs#L1113); [crates/shell/src/widgets/views/mod.rs:212](../crates/shell/src/widgets/views/mod.rs#L212) |
| widgets.rs:740 | `/instances/{n}/cell` | [crates/shell/src/widgets/mod.rs:1113](../crates/shell/src/widgets/mod.rs#L1113); [crates/shell/src/widgets/views/mod.rs:212](../crates/shell/src/widgets/views/mod.rs#L212) |
| widgets.rs:741 | `/instances/{n}/size` | [crates/shell/src/widgets/mod.rs:1113](../crates/shell/src/widgets/mod.rs#L1113); [crates/shell/src/widgets/views/mod.rs:212](../crates/shell/src/widgets/views/mod.rs#L212) |

## Studio follow-ups

- Stop projecting Look into bars.json/widgets.json once Studio uses the runtime-owned layer. The projection has two sources of truth, can freeze former global values into per-family files, and does not project `/look/bars/style` or `/look/menus/style` when those exact fields change. Runtime now reads them itself. Keep the migration/undo implications in mind before removing old projected keys.
- The Corners slider permits 0–32 while config validation permits 0–48. Align the documented range. Defaults shown by the Look page (16 corners, Fluent icons, shadow on) are not necessarily the active theme/design defaults; an unset row should show its resolved/inherited value.
- `/look/text/tone` and `/look/text/shadow` mean bare wallpaper text. Projection currently writes global `appearance/text` and `appearance/shadow`, which also change panel appearance or surface shadow. Keep those separate in Studio previews and resets.
- Custom virtual paths (`/windows_null/*`, `/@both/*`, `/theme:*`, draft form values, plugin actions) are intentionally not runtime JSON pointers. Their expansion/forwarding is recorded above. No Studio files were edited by this task.

## Validation and live evidence

Pending the final merged-Studio audit, release deployment, and isolated fixture capture. This section is updated after verification; no claim of live verification is implied by token tests.
