//! Measures the real helper with Hello's one-second timer active, then game-paused. It uses
//! an isolated profile and owns/reaps its child; it never publishes game activity or touches UI.
use anyhow::{Context, Result, ensure};
use glaze_plugin_api::{
    SizeClass,
    wire::{self, HelperMsg, HostMsg},
};
use std::{
    io::BufReader,
    os::windows::{io::AsRawHandle, process::CommandExt},
    path::PathBuf,
    process::{Child, Command, Stdio},
    sync::mpsc,
    time::{Duration, Instant},
};
use windows::Win32::{
    Foundation::{FILETIME, HANDLE},
    System::{
        ProcessStatus::{GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS_EX},
        Threading::{CREATE_NO_WINDOW, GetProcessTimes},
    },
};

struct Helper(Child);
impl Drop for Helper {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn cpu(child: &Child) -> Result<u64> {
    let (mut created, mut exited, mut kernel, mut user) = (
        FILETIME::default(),
        FILETIME::default(),
        FILETIME::default(),
        FILETIME::default(),
    );
    // SAFETY: the child owns this process handle and every output buffer is live.
    unsafe {
        GetProcessTimes(
            HANDLE(child.as_raw_handle()),
            &mut created,
            &mut exited,
            &mut kernel,
            &mut user,
        )?;
    }
    let ticks = |t: FILETIME| u64::from(t.dwLowDateTime) | (u64::from(t.dwHighDateTime) << 32);
    Ok(ticks(kernel) + ticks(user))
}

fn private(child: &Child) -> Result<usize> {
    let mut memory = PROCESS_MEMORY_COUNTERS_EX {
        cb: size_of::<PROCESS_MEMORY_COUNTERS_EX>() as u32,
        ..Default::default()
    };
    // SAFETY: the EX output starts with PROCESS_MEMORY_COUNTERS, with its exact size in cb.
    unsafe {
        GetProcessMemoryInfo(
            HANDLE(child.as_raw_handle()),
            (&raw mut memory).cast(),
            memory.cb,
        )?;
    }
    Ok(memory.PrivateUsage)
}

fn main() -> Result<()> {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    ensure!(
        args.len() == 4,
        "quiet_helper HELPER_EXE HELLO_DIR SECONDS OUTPUT.json"
    );
    let seconds = args[2].to_string_lossy().parse::<u64>()?;
    let root = std::env::temp_dir().join(format!("glaze-game-helper-{}", std::process::id()));
    std::fs::create_dir_all(&root)?;
    let mut helper = Helper(
        Command::new(&args[0])
            .env("APPDATA", &root)
            .env("LOCALAPPDATA", &root)
            .creation_flags(CREATE_NO_WINDOW.0)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()?,
    );
    let mut input = helper.0.stdin.take().context("helper stdin")?;
    let mut output = BufReader::new(helper.0.stdout.take().context("helper stdout")?);
    let (tx, rx) = mpsc::channel();
    let reader = std::thread::spawn(move || {
        while let Ok(Some(message)) = wire::read::<HelperMsg>(&mut output) {
            if tx.send(message).is_err() {
                break;
            }
        }
    });
    let plugin = std::fs::canonicalize(PathBuf::from(&args[1]))?;
    wire::write(
        &mut input,
        &HostMsg::Open {
            plugin: "hello".into(),
            dir: plugin.to_string_lossy().into_owned(),
            instance: "quiet-probe".into(),
            face: "main".into(),
            size: SizeClass::Medium,
            settings: "{}".into(),
        },
    )?;
    while !matches!(
        rx.recv_timeout(Duration::from_secs(30))?,
        HelperMsg::View { .. }
    ) {}
    let mut measurements = Vec::new();
    for paused in [false, true] {
        if paused {
            wire::write(&mut input, &HostMsg::GameActivity(true))?;
            while !matches!(rx.recv_timeout(Duration::from_secs(5))?, HelperMsg::Demand { topics } if topics.is_empty())
            {
            }
        }
        let _ = rx.try_iter().count();
        let (before, private_before, started) =
            (cpu(&helper.0)?, private(&helper.0)?, Instant::now());
        println!(
            "helper pid={}, paused={paused}, measuring {seconds}s",
            helper.0.id()
        );
        std::thread::sleep(Duration::from_secs(seconds));
        let (ticks, elapsed) = (cpu(&helper.0)? - before, started.elapsed().as_secs_f64());
        let views = rx
            .try_iter()
            .filter(|m| matches!(m, HelperMsg::View { .. }))
            .count();
        measurements.push(serde_json::json!({ "paused": paused, "seconds": elapsed, "cpu_ms": ticks as f64 / 10_000.0,
            "cpu_core_pct": ticks as f64 / (elapsed * 100_000.0), "private_before": private_before, "private_after": private(&helper.0)?, "views": views }));
    }
    wire::write(&mut input, &HostMsg::GameActivity(false))?;
    ensure!(
        matches!(
            rx.recv_timeout(Duration::from_secs(5))?,
            HelperMsg::Demand { .. } | HelperMsg::View { .. }
        ),
        "helper did not resume"
    );
    wire::write(&mut input, &HostMsg::Shutdown)?;
    drop(input);
    ensure!(helper.0.wait()?.success(), "helper failed");
    reader.join().expect("helper reader");
    std::fs::write(
        PathBuf::from(&args[3]),
        serde_json::to_vec_pretty(&measurements)?,
    )?;
    println!("{}", serde_json::to_string_pretty(&measurements)?);
    // Only our fixed-prefix, isolated profile under TEMP is removed, after its child exited.
    ensure!(
        root.parent() == Some(std::env::temp_dir().as_path())
            && root
                .file_name()
                .is_some_and(|n| n.to_string_lossy().starts_with("glaze-game-helper-")),
        "unexpected probe profile path"
    );
    std::fs::remove_dir_all(root)?;
    Ok(())
}
