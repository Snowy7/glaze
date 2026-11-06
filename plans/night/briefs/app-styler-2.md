# Brief: app-styler round 2 (make it look good, prove it's safe)

Read `plans/night/2026-11-06.md`, `plans/native/LIVE.md`, `docs/windows-styling.md` › "App styler",
and the first round's report below. You own `crates/app-styler` and its doc section; additive only in
`crates/theme` / `crates/config` / `crates/inject`. Not `crates/studio`.

## Where round 1 left it (commits 8e73985..2462731)

Works live in Settings and Explorer: selector rules, live rule changes, undo on every exit path, and an
allowlist re-check before each attach. The flag is off. Gaps:
1. The presets barely change Explorer: the header already sits on the backdrop, and the address/search
   box rules don't land on the surface that's actually drawn.
2. `resources` (theme-brush overrides) are parsed but not applied. That is also why hover/pressed
   looks still come from the control templates.
3. The Settings preset captures were never verified.
4. The crash guard was never exercised.
5. No 30-minute soak across Explorer restarts.

## Goal

1. **Resources:** apply `resources` overrides (the WinUI/system-XAML theme brushes and values such as
   `ControlFillColorDefaultBrush`, `SubtleFillColor*`, `CardBackgroundFillColor*`,
   `TextFillColor*`, `ControlCornerRadius`, `TabView*` keys, light/dark dictionaries) in the right
   scope, so controls' own templates, including hover/pressed, follow the look. Undo them like
   properties.
2. **Presets that visibly match mypaper**: calm/mono, glass (backdrop through, clear surfaces, soft
   fills), compact (denser rows and command bar). Explorer: tabs, address and search boxes, command
   bar, details pane, Home/Gallery. Settings: navigation pane, cards, search, title area. Use
   `--dump` to find the surfaces that are actually drawn. Look at before/after DPI-aware captures of
   **your own** Explorer and Settings windows for each preset in light and dark, and keep them under
   `C:\mypaper-targets\app-styler\round2\`. Calm, flat and translucent; no outlines on glass, no
   gloss (the `visual-restraint` / `liquid-glass` rules in `docs/glass.md`).
3. **Crash guard, proven:** exercise it with your own Settings window: end its process twice within
   60 s of attaching, check the guard trips and logs, and check a fresh session starts clean. Never
   kill explorer.exe on purpose.
4. **Soak:** with the flag on and the glass preset, 30 minutes of live use. Open and close your own
   Explorer windows and tabs, navigate, open Settings pages, restart Explorer twice
   (`Stop-Process -Name explorer` is allowed twice for this; mypaper recovers from Explorer
   restarts; check the bars and widgets come back). Measure the helper and the explorer.exe memory
   before and after. Idle helper CPU must be 0 %.
5. If the soak is clean, **leave `experimental.app_styler` on** in the user's `config.json` with the
   glass preset, and say so in the report so they can switch it off. If not, leave it off and say
   why.

## Stop when

Done above, or after 3 hours. Commit as you go. Report: what each preset changes (captures), the
crash-guard and soak evidence, the final flag state, commits.
