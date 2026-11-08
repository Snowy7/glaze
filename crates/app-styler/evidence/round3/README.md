# App styler round 3 evidence (2026-11-06)

Scripts and their output from the live desktop (Explorer 76024, Settings 73468).

| File | What |
|---|---|
| `flag-off.txt`, `flag-off-*.png` | Finding 4: 11 rules flips in about 1 s, then `experimental.app_styler` off while the restyles land. Both sessions put everything back (Settings 24, Explorer 16 elements on 4 UI threads) and detached; captures styled, then stock, then styled again. |
| `crash-guard.txt` | Finding 7: two separate Explorer folder processes ended within 3 s of being attached, 9 s apart. The guard gave up on Explorer and the surviving shell Explorer put its look back and detached (stop marker). |
| `core-kill.txt` | Finding 2: `glaze-core` ended with TerminateProcess. The styler exited 84 ms later, both apps restored, and the restarted core's helper re-attached with `--parent`. |
| `refuse-non-allowlisted.txt` | Finding 3: the CLI pins and checks the process; a non-allowlisted pid is refused. |
| `soak-attached.log` | The soak: held (sampling only) through other agents' windows, then 7 bursts, paused when Explorer's private bytes climbed 754 MB to 2.4 GB. |
| `control-flag-off-then-on.log`, `flag-off.log`, `attached-no-rules.log` | The controls behind the known issue in `docs/windows-styling.md`: 8-17 MB per burst with the flag off, 127-151 MB attached with no rules. |
