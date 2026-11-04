# mypaper performance: 2026-11-04 04:18 (baseline-shell)

- Machine: 16 logical CPUs, screens 2560x1440 + 2560x1440
- Build: `C:\mypaper-targets\perf\baseline`
- 4 starts per scenario (1 cold, then warm), CPU windows of 10 s after 5 s settling

## Budget

**10 of 16 limits broken.**

- widgets: wallpaper.first_frame_ms.cold = 1139.577 > 1000
- widgets: wallpaper.first_frame_ms.warm = 849.508 > 500
- bars: wallpaper.first_frame_ms.cold = 3060.804 > 1000
- bars: wallpaper.first_frame_ms.warm = 1074.701 > 500
- bars-hide: core.spawn_ms.warm = 111.525 > 100
- bars-hide: wallpaper.first_frame_ms.cold = 1319.840 > 1000
- bars-hide: wallpaper.first_frame_ms.warm = 1027.239 > 500
- full: core.spawn_ms.warm = 110.846 > 100
- full: wallpaper.first_frame_ms.cold = 1326.877 > 1000
- full: wallpaper.first_frame_ms.warm = 1171.661 > 500

## Startup

Milliseconds from core's process creation to each component being ready, cold (fresh profile) / warm (median).

| Scenario | core spawned | wallpaper first frame | widgets first paint | bars first paint | taskbar hidden |
|---|---|---|---|---|---|
| `widgets` | 678 / 85 | 1140 / 850 | - | - | - |
| `bars` | 736 / 85 | 3061 / 1075 | - | 2715 / 1455 | - |
| `bars-hide` | 363 / 112 | 1320 / 1027 | - | 1553 / 1286 | 434 / 156 |
| `full` | 455 / 111 | 1327 / 1172 | - | 1792 / 1477 | - |

## CPU

Percent of the whole machine (of one core in parentheses), WebView2 processes included in their host. *Held* is paused from the tray (`paused: true`).

| Scenario | screens visible | total | wallpaper | widgets | bars | core | held total |
|---|---|---|---|---|---|---|---|
| `widgets` | 1 | 0.010% (0.16%) | 0.010% (0.16%) | 0.000% (0.00%) | - | 0.000% (0.00%) | 0.000% (0.00%) |
| `bars` | 1 | 0.146% (2.34%) | 0.010% (0.16%) | - | 0.136% (2.18%) | 0.000% (0.00%) | 0.049% (0.78%) |
| `bars-hide` | 1 | 0.039% (0.62%) | 0.010% (0.16%) | - | 0.029% (0.47%) | 0.000% (0.00%) | 0.039% (0.62%) |
| `full` | 1 | 0.195% (3.12%) | 0.166% (2.65%) | 0.000% (0.00%) | 0.029% (0.47%) | 0.000% (0.00%) | 0.049% (0.78%) |

## Memory

Private / working set in MB, running. A browser process both hosts share is `shared WebView2`.

| Scenario | total | core | wallpaper | widgets | bars | shared WebView2 |
|---|---|---|---|---|---|---|
| `widgets` | 311 / 39 | 2 / 12 | 307 / 16 | 2 / 12 | - | - |
| `bars` | 563 / 537 | 2 / 12 | 303 / 15 | - | 258 / 510 | - |
| `bars-hide` | 584 / 539 | 2 / 12 | 305 / 16 | - | 277 / 512 | - |
| `full` | 544 / 548 | 2 / 12 | 289 / 19 | 2 / 12 | 251 / 505 | - |

## Notes

- `widgets`: run 0: no READY component=widgets first_paint_ms within 20s
- `widgets`: run 1: no READY component=widgets first_paint_ms within 20s
- `widgets`: run 2: no READY component=widgets first_paint_ms within 20s
- `widgets`: run 3: no READY component=widgets first_paint_ms within 20s
- `widgets`: 1 of 2 screens were covered by windows during the steady sample (the wallpaper and widgets pause there)
- `bars`: run 0: some child processes outlived core by 5 s
- `bars`: 1 of 2 screens were covered by windows during the steady sample (the wallpaper and widgets pause there)
- `bars-hide`: run 0: some child processes outlived core by 5 s
- `bars-hide`: 1 of 2 screens were covered by windows during the steady sample (the wallpaper and widgets pause there)
- `full`: run 0: no READY component=widgets first_paint_ms within 20s
- `full`: run 1: no READY component=widgets first_paint_ms within 20s
- `full`: run 2: no READY component=widgets first_paint_ms within 20s
- `full`: run 3: no READY component=widgets first_paint_ms within 20s
- `full`: 1 of 2 screens were covered by windows during the steady sample (the wallpaper and widgets pause there)
