# mypaper performance: 2026-11-04 05:02 (final)

- Machine: 16 logical CPUs, screens 2560x1440 + 2560x1440
- Build: `C:\mypaper-targets\perf\after3`
- 4 starts per scenario (1 cold, then warm), CPU windows of 10 s after 5 s settling

## Budget

**4 of 82 limits broken.**

- widgets: total.cpu_held_pct = 0.054 > 0.05
- bars: wallpaper.first_frame_ms.warm = 953.391 > 950
- bars-hide: core.spawn_ms.warm = 101.897 > 90
- full: total.cpu_held_pct = 0.102 > 0.05

## Startup

Milliseconds from core's process creation to each component being ready, cold (fresh profile) / warm (median).

| Scenario | core spawned | wallpaper first frame | widgets first paint | bars native paint | bars first paint | taskbar hidden |
|---|---|---|---|---|---|---|
| `shader` | 49 / 41 | 809 / 755 | - | - | - | - |
| `image` | 106 / 50 | 1112 / 759 | - | - | - | - |
| `slideshow` | 54 / 45 | 1002 / 802 | - | - | - | - |
| `video` | 66 / 43 | 987 / 934 | - | - | - | - |
| `scene` | 57 / 43 | 917 / 761 | - | - | - | - |
| `web` | 67 / 43 | 670 / 570 | - | - | - | - |
| `widgets` | 80 / 54 | 957 / 883 | 989 / 930 | - | - | - |
| `bars` | 83 / 56 | 965 / 953 | - | - / 173 | 1604 / 1662 | - |
| `bars-hide` | 147 / 102 | 1174 / 927 | - | - / 219 | 1567 / 1572 | 101 / 60 |
| `full` | 207 / 69 | 1238 / 1083 | 1249 / 1143 | - / 224 | 1739 / 1606 | - |

## CPU

Percent of the whole machine (of one core in parentheses), WebView2 processes included in their host (or in *shared WebView2*). *Held* is paused from the tray (`paused: true`).

| Scenario | screens visible | total | wallpaper | widgets | bars | shared WebView2 | core | held total |
|---|---|---|---|---|---|---|---|---|
| `shader` | 1 | 0.214% (3.43%) | 0.214% (3.43%) | - | - | - | 0.000% (0.00%) | 0.000% (0.00%) |
| `image` | 1 | 0.000% (0.00%) | 0.000% (0.00%) | - | - | - | 0.000% (0.00%) | 0.015% (0.23%) |
| `slideshow` | 1 | 0.029% (0.47%) | 0.019% (0.31%) | - | - | - | 0.010% (0.16%) | 0.005% (0.08%) |
| `video` | 1 | 0.351% (5.61%) | 0.351% (5.61%) | - | - | - | 0.000% (0.00%) | 0.005% (0.08%) |
| `scene` | 1 | 0.185% (2.96%) | 0.185% (2.96%) | - | - | - | 0.000% (0.00%) | 0.000% (0.00%) |
| `web` | 1 | 0.536% (8.58%) | 0.536% (8.58%) | - | - | - | 0.000% (0.00%) | 0.010% (0.16%) |
| `widgets` | 1 | 0.068% (1.09%) | 0.000% (0.00%) | 0.010% (0.16%) | - | 0.058% (0.93%) | 0.000% (0.00%) | 0.054% (0.86%) |
| `bars` | 1 | 0.019% (0.31%) | 0.000% (0.00%) | - | 0.010% (0.16%) | 0.010% (0.16%) | 0.000% (0.00%) | 0.005% (0.08%) |
| `bars-hide` | 1 | 0.058% (0.94%) | 0.019% (0.31%) | - | 0.000% (0.00%) | 0.039% (0.62%) | 0.000% (0.00%) | 0.039% (0.62%) |
| `full` | 1 | 0.380% (6.08%) | 0.166% (2.65%) | 0.020% (0.31%) | 0.000% (0.00%) | 0.195% (3.12%) | 0.000% (0.00%) | 0.102% (1.64%) |

## Memory

Private / working set in MB, running. A browser process both hosts share is `shared WebView2`.

| Scenario | total | core | wallpaper | widgets | bars | shared WebView2 |
|---|---|---|---|---|---|---|
| `shader` | 261 / 30 | 2 / 12 | 259 / 18 | - | - | - |
| `image` | 255 / 21 | 2 / 12 | 253 / 9 | - | - | - |
| `slideshow` | 290 / 23 | 2 / 12 | 288 / 11 | - | - | - |
| `video` | 355 / 57 | 2 / 12 | 353 / 45 | - | - | - |
| `scene` | 380 / 31 | 2 / 12 | 378 / 19 | - | - | - |
| `web` | 353 / 407 | 2 / 12 | 351 / 394 | - | - | - |
| `widgets` | 504 / 482 | 2 / 12 | 255 / 9 | 10 / 34 | - | 237 / 426 |
| `bars` | 515 / 530 | 2 / 12 | 253 / 5 | - | 7 / 39 | 253 / 474 |
| `bars-hide` | 533 / 535 | 2 / 12 | 254 / 9 | - | 7 / 39 | 270 / 475 |
| `full` | 670 / 741 | 2 / 12 | 258 / 18 | 11 / 34 | 7 / 39 | 391 / 637 |

## Notes

- `shader`: 1 of 2 screens were covered by windows during the steady sample (the wallpaper and widgets pause there)
- `image`: 1 of 2 screens were covered by windows during the steady sample (the wallpaper and widgets pause there)
- `slideshow`: 1 of 2 screens were covered by windows during the steady sample (the wallpaper and widgets pause there)
- `video`: 1 of 2 screens were covered by windows during the steady sample (the wallpaper and widgets pause there)
- `scene`: 1 of 2 screens were covered by windows during the steady sample (the wallpaper and widgets pause there)
- `web`: 1 of 2 screens were covered by windows during the steady sample (the wallpaper and widgets pause there)
- `widgets`: 1 of 2 screens were covered by windows during the steady sample (the wallpaper and widgets pause there)
- `bars`: 1 of 2 screens were covered by windows during the steady sample (the wallpaper and widgets pause there)
- `bars-hide`: 1 of 2 screens were covered by windows during the steady sample (the wallpaper and widgets pause there)
- `full`: 1 of 2 screens were covered by windows during the steady sample (the wallpaper and widgets pause there)
