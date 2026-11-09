# Brief: app-styler round 4 (no leak in Explorer)

Read `plans/native/LIVE.md`, `docs/windows-styling.md` › "App styler", and
round 3's report (`crates/app-styler/evidence/round3`, commits 19a5b7c..a2e1f99). You own
`crates/app-styler`. The flag is **off** and stays off unless this round ends clean.

## The problem

While a styler session is subscribed (`AdviseVisualTreeChange`), every Explorer folder window opened and
closed leaves 127-410 MB in Explorer's private bytes, even with no rules (flag off: 8-17 MB per burst).
WinUI's diagnostics `HandleMap` keeps strong references to closed windows' elements, and detaching
doesn't reliably free them. The user's Explorer sits at ~1.3 GB from tonight's tests (leave it; the user
can restart it).

## Goal

Styling Explorer must not grow its memory beyond the flag-off control. Investigate, then fix:

1. Find what actually holds the memory: is it per-subscription (the HandleMap lives as long as
   AdviseVisualTreeChange), per handle we ask for, or per IInspectable we hold? Measure each variant
   in your own `explorer /separate,<dir>` processes with open/close bursts.
2. Then the cheapest design that styles new windows without the leak. Candidates: unadvise when the
   last folder window on a thread closes and re-advise when one opens (watch top-level window creation
   from the helper with a WinEvent hook, no polling); styling once per window and unadvising right
   after; or per-window short-lived sessions. Whatever you pick must still restyle live on rule
   changes and restore everything on exit.
3. Prove it: 10 bursts of open/navigate/close with styling on vs the flag-off control, in a separate
   Explorer process. Growth must match the control within noise. Then 30 minutes in the user's shell
   Explorer with the glass preset (your own windows only): private bytes and handles must stay flat
   within the control's range.
4. Clean: turn `experimental.app_styler` on with the glass preset. Not clean: leave it off and write
   down exactly what holds the memory and what would fix it.

Same safety rules as rounds 2-3. Test in separate Explorer processes first. Deploy only from
`C:\glaze-targets\main`. **No deploy, core restart or lock use 09:12-09:32 local** (index window).
`CARGO_TARGET_DIR=C:\glaze-targets\app-styler`. Stop when clean, or after 2.5 hours.
Report: the cause, the fix, the burst table vs control, the soak, the final flag state, commits.
