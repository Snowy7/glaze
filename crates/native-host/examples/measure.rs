//! cargo run --release -p glaze-native-host --example measure -- HOST_EXE ECHO_DLL [SECONDS]
#[cfg(windows)]
fn main() -> std::io::Result<()> {
    use glaze_native_host::{HostConfig, Isolation, PluginSpec, Supervisor};
    use std::{
        ffi::c_void,
        os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle},
        path::PathBuf,
        time::{Duration, Instant},
    };
    #[repr(C)]
    #[derive(Default)]
    struct Memory {
        cb: u32,
        faults: u32,
        peak_working: usize,
        working: usize,
        peak_paged: usize,
        paged: usize,
        peak_nonpaged: usize,
        nonpaged: usize,
        pagefile: usize,
        peak_pagefile: usize,
        private: usize,
    }
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn OpenProcess(access: u32, inherit: i32, pid: u32) -> *mut c_void;
        fn GetProcessTimes(
            process: *mut c_void,
            created: *mut u64,
            exited: *mut u64,
            kernel: *mut u64,
            user: *mut u64,
        ) -> i32;
        fn WaitForSingleObject(handle: *mut c_void, millis: u32) -> u32;
    }
    #[link(name = "psapi")]
    unsafe extern "system" {
        fn GetProcessMemoryInfo(process: *mut c_void, info: *mut Memory, size: u32) -> i32;
    }
    fn cpu(process: &OwnedHandle) -> std::io::Result<u64> {
        let (mut created, mut exited, mut kernel, mut user) = (0, 0, 0, 0);
        // SAFETY: valid query handle and writable FILETIME storage.
        if unsafe {
            GetProcessTimes(
                process.as_raw_handle(),
                &mut created,
                &mut exited,
                &mut kernel,
                &mut user,
            )
        } == 0
        {
            return Err(std::io::Error::last_os_error());
        }
        Ok(kernel + user)
    }
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    if args.len() < 2 {
        return Err(std::io::Error::other(
            "usage: measure HOST_EXE ECHO_DLL [idle seconds per sample, default 60]",
        ));
    }
    let seconds: u64 = args
        .get(2)
        .and_then(|arg| arg.to_str())
        .and_then(|arg| arg.parse().ok())
        .unwrap_or(60);
    let folder =
        std::env::temp_dir().join(format!("glaze-native-measure-{}", std::process::id()));
    std::fs::create_dir_all(&folder)?;
    std::fs::copy(&args[1], folder.join("native_echo.dll"))?;
    let supervisor = Supervisor::new(PathBuf::from(&args[0]), HostConfig::default());
    supervisor.activate(PluginSpec {
        id: "measure".into(),
        folder: folder.clone(),
        native: vec!["native_echo.dll".into()],
        permissions: vec!["native-code".into()],
        isolation: Isolation::Shared,
    })?;
    let pid = supervisor.hosts()?[0].pid;
    // SAFETY: open this harness's own child with query/read/synchronize rights.
    let raw = unsafe { OpenProcess(0x0400 | 0x0010 | 0x00100000, 0, pid) };
    if raw.is_null() {
        return Err(std::io::Error::last_os_error());
    }
    // SAFETY: take ownership of the new process handle.
    let process = unsafe { OwnedHandle::from_raw_handle(raw) };
    println!("host_pid={pid}, payload_bytes=64, warmup_calls=100, measured_calls=10000");
    let input = [42_u8; 64];
    for _ in 0..100 {
        assert_eq!(
            supervisor.call("measure", "native_echo.dll", "echo", &input)?,
            input
        );
    }
    let mut timings = Vec::with_capacity(10_000);
    for _ in 0..10_000 {
        let start = Instant::now();
        assert_eq!(
            supervisor.call("measure", "native_echo.dll", "echo", &input)?,
            input
        );
        timings.push(start.elapsed().as_nanos() as f64 / 1000.0);
    }
    timings.sort_by(f64::total_cmp);
    println!(
        "roundtrip_us: median={:.2}, p95={:.2}, p99={:.2}, mean={:.2}",
        timings[5000],
        timings[9500],
        timings[9900],
        timings.iter().sum::<f64>() / timings.len() as f64
    );
    for sample in 1..=3 {
        println!("idle sample {sample}/3: {seconds} seconds");
        let start_cpu = cpu(&process)?;
        let start = Instant::now();
        std::thread::sleep(Duration::from_secs(seconds));
        let percent =
            (cpu(&process)? - start_cpu) as f64 / 10_000_000.0 / start.elapsed().as_secs_f64()
                * 100.0;
        let mut memory = Memory {
            cb: size_of::<Memory>() as u32,
            ..Memory::default()
        };
        // SAFETY: correctly sized PROCESS_MEMORY_COUNTERS_EX and a query handle.
        if unsafe {
            GetProcessMemoryInfo(
                process.as_raw_handle(),
                &mut memory,
                size_of::<Memory>() as u32,
            )
        } == 0
        {
            return Err(std::io::Error::last_os_error());
        }
        println!(
            "sample={sample}, private_mib={:.3}, working_set_mib={:.3}, cpu_one_core_percent={percent:.6}",
            memory.private as f64 / 1048576.0,
            memory.working as f64 / 1048576.0
        );
    }
    drop(supervisor);
    // SAFETY: synchronization handle for the child owned by this harness.
    let exited = unsafe { WaitForSingleObject(process.as_raw_handle(), 5000) };
    assert_eq!(exited, 0, "child must be reaped on drop");
    std::fs::remove_dir_all(folder)?;
    Ok(())
}
#[cfg(not(windows))]
fn main() {
    eprintln!("Windows only");
}
