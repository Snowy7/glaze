//! The machine side: Glaze's three processes (core, the engine, the shell) with their state,
//! memory and CPU; starting, stopping and restarting them; Start with Windows; the graphics
//! cards; diagnostics; and opening files. Live numbers are polled only while someone looks.

use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use serde::Deserialize;
use windows::Win32::Foundation::{CloseHandle, FILETIME, HANDLE, LPARAM, WPARAM};
use windows::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW, TH32CS_SNAPPROCESS,
    TH32CS_SNAPTHREAD, THREADENTRY32, Thread32First, Thread32Next,
};
use windows::Win32::System::ProcessStatus::{GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS_EX};
use windows::Win32::System::Threading::{
    GetProcessTimes, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_SYNCHRONIZE,
    PROCESS_TERMINATE, TerminateProcess, WaitForSingleObject,
};
use windows::Win32::UI::WindowsAndMessaging::{PostThreadMessageW, WM_QUIT};

const CREATE_NO_WINDOW: u32 = 0x0800_0000;
const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;

/// Glaze's parts: what Troubleshoot lists.
pub const PARTS: [(&str, &str); 3] = [
    ("core", "glaze-core.exe"),
    ("engine", "glaze-wallpaper.exe"),
    ("shell", "glaze-shell.exe"),
];

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Part {
    pub running: bool,
    pub pid: u32,
    /// Private memory, MB.
    pub memory: f32,
    /// Percent of one core since the last poll.
    pub cpu: f32,
    /// Process CPU time at the last poll, 100 ns units.
    cpu_time: u64,
}

#[derive(Debug, Clone, Default, Deserialize, PartialEq)]
pub struct Autostart {
    pub enabled: bool,
    pub method: Option<String>,
    pub command: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct System {
    pub parts: [Part; 3],
    polled: Option<Instant>,
    pub autostart: Option<Autostart>,
    /// Graphics cards: (name, integrated?), read on first use.
    pub gpus: Option<Vec<(String, bool)>>,
    /// The card that drives the screens (where `auto` renders), read with `gpus`.
    pub display_gpu: Option<String>,
    pub core_path: Option<PathBuf>,
}

/// How often live numbers refresh while visible.
const POLL: Duration = Duration::from_secs(3);

impl System {
    pub fn probe() -> Self {
        let mut s = Self {
            core_path: find_exe("glaze-core.exe"),
            ..Self::default()
        };
        s.refresh();
        s
    }

    /// Fixed state for goldens: everything running.
    #[cfg(test)]
    pub fn fixture() -> Self {
        let part = |memory| Part {
            running: true,
            pid: 1,
            memory,
            cpu: 0.0,
            cpu_time: 0,
        };
        Self {
            parts: [part(2.0), part(448.0), part(22.0)],
            polled: None,
            autostart: Some(Autostart {
                enabled: true,
                method: Some("task".into()),
                command: None,
            }),
            gpus: Some(vec![
                ("Intel Iris Xe Graphics".into(), true),
                ("NVIDIA GeForce RTX 4060 Laptop GPU".into(), false),
            ]),
            display_gpu: Some("Intel Iris Xe Graphics".into()),
            core_path: Some(PathBuf::from("C:\\Program Files\\glaze\\glaze-core.exe")),
        }
    }

    pub fn running(&self) -> bool {
        self.parts[0].running
    }

    /// Glaze's whole cost: CPU (percent of one core) and private MB.
    pub fn total(&self) -> (f32, f32) {
        self.parts
            .iter()
            .fold((0.0, 0.0), |(c, m), p| (c + p.cpu, m + p.memory))
    }

    pub fn next_poll(&self, watching: bool) -> Option<Instant> {
        watching.then(|| self.polled.map_or_else(Instant::now, |at| at + POLL))
    }

    pub fn poll_due(&self, now: Instant) -> bool {
        self.polled.is_none_or(|at| at + POLL <= now)
    }

    /// Reads the processes again (a snapshot, then per part its memory and CPU time).
    pub fn refresh(&mut self) {
        let now = Instant::now();
        let elapsed = self.polled.map(|at| now.duration_since(at).as_secs_f32());
        let processes = processes();
        for (i, (_, exe)) in PARTS.iter().enumerate() {
            let pid = processes
                .iter()
                .find(|(_, name)| name.eq_ignore_ascii_case(exe))
                .map(|(pid, _)| *pid);
            let before = self.parts[i];
            self.parts[i] = match pid {
                Some(pid) => {
                    let (memory, cpu_time) = usage(pid).unwrap_or((0.0, 0));
                    let cpu = match elapsed {
                        Some(secs) if before.pid == pid && secs > 0.0 && cpu_time >= before.cpu_time => {
                            (cpu_time - before.cpu_time) as f32 / 1.0e7 / secs * 100.0
                        }
                        _ => 0.0,
                    };
                    Part {
                        running: true,
                        pid,
                        memory,
                        cpu,
                        cpu_time,
                    }
                }
                None => Part::default(),
            };
        }
        self.polled = Some(now);
    }

    /// The graphics cards, read once (adapter enumeration creates no device).
    pub fn gpus(&mut self) -> &[(String, bool)] {
        if self.gpus.is_none() {
            self.display_gpu = glaze_desktop::display_gpu::display_adapter().map(|a| a.name);
        }
        self.gpus.get_or_insert_with(adapters)
    }
}

fn processes() -> Vec<(u32, String)> {
    // SAFETY: a snapshot handle closed below.
    let Ok(snapshot) = (unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) }) else {
        return Vec::new();
    };
    let mut entry = PROCESSENTRY32W {
        dwSize: size_of::<PROCESSENTRY32W>() as u32,
        ..Default::default()
    };
    let mut found = Vec::new();
    // SAFETY: `entry.dwSize` is set; the snapshot is valid until closed.
    unsafe {
        let mut more = Process32FirstW(snapshot, &mut entry).is_ok();
        while more {
            let len = entry.szExeFile.iter().position(|&c| c == 0).unwrap_or(entry.szExeFile.len());
            found.push((entry.th32ProcessID, String::from_utf16_lossy(&entry.szExeFile[..len])));
            more = Process32NextW(snapshot, &mut entry).is_ok();
        }
        let _ = CloseHandle(snapshot);
    }
    found
}

/// Private MB and total CPU time (100 ns) of a process.
fn usage(pid: u32) -> Option<(f32, u64)> {
    // SAFETY: a limited query handle, closed below; locals of the right sizes.
    unsafe {
        let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
        let mut counters = PROCESS_MEMORY_COUNTERS_EX {
            cb: size_of::<PROCESS_MEMORY_COUNTERS_EX>() as u32,
            ..Default::default()
        };
        let memory = GetProcessMemoryInfo(process, (&raw mut counters).cast(), counters.cb)
            .ok()
            .map(|()| counters.PrivateUsage as f32 / (1024.0 * 1024.0));
        let (mut created, mut exited, mut kernel, mut user) = (FILETIME::default(), FILETIME::default(), FILETIME::default(), FILETIME::default());
        let times = GetProcessTimes(process, &mut created, &mut exited, &mut kernel, &mut user).ok();
        let _ = CloseHandle(process);
        let ticks = |t: FILETIME| (u64::from(t.dwHighDateTime) << 32) | u64::from(t.dwLowDateTime);
        Some((memory?, times.map_or(0, |()| ticks(kernel) + ticks(user))))
    }
}

fn adapters() -> Vec<(String, bool)> {
    use windows::Win32::Graphics::Dxgi::{CreateDXGIFactory1, DXGI_ADAPTER_FLAG_SOFTWARE, IDXGIFactory1};
    let mut out = Vec::new();
    // SAFETY: a factory and adapter descriptions only; no device is created, so no vendor
    // driver loads (plan Rule 1).
    unsafe {
        let Ok(factory) = CreateDXGIFactory1::<IDXGIFactory1>() else {
            return out;
        };
        let mut i = 0;
        while let Ok(adapter) = factory.EnumAdapters1(i) {
            i += 1;
            let Ok(desc) = adapter.GetDesc1() else { continue };
            if desc.Flags & DXGI_ADAPTER_FLAG_SOFTWARE.0 as u32 != 0 {
                continue;
            }
            let len = desc.Description.iter().position(|&c| c == 0).unwrap_or(desc.Description.len());
            let name = String::from_utf16_lossy(&desc.Description[..len]);
            // Integrated graphics share system memory and have little of their own.
            let integrated = desc.DedicatedVideoMemory < 1 << 30;
            if !out.iter().any(|(n, _)| *n == name) {
                out.push((name, integrated));
            }
        }
    }
    out
}

/// `name` beside Studio (installed), else the newest build in a parent's `target` folder.
pub fn find_exe(name: &str) -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let beside = exe.with_file_name(name);
    if beside.is_file() {
        return Some(beside);
    }
    exe.ancestors()
        .skip(1)
        .flat_map(|dir| ["release", "debug"].map(|p| dir.join("target").join(p).join(name)))
        .filter(|p| p.is_file())
        .max_by_key(|p| std::fs::metadata(p).and_then(|m| m.modified()).ok())
}

/// Starts core (it reads `config.json` itself); nothing if it's running.
pub fn start() -> Result<()> {
    if processes().iter().any(|(_, n)| n.eq_ignore_ascii_case("glaze-core.exe")) {
        return Ok(());
    }
    let exe = find_exe("glaze-core.exe").context("glaze-core.exe isn't installed beside Studio")?;
    let mut child = Command::new(&exe)
        .current_dir(exe.parent().unwrap_or(Path::new(".")))
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .creation_flags(CREATE_NO_WINDOW | CREATE_NEW_PROCESS_GROUP)
        .spawn()
        .with_context(|| format!("starting {}", exe.display()))?;
    // Core exits at once on a bad config or a second instance: say so.
    std::thread::sleep(Duration::from_millis(400));
    if let Some(exit) = child.try_wait()? {
        bail!("Glaze stopped right away ({exit}); Open logs in Troubleshoot says why");
    }
    Ok(())
}

/// Starts Studio again once this one has quit: a hidden PowerShell waits for this process, then
/// starts `glaze-studio.exe` beside it (which a deploy may have replaced meanwhile). The
/// caller quits right after.
pub fn restart_studio() -> Result<()> {
    let exe = std::env::current_exe()?.with_file_name("glaze-studio.exe");
    let path = exe.display().to_string().replace('\'', "''");
    let pid = std::process::id();
    Command::new("powershell.exe")
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-WindowStyle",
            "Hidden",
            "-Command",
            &format!("Wait-Process -Id {pid} -Timeout 30 -ErrorAction SilentlyContinue; Start-Process -FilePath '{path}'"),
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .creation_flags(CREATE_NO_WINDOW)
        .spawn()
        .context("restarting Studio")?;
    Ok(())
}

/// Stops core (which restores the desktop and stops its children), then what's left.
pub fn stop() -> Result<()> {
    let ours = || {
        processes()
            .into_iter()
            .filter(|(_, n)| PARTS.iter().any(|(_, exe)| n.eq_ignore_ascii_case(exe)))
            .collect::<Vec<_>>()
    };
    for (pid, name) in ours() {
        if name.eq_ignore_ascii_case("glaze-core.exe") {
            post_quit(pid);
        }
    }
    let deadline = Instant::now() + Duration::from_secs(3);
    while Instant::now() < deadline && !ours().is_empty() {
        std::thread::sleep(Duration::from_millis(50));
    }
    for (pid, _) in ours() {
        terminate(pid)?;
    }
    Ok(())
}

/// Restarts the engine or the shell: core's supervisor starts a part again when it exits.
pub fn restart(part: &str) -> Result<()> {
    let exe = PARTS
        .iter()
        .find(|(p, _)| *p == part)
        .map(|(_, exe)| *exe)
        .context("no such part")?;
    if part == "core" {
        stop()?;
        return start();
    }
    let Some((pid, _)) = processes().into_iter().find(|(_, n)| n.eq_ignore_ascii_case(exe)) else {
        bail!("the {part} isn't running; Start Glaze starts it");
    };
    terminate(pid)
}

fn post_quit(pid: u32) {
    // SAFETY: a thread snapshot, closed below; posting to a thread id is always safe.
    unsafe {
        let Ok(snapshot) = CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0) else { return };
        let mut entry = THREADENTRY32 {
            dwSize: size_of::<THREADENTRY32>() as u32,
            ..Default::default()
        };
        let mut more = Thread32First(snapshot, &mut entry).is_ok();
        while more {
            if entry.th32OwnerProcessID == pid {
                let _ = PostThreadMessageW(entry.th32ThreadID, WM_QUIT, WPARAM(0), LPARAM(0));
            }
            more = Thread32Next(snapshot, &mut entry).is_ok();
        }
        let _ = CloseHandle(snapshot);
    }
}

fn terminate(pid: u32) -> Result<()> {
    // SAFETY: a handle we open and close here.
    unsafe {
        let Ok(process) = OpenProcess(PROCESS_TERMINATE | PROCESS_SYNCHRONIZE, false, pid) else {
            return Ok(()); // already gone
        };
        let result = TerminateProcess(process, 1).with_context(|| format!("stopping process {pid}"));
        WaitForSingleObject(process, 2000);
        let _ = CloseHandle(process);
        result
    }
}

/// The emergency stop, as a button: the same keys core listens for anywhere.
pub fn restore_desktop() {
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        INPUT, INPUT_0, INPUT_KEYBOARD, KEYBD_EVENT_FLAGS, KEYBDINPUT, KEYEVENTF_KEYUP, SendInput,
        VIRTUAL_KEY, VK_BACK, VK_CONTROL, VK_MENU, VK_SHIFT,
    };
    let key = |vk: VIRTUAL_KEY, up: bool| INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: vk,
                dwFlags: if up { KEYEVENTF_KEYUP } else { KEYBD_EVENT_FLAGS(0) },
                ..Default::default()
            },
        },
    };
    let chord = [VK_CONTROL, VK_MENU, VK_SHIFT, VK_BACK];
    let inputs: Vec<INPUT> = chord
        .iter()
        .map(|&vk| key(vk, false))
        .chain(chord.iter().rev().map(|&vk| key(vk, true)))
        .collect();
    // SAFETY: a local array of well-formed keyboard inputs.
    unsafe { SendInput(&inputs, size_of::<INPUT>() as i32) };
}

/// Start with Windows, from core's `--autostart` (the source of truth).
pub fn autostart(command: &str) -> Result<Autostart> {
    let core = find_exe("glaze-core.exe").context("glaze-core.exe isn't installed beside Studio")?;
    let output = Command::new(&core)
        .args(["--autostart", command])
        .creation_flags(CREATE_NO_WINDOW)
        .stdin(Stdio::null())
        .output()
        .with_context(|| format!("running {}", core.display()))?;
    if !output.status.success() {
        bail!("{}", String::from_utf8_lossy(&output.stderr).trim());
    }
    serde_json::from_slice(&output.stdout).context("reading core's answer")
}

/// Versions, parts, graphics and the settings files' validity; no personal paths.
pub fn diagnostics(app: &crate::app::App) -> String {
    let mut out = format!("Glaze Studio {}\n", env!("CARGO_PKG_VERSION"));
    for ((name, _), part) in PARTS.iter().zip(app.system.parts) {
        out += &format!(
            "{name}: {} {:.0} MB {:.1}% CPU\n",
            if part.running { "running" } else { "stopped" },
            part.memory,
            part.cpu
        );
    }
    for (gpu, integrated) in app.system.gpus.clone().unwrap_or_default() {
        out += &format!("GPU: {gpu}{}\n", if integrated { " (integrated)" } else { "" });
    }
    for id in crate::docs::DocId::ALL {
        out += &format!(
            "{:?}: {}\n",
            id,
            app.docs.error(id).map_or("valid".into(), |e| format!("can't be read ({})", e.lines().next().unwrap_or("")))
        );
    }
    out += &format!("wallpapers: {}\n", app.library.items.len());
    out
}

pub fn copy_text(text: &str) {
    use windows::Win32::System::DataExchange::{CloseClipboard, EmptyClipboard, OpenClipboard, SetClipboardData};
    use windows::Win32::System::Memory::{GMEM_MOVEABLE, GlobalAlloc, GlobalLock, GlobalUnlock};
    let wide: Vec<u16> = text.encode_utf16().chain(Some(0)).collect();
    // SAFETY: the clipboard owns the global memory after SetClipboardData succeeds.
    unsafe {
        if OpenClipboard(None).is_err() {
            return;
        }
        let _ = EmptyClipboard();
        if let Ok(mem) = GlobalAlloc(GMEM_MOVEABLE, wide.len() * 2) {
            let ptr = GlobalLock(mem) as *mut u16;
            if !ptr.is_null() {
                std::ptr::copy_nonoverlapping(wide.as_ptr(), ptr, wide.len());
                let _ = GlobalUnlock(mem);
                let _ = SetClipboardData(13, Some(HANDLE(mem.0))); // CF_UNICODETEXT
            }
        }
        let _ = CloseClipboard();
    }
}

/// Opens a URL or file with its default app.
pub fn open(target: &str) {
    use windows::Win32::UI::Shell::ShellExecuteW;
    use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
    // SAFETY: plain strings that outlive the call.
    unsafe {
        ShellExecuteW(
            None,
            windows::core::w!("open"),
            &windows::core::HSTRING::from(target),
            None,
            None,
            SW_SHOWNORMAL,
        );
    }
}

/// Shows a file or folder selected in Explorer.
pub fn reveal(path: &Path) {
    let _ = Command::new("explorer.exe")
        .arg(format!("/select,{}", path.display()))
        .spawn();
}

pub fn open_logs() {
    if let Some(dir) = std::env::var_os("LOCALAPPDATA") {
        let log = PathBuf::from(dir).join("glaze").join("glaze.log");
        if log.exists() {
            reveal(&log);
        } else if let Some(parent) = log.parent() {
            open(&parent.display().to_string());
        }
    }
}
