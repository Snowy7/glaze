# Native echo

This standalone Rust `cdylib` implements the C ABI in
`crates/native-host/include/mypaper_native.h`. Build it with
`cargo build --manifest-path examples/native-echo/Cargo.toml --release`, then copy
`target/release/mypaper_native_echo.dll` into a plugin folder as `native_echo.dll`.
`plugin.toml` illustrates the manifest fields; the platform's component and other
required metadata belong alongside it.

Methods: `echo` returns the input unchanged; `background` returns it and emits a
`background` event with the same bytes from a new thread; `crash` aborts its host;
`hang` never returns and tests the watchdog. Payloads can contain zero bytes.

The native host integration suite compiles this same source directly with rustc
into a temporary plugin directory. It never loads the DLL in the test process.
