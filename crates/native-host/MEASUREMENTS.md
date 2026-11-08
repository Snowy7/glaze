# Native host verification and measurements

Measured 5 November 2026 on the user's Windows 11 Pro desktop, build 26300,
Ryzen 7 7800X3D, x86_64-pc-windows-msvc. Compiler:
`rustc 1.101.0-nightly (1c3a65069 2026-11-02)`, LLVM 23.1.5.
Release profile: thin LTO, one codegen unit, panic abort, stripped.

The measured source is `ad596ae` (including the restart-priority event queue).
The subsequent watchdog change only replaces process termination on the timeout
path; the echo and idle paths measured here are unchanged.

## Fixture and results

One native host, one active plugin and the dependency-free native echo DLL.
The benchmark's parent owns a real `Supervisor`; every latency sample includes
its worker channel, named-pipe framing, DLL dispatch, output copy/free, and reply.
It does not include Wasmtime, a UI repaint, or a real vendor SDK's allocations.
The shared desktop remained in use by the user and other agents.

| Metric | Result |
|---|---:|
| Warm-up | 100 calls |
| Timed calls | 10,000 |
| Input and output payload | 64 bytes each |
| Median round trip | 46.60 microseconds |
| p95 round trip | 100.50 microseconds |
| p99 round trip | 250.50 microseconds |
| Mean round trip | 55.77 microseconds |
| Median private memory, 3 x 60 s idle | 0.613 MiB |
| Median working set, 3 x 60 s idle | 4.566 MiB |
| Recorded CPU, percent of one core | 0.000000% in all three samples |
| Native processes before activate / after last deactivate | 0 / 0 |

No separate native-host numeric budget was specified. Idle uses blocking I/O and
condition variables; supervision wakes only for commands, pipe failure, events,
or a scheduled restart. Zero CPU means no accounted CPU-time increase at the
Windows process accounting resolution over these intervals.

Raw final-run output (host PID 62892):

```text
host_pid=62892, payload_bytes=64, warmup_calls=100, measured_calls=10000
roundtrip_us: median=46.60, p95=100.50, p99=250.50, mean=55.77
sample=1, private_mib=0.613, working_set_mib=4.566, cpu_one_core_percent=0.000000
sample=2, private_mib=0.613, working_set_mib=4.566, cpu_one_core_percent=0.000000
sample=3, private_mib=0.613, working_set_mib=4.566, cpu_one_core_percent=0.000000
```

An earlier run of `7da4eaa` recorded 29.50 us median / 48.60 us p95 / 76.20 us
p99, and 0.656 MiB private / 5.055 MiB working set / zero recorded CPU in all
three 60-second intervals. Use the final-source figures above for the handoff;
the two latency runs illustrate scheduling variation on this shared PC.

## Validation

`cargo test -p glaze-native-host`: **7 passed** (2 unit tests, 5 Windows
integration tests). `cargo clippy -p glaze-native-host --all-targets -- -D warnings`:
clean. Release binary and measurement example built successfully.

The tests use real DLLs and disposable child processes. They cover opaque/empty/
1-MiB round-trips, background-thread events, module errors preserving the host,
missing native-code permission, undeclared modules, shared and explicit isolated
hosts, stopping the last host, crash recovery and restarted events, automatic
isolation after three crashes, recovery from a background crash without another
call, the 150-ms hang deadline, traversal/absolute-path/alternate-stream refusal,
and a DLL symlink resolving outside the plugin folder. Symlink creation succeeded
on this machine. The queue unit test checks that a vendor event flood cannot hide
the restart notification. The measurement harness additionally waits on the
child process handle after dropping the supervisor, proving it exited.

The existing stopped-agent folders prevent Cargo from loading the working
workspace (`plugins/discord/Cargo.toml` is missing). Validation therefore used
`git archive HEAD` plus this slice's files under
`C:\WINDOWS\TEMP\glaze-native-host-validation`, with target directory
`C:\glaze-targets\native-host`. No leftover file or lockfile was changed to
make these checks pass. No synthetic input, window manipulation or deployment
was performed, and every host started by the harness exited.

## Reproduce

From a complete checkout (or an archived checkout with the native-host and
native-echo files overlaid):

```powershell
cargo test -p glaze-native-host
cargo clippy -p glaze-native-host --all-targets -- -D warnings
cargo build --release -p glaze-native-host --bins --examples
rustc '--edition=2024' '--crate-type=cdylib' '-Cpanic=abort' '-Copt-level=2' '-Clinker=rust-lld.exe' examples/native-echo/src/lib.rs '-o' "$env:TEMP/native_echo.dll"
& "$env:CARGO_TARGET_DIR/release/examples/measure.exe" "$env:CARGO_TARGET_DIR/release/glaze-native-host.exe" "$env:TEMP/native_echo.dll" 60
```

Set `CARGO_TARGET_DIR` first (the measured run used
`C:/glaze-targets/native-host`), or substitute the checkout's `target` directory.
The last argument is the number of idle seconds for **each of three** samples.
Private memory comes from `GetProcessMemoryInfo` / `PrivateUsage`, working set
from `WorkingSetSize`, and CPU from the kernel+user `GetProcessTimes` delta,
divided by actual wall time and multiplied by 100 (one-core percent).

The timeout watchdog uses `TerminateProcess` so vendor DLL detach code cannot
deadlock the timeout path, consistent with
[Microsoft's process-exit guidance](https://learn.microsoft.com/en-us/windows/win32/api/processthreadsapi/nf-processthreadsapi-exitprocess).
