#![cfg_attr(windows, windows_subsystem = "windows")]
fn main() {
    #[cfg(windows)]
    {
        let mut args = std::env::args();
        let _ = args.next();
        if args.next().as_deref() != Some("--pipe") {
            std::process::exit(2);
        }
        let Some(pipe) = args.next() else {
            std::process::exit(2)
        };
        if mypaper_native_host::run_host(&pipe).is_err() {
            std::process::exit(1);
        }
    }
    #[cfg(not(windows))]
    {
        eprintln!("mypaper-native-host requires Windows");
        std::process::exit(2);
    }
}
