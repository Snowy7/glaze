# Council review brief (round 7)

This review is read-only: do not edit, build, commit or run the app. Context is in
`plans/council/brief-native.md`. Use proven techniques and measure narrowly. Filesystem and OS tuning are out of scope.

Review `plans/council/plan.md` (v6). Score history:
- R2: Sol 7, Opus 7, Grok 6
- R3: 8, 7, 7
- R4: 9, 8, 8
- R5: Opus 8, Grok 8
- R6: Opus 7, Grok 7

Round 6's blockers and how v6 resolves them:
- **Slideshow republish** (Opus, Grok): cut. The bars' wallpaper background now shows only over a
  still image wallpaper, and falls back to a tint over every animated, web and app source.
- **Bars following liquid widgets**: S1 removes the fallback at `crates/bars/src/app.rs:1474-1476` and
  `look.rs:133-134`.
- **The S5 acknowledgement**: S5 deletes the local snapshot panes. The acknowledgement becomes
  "liquid look, and a snapshot exists, and a plate was sent".
- **Window-under-clear-bar switch**: dropped. A clear bar stays clear. The second S1 gate is now a
  `background.type: blur` dock over maximized apps.
- **Web `redraw()`**: changes to `OnChange`.
- **Wallpaper lens fill**: `"wallpaper"` is no longer a lens fill.
- **Remaining `backdrop-filter`s**: all of them are deleted, checked with a grep.

This is a fresh review. Check claims against the code (`D:\Coding\projects\desktop\wallpaper`, main
`f378a4c`). Answer:
1. **Score: N/10**: whether this plan, executed as written, delivers the requested retheme at
   near-zero performance cost, simply and without scope creep. It is a *plan*: implementation
   details an engineer resolves in the slice, with the stated check catching them, are not blocking.
2. Blocking problems only: a wrong decision, a budget that is unreachable as designed, a wrong order,
   or an internal contradiction. Cite a file and line. Do not re-raise fixed items unless the fix is wrong.
3. At most 2 non-blocking suggestions. Do not add scope.

Keep it under 300 words. Put the score on the first line.
