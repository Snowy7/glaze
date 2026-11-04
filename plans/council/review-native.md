# Council review brief: native plan, round 7

Round 6 scored v6 Opus 8 and Grok 8 (Sol was 9 on v3). v7 fixes:
- **Startup:** core keeps `wait_for_shell` and `stop_leftover`, then spawns the shell. The shell owns
  the early hide and the journal, and core replays only after the shell exits.
- **Borderless:** the unpackaged flow comes first, with the sparse package only if access isn't
  `Allowed`. The check requires `Allowed`.
- **Glass-only redraw:** restores the swapchain image first (the 0.65 upscale blit or a copy of the
  kept texture), then draws the glass.
- **Scratch:** deleted in N2.
- **N0.3:** names its wallpaper and estimates rain-city.

Only raise a **blocking** item if it would make the plan fail as written, not one an engineer
naturally resolves inside the slice, where the slice's check catches it. The plan has been through
six rounds; give a 9 or above if no such item remains.

---

# (previous round's brief follows; read "round 6" as "round 7")
# Council review brief: native plan, round 6

Round 5 scored v4 Opus 8 and Grok 8 (Sol was 9 on v3). v6 fixes:
- **Bars-on-screen gate:** core's spawn time plus N0.1's first paint, with the shell spawned before
  the taskbar work.
- **Plate geometry:** sent over shared memory and used as is, with no engine spring. The JSON-file
  transport is deleted, and the shell owns all motion.
- **Submenu gap:** **0**, butting against the parent and never overlapping. Rims sample only what is
  behind the parent, which is excluded from capture.
- **The kept frame:** rain-city's 0.65 offscreen target, or a per-screen texture measured in N0.3.
  The glass samples it directly, and the scratch copy is deleted in N5.
- **N0.2 additions:** a no-feedback-loop check, and a sparse package that gives identity with
  `graphicsCaptureWithoutBorder` for borderless capture.
- **Overlay:** stays `WS_EX_LAYERED` so that `WS_EX_TRANSPARENT` passes clicks through.

---

Round 4 scored v3 Sol 9, Opus 8 and Grok 8. All three confirmed that `SetWindowExclusionList` exists.

v4 adds:
- under the tiny-skia fallback, an opaque field fill with alpha set to 255 for RichEdit `TxDraw`;
- a glass-only redraw over the cached wallpaper frame for moving surfaces, with a ≤1 frame lag
  check in N2 and N4;
- in N0.2, the capture device (D3D11On12 vs a separate D3D11), borderless and no cursor, dropping
  stale `ConfigurationIteration`s, and the binding and `QueryInterface` detection;
- rain-city keeps its 0.65 scale and gains a GPU row;
- the submenu gap is sized to the sampled kernel (≤12 px);
- the scratch shrinks to the sampled regions.

The remainder of this brief is unchanged from round 4; where it says round 4, read round 5.

This review is read-only: do not edit, build, commit or run the app.

Review `plans/council/native-plan.md` (v3). The "User decisions" at its top are **fixed**. Point out
only where the plan fails to honour one. The user builds the whole plan in one go, so the slices are
build order.

Scores so far:
- round 2: Sol 8, Opus 6, Grok 5;
- round 3: Sol 9, Opus 7, Grok 6.

v3 fixes:
- **Clicks:** surfaces fill their shape at alpha ≥1/255, overlays are `WS_EX_LAYERED|TRANSPARENT`,
  and every slice gets a click test.
- **Rule 1:** now "no *hardware* device". WARP is allowed and measured, with a tiny-skia fallback.
- **Capture:** one WGC session per monitor; a per-session exclusion list is tried first (verified in
  N0), with WDA only if that fails; popups never overlap.
- **Solid and mono** have no backdrop.
- **Rain-city gate:** ≤4% of a core on 2 screens.
- **Plates GPU:** wgpu timestamps in N2.
- **Web and app kinds:** removed by default (user decision D-W pending).

Check the plan's claims against the code (`D:\Coding\projects\desktop\wallpaper`, main `f378a4c`)
and against Microsoft documentation. **Verify** whether a per-session window exclusion API exists for
Windows.Graphics.Capture on build 26100+ (Grok cited `SetWindowExclusionList`), and say so plainly.

This is a fresh review. Answer:
1. **Score: N/10**: whether this plan, executed as written, delivers a fully native, near-zero-resource
   shell with live Liquid Glass everywhere, simply and without scope creep. It is a plan: details an
   engineer resolves inside a slice, caught by that slice's check, are not blocking.
2. Blocking problems only: a wrong technical claim, an unreachable budget, a design that cannot
   work, a wrong order, or an internal contradiction. Cite a file, line or source. Do not re-raise
   items already fixed unless the fix is wrong.
3. At most 2 non-blocking suggestions. Do not add scope.

Keep it under 400 words. Put the score on the first line.
