# Glaze performance: 2026-11-04 04:24 (baseline-widgets)

- Machine: 16 logical CPUs, screens 2560x1440 + 2560x1440
- Build: `C:\glaze-targets\perf\baseline`
- 4 starts per scenario (1 cold, then warm), CPU windows of 10 s after 5 s settling

## Budget

**6 of 8 limits broken.**

- widgets: wallpaper.first_frame_ms.cold = 1079.678 > 1000
- widgets: wallpaper.first_frame_ms.warm = 796.917 > 500
- full: core.spawn_ms.warm = 110.839 > 100
- full: total.cpu_held_pct = 0.273 > 0.1
- full: wallpaper.first_frame_ms.cold = 1321.403 > 1000
- full: wallpaper.first_frame_ms.warm = 1171.918 > 500

## Startup

Milliseconds from core's process creation to each component being ready, cold (fresh profile) / warm (median).

| Scenario | core spawned | wallpaper first frame | widgets first paint | bars first paint | taskbar hidden |
|---|---|---|---|---|---|
| `widgets` | 351 / 85 | 1080 / 797 | 1242 / 1937 | - | - |
| `full` | 366 / 111 | 1321 / 1172 | 1027 / 981 | 2380 / 1590 | - |

## CPU

Percent of the whole machine (of one core in parentheses), WebView2 processes included in their host. *Held* is paused from the tray (`paused: true`).

| Scenario | screens visible | total | wallpaper | widgets | bars | core | held total |
|---|---|---|---|---|---|---|---|
| `widgets` | 1 | 0.244% (3.90%) | 0.029% (0.47%) | 0.214% (3.43%) | - | 0.000% (0.00%) | 0.019% (0.31%) |
| `full` | 1 | 0.292% (4.68%) | 0.136% (2.18%) | 0.127% (2.03%) | 0.029% (0.47%) | 0.000% (0.00%) | 0.273% (4.36%) |

## Memory

Private / working set in MB, running. A browser process both hosts share is `shared WebView2`.

| Scenario | total | core | wallpaper | widgets | bars | shared WebView2 |
|---|---|---|---|---|---|---|
| `widgets` | 551 / 491 | 2 / 12 | 302 / 16 | 247 / 464 | - | - |
| `full` | 797 / 1009 | 2 / 12 | 289 / 19 | 250 / 462 | 256 / 516 | - |

## Notes

- `widgets`: run 0: some child processes outlived core by 5 s
- `widgets`: run 1: some child processes outlived core by 5 s
- `widgets`: run 2: some child processes outlived core by 5 s
- `widgets`: 1 of 2 screens were covered by windows during the steady sample (the wallpaper and widgets pause there)
- `full`: run 0: some child processes outlived core by 5 s
- `full`: 1 of 2 screens were covered by windows during the steady sample (the wallpaper and widgets pause there)
