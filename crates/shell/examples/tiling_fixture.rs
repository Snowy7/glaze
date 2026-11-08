//! Medium-integrity, PID-scoped desktop fixture. It never injects input or requests foreground.
#![windows_subsystem = "windows"]
use anyhow::{Context, Result, ensure};
use glaze_desktop::{
    tiling_recovery::{self, Identity},
    win_events::WinEvents,
};
use glaze_shell::tiling::{
    Tiler,
    animation::{Animator, Change},
};
use glaze_tiling::{Command as Op, Direction, Rect, Settings};
use std::{
    path::{Path, PathBuf},
    process::{Child, Command},
    time::{Duration, Instant},
};
use windows::{
    Win32::{
        Foundation::{FILETIME, HWND, LPARAM, LRESULT, RECT, WPARAM},
        Graphics::Gdi::*,
        System::{
            ProcessStatus::{K32GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS_EX},
            Threading::{GetCurrentProcess, GetProcessTimes},
        },
        UI::WindowsAndMessaging::*,
    },
    core::{HSTRING, w},
};

fn main() {
    let args: Vec<_> = std::env::args().collect();
    let output = args
        .get(2)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("C:/glaze-targets/tiling/evidence"));
    let _ = std::fs::create_dir_all(&output);
    let log = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(output.join("fixture.log"))
        .unwrap();
    env_logger::Builder::new()
        .filter_level(log::LevelFilter::Info)
        .target(env_logger::Target::Pipe(Box::new(log)))
        .init();
    glaze_desktop::enable_dpi_awareness();
    let result = if args.get(1).is_some_and(|s| s == "--child") {
        child(
            &output,
            args.get(3).and_then(|s| s.parse().ok()).unwrap_or(4),
        )
    } else if args.get(1).is_some_and(|s| s == "--recover") {
        recover(&output)
    } else {
        run(
            &output,
            args.get(3).and_then(|s| s.parse().ok()).unwrap_or(4),
            args.get(4).is_some_and(|s| s == "idle"),
            args.get(5).is_some_and(|s| s == "off"),
            args.get(1).is_some_and(|s| s == "--crash"),
            args.get(1).is_some_and(|s| s == "--cases"),
            args.get(1).is_some_and(|s| s == "--bench"),
        )
    };
    if let Err(e) = result {
        let _ = std::fs::write(output.join("error.txt"), format!("{e:#}"));
        log::error!("fixture: {e:#}");
        std::process::exit(1);
    }
    let _ = std::fs::write(output.join("done.txt"), "ok");
}
struct Process(Child);
impl Drop for Process {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn run(
    output: &Path,
    count: usize,
    idle: bool,
    off: bool,
    crash: bool,
    cases: bool,
    benchmark: bool,
) -> Result<()> {
    ensure!(
        glaze_shell::tiling::current_integrity()? == 0x2000,
        "fixture must run at medium integrity"
    );
    let foreground = unsafe { GetForegroundWindow() };
    let child = Process(
        Command::new(std::env::current_exe()?)
            .arg("--child")
            .arg(output)
            .arg(count.to_string())
            .spawn()?,
    );
    let deadline = Instant::now() + Duration::from_secs(10);
    let manifest = output.join("windows.json");
    while !manifest.exists() {
        ensure!(
            Instant::now() < deadline,
            "fixture child did not publish windows"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
    let handles: Vec<isize> = serde_json::from_slice(&std::fs::read(&manifest)?)?;
    let identities: Vec<_> = handles
        .iter()
        .map(|&h| Identity::read(HWND(h as *mut _)))
        .collect::<Result<_>>()?;
    ensure!(
        identities.iter().all(|i| i.pid == child.0.id()),
        "fixture identity outside child PID"
    );
    let originals: Vec<_> = identities
        .iter()
        .map(|id| Ok((*id, outer_rect(*id)?)))
        .collect::<Result<_>>()?;
    std::fs::write(
        output.join("originals.json"),
        serde_json::to_vec(&originals)?,
    )?;
    // SAFETY: no worker exists yet; this fixture's environment is private to its process.
    unsafe {
        std::env::set_var("GLAZE_TILING_PIDS", child.0.id().to_string());
        std::env::set_var("GLAZE_TILING_JOURNAL", output.join("recovery.json"));
        std::env::set_var("APPDATA", output);
        std::env::set_var("GLAZE_TILING_STATS", "1");
    }
    let _com = glaze_desktop::desktop_view::Com::init();
    let _source = WinEvents::install();
    let mut tiler = if off {
        None
    } else {
        Some(Tiler::start(Settings {
            animations: false,
            ..Settings::default()
        })?)
    };
    pump(&mut tiler, Duration::from_millis(500));
    ensure!(
        unsafe { GetForegroundWindow() } == foreground,
        "foreground changed during setup"
    );
    if let Some(tiler) = &tiler {
        ensure!(
            tiler.placements().len() == count,
            "not all fixture windows were admitted"
        );
        verify_placements(tiler)?;
    }
    if crash {
        std::fs::write(
            output.join("crash-ready.txt"),
            (foreground.0 as isize).to_string(),
        )?;
        std::process::exit(77);
    }
    let before = cpu()?;
    let start = Instant::now();
    let wakes = pump(
        &mut tiler,
        if idle {
            Duration::from_secs(60)
        } else {
            Duration::from_secs(2)
        },
    );
    let seconds = start.elapsed().as_secs_f64();
    let after = cpu()?;
    let mut memory = PROCESS_MEMORY_COUNTERS_EX::default();
    memory.cb = size_of::<PROCESS_MEMORY_COUNTERS_EX>() as u32;
    unsafe {
        K32GetProcessMemoryInfo(
            GetCurrentProcess(),
            (&mut memory as *mut PROCESS_MEMORY_COUNTERS_EX).cast(),
            size_of::<PROCESS_MEMORY_COUNTERS_EX>() as u32,
        )
        .ok()?;
    }
    let mut report = serde_json::json!({"count":count,"off":off,"seconds":seconds,"cpu_ms":(after-before) as f64/10000.0,"one_core_percent":(after-before) as f64/100000.0/seconds,"private_bytes":memory.PrivateUsage,"message_wakes":wakes,"foreground_before":foreground.0 as isize});
    if cases {
        exercise_cases(output, &mut tiler, &identities).context("running window cases")?;
        report["window_cases"] = true.into();
    }
    if !idle {
        if let Some(t) = &mut tiler {
            t.command(Op::Focus(Direction::Right));
            t.command(Op::Maximize);
            t.command(Op::Maximize);
        }
        pump(&mut tiler, Duration::from_millis(200));
        if let Some(t) = &tiler {
            verify_placements(t)?;
            let monitors = glaze_desktop::monitors();
            let placements = t.placements();
            let mut visible = Vec::new();
            for (identity, r) in placements {
                if monitors
                    .iter()
                    .any(|m| m.contains_point(r.x as i32, r.y as i32))
                {
                    let i = visible.len();
                    capture(identity, &output.join(format!("tiled-{i}.png")))
                        .with_context(|| format!("capturing {identity:?}"))?;
                    visible.push((identity, r));
                }
            }
            std::fs::write(output.join("visible.json"), serde_json::to_vec(&visible)?)?;
        }
        drop(tiler.take());
        // All original rectangles must be back before standalone animation measurements.
        ensure!(
            !output.join("recovery.json").exists(),
            "recovery journal remained after off"
        );
        for (identity, original) in &originals {
            ensure!(
                outer_rect(*identity)? == *original,
                "off did not restore original bounds"
            );
        }
        let mut animator = Animator::new();
        let counts = if benchmark {
            ensure!(count == 30, "benchmark needs 30 fixture windows");
            vec![4, 10, 30]
        } else {
            vec![count]
        };
        for count in counts {
            for _ in 0..if benchmark { 3 } else { 1 } {
                let mut changes = Vec::new();
                for identity in identities.iter().take(count) {
                    let mut outer = RECT::default();
                    unsafe {
                        GetWindowRect(identity.window(), &mut outer)?;
                    }
                    let mut frame = RECT::default();
                    unsafe {
                        windows::Win32::Graphics::Dwm::DwmGetWindowAttribute(
                            identity.window(),
                            windows::Win32::Graphics::Dwm::DWMWA_EXTENDED_FRAME_BOUNDS,
                            (&mut frame as *mut RECT).cast(),
                            size_of::<RECT>() as u32,
                        )?;
                    }
                    let from = Rect {
                        x: f64::from(frame.left),
                        y: f64::from(frame.top),
                        w: f64::from(frame.right - frame.left),
                        h: f64::from(frame.bottom - frame.top),
                    };
                    changes.push(Change {
                        identity: *identity,
                        from,
                        to: Rect {
                            x: from.x + 40.0,
                            y: from.y + 20.0,
                            ..from
                        },
                        border: [
                            f64::from(outer.left) - from.x,
                            f64::from(outer.top) - from.y,
                            f64::from(outer.right - outer.left) - from.w,
                            f64::from(outer.bottom - outer.top) - from.h,
                        ],
                        visible: true,
                        dpi_changed: false,
                    });
                }
                let mut journal =
                    tiling_recovery::Journal::open(output.join("animation-recovery.json"))?;
                for identity in &identities {
                    journal.remember(*identity)?;
                }
                animator.apply(changes, true)?;
                pump(&mut None, Duration::from_secs(1));
                animator.cancel();
                journal.restore()?;
                report["restored"] = true.into();
            }
        }
        report["animation"] = serde_json::to_value(glaze_shell::tiling::animation::measurements())?;
    }
    drop(tiler);
    tiling_recovery::restore_pending()?;
    report["foreground_after"] = (unsafe { GetForegroundWindow() }.0 as isize).into();
    std::fs::write(
        output.join("report.json"),
        serde_json::to_vec_pretty(&report)?,
    )?;
    ensure!(
        unsafe { GetForegroundWindow() } == foreground,
        "foreground changed during fixture"
    );
    drop(child);
    Ok(())
}
fn outer_rect(identity: Identity) -> Result<[i32; 4]> {
    let mut r = RECT::default();
    unsafe {
        GetWindowRect(identity.window(), &mut r)
            .with_context(|| format!("reading outer bounds of {identity:?}"))?;
    }
    Ok([r.left, r.top, r.right, r.bottom])
}
fn verify_placements(tiler: &Tiler) -> Result<()> {
    for (identity, expected) in tiler.placements() {
        let mut actual = RECT::default();
        unsafe {
            windows::Win32::Graphics::Dwm::DwmGetWindowAttribute(
                identity.window(),
                windows::Win32::Graphics::Dwm::DWMWA_EXTENDED_FRAME_BOUNDS,
                (&mut actual as *mut RECT).cast(),
                size_of::<RECT>() as u32,
            )
            .with_context(|| format!("reading placement of {identity:?}"))?;
        }
        ensure!(
            (f64::from(actual.left) - expected.x).abs() < 2.0
                && (f64::from(actual.top) - expected.y).abs() < 2.0
                && (f64::from(actual.right - actual.left) - expected.w).abs() < 2.0
                && (f64::from(actual.bottom - actual.top) - expected.h).abs() < 2.0,
            "actual frame did not match layout for {:?}",
            identity
        );
    }
    Ok(())
}
fn recover(output: &Path) -> Result<()> {
    ensure!(
        glaze_shell::tiling::current_integrity()? == 0x2000,
        "recovery fixture must be medium integrity"
    );
    let originals: Vec<(Identity, [i32; 4])> =
        serde_json::from_slice(&std::fs::read(output.join("originals.json"))?)?;
    let child_pid: u32 = std::fs::read_to_string(output.join("child.pid"))?.parse()?;
    ensure!(
        originals
            .iter()
            .all(|(id, _)| id.pid == child_pid && id.valid()),
        "recovery only accepts the fixture's live child"
    );
    #[derive(serde::Deserialize)]
    struct Recorded {
        identity: Identity,
    }
    #[derive(serde::Deserialize)]
    struct TestJournal {
        windows: Vec<Recorded>,
        snap: Option<bool>,
    }
    let recorded: TestJournal =
        serde_json::from_slice(&std::fs::read(output.join("recovery.json"))?)?;
    ensure!(
        recorded.snap.is_none()
            && recorded
                .windows
                .iter()
                .all(|e| originals.iter().any(|(id, _)| *id == e.identity)),
        "recovery journal contains a non-fixture mutation"
    );
    unsafe {
        std::env::set_var("GLAZE_TILING_PIDS", child_pid.to_string());
    }
    let foreground = unsafe { GetForegroundWindow() };
    tiling_recovery::restore_at(&output.join("recovery.json"))?;
    for (id, original) in originals {
        ensure!(
            outer_rect(id)? == original,
            "crash recovery did not restore original bounds"
        );
    }
    ensure!(
        unsafe { GetForegroundWindow() } == foreground,
        "recovery changed foreground"
    );
    std::fs::write(
        output.join("report.json"),
        serde_json::to_vec_pretty(
            &serde_json::json!({"crash_recovery":true,"foreground_before":foreground.0 as isize,"foreground_after":unsafe{GetForegroundWindow()}.0 as isize}),
        )?,
    )?;
    Ok(())
}
fn cpu() -> Result<u64> {
    let (mut created, mut exited, mut kernel, mut user) = (
        FILETIME::default(),
        FILETIME::default(),
        FILETIME::default(),
        FILETIME::default(),
    );
    unsafe {
        GetProcessTimes(
            GetCurrentProcess(),
            &mut created,
            &mut exited,
            &mut kernel,
            &mut user,
        )?;
    }
    Ok(
        (u64::from(kernel.dwHighDateTime) << 32 | u64::from(kernel.dwLowDateTime))
            + (u64::from(user.dwHighDateTime) << 32 | u64::from(user.dwLowDateTime)),
    )
}
fn pump(tiler: &mut Option<Tiler>, duration: Duration) -> u64 {
    let end = Instant::now() + duration;
    let mut wakes = 0;
    let mut msg = MSG::default();
    while Instant::now() < end {
        unsafe {
            MsgWaitForMultipleObjectsEx(
                None,
                end.saturating_duration_since(Instant::now())
                    .as_millis()
                    .max(1)
                    .min(u128::from(u32::MAX - 1)) as u32,
                QS_ALLINPUT,
                MWMO_INPUTAVAILABLE,
            );
            while PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
                DispatchMessageW(&msg);
                wakes += 1;
            }
        }
        if let Some(tiler) = tiler {
            tiler.process();
        }
    }
    wakes
}

fn child(output: &Path, count: usize) -> Result<()> {
    CHILD_OUTPUT.with(|path| *path.borrow_mut() = output.into());
    std::fs::write(output.join("child.pid"), std::process::id().to_string())?;
    let front = unsafe { GetForegroundWindow() };
    let monitors = glaze_desktop::monitor_infos();
    let mut front_bounds = RECT::default();
    // Read geometry only. Visible frame bounds avoid treating an invisible maximize border
    // on the adjacent monitor as foreground content.
    unsafe {
        if windows::Win32::Graphics::Dwm::DwmGetWindowAttribute(
            front,
            windows::Win32::Graphics::Dwm::DWMWA_EXTENDED_FRAME_BOUNDS,
            (&mut front_bounds as *mut RECT).cast(),
            size_of::<RECT>() as u32,
        )
        .is_err()
        {
            GetWindowRect(front, &mut front_bounds).context("reading foreground placement")?;
        }
    }
    // SAFETY: cached desktop HWNDs, without messages or activation.
    let desktop = unsafe { front == GetDesktopWindow() || front == GetShellWindow() };
    let monitor = monitors
        .iter()
        .find(|m| {
            desktop
                || !(m.rect.x < front_bounds.right
                    && front_bounds.left < m.rect.x + m.rect.width
                    && m.rect.y < front_bounds.bottom
                    && front_bounds.top < m.rect.y + m.rect.height)
        })
        .context("no monitor clear of foreground content; refusing the live fixture")?;
    let mut handles = Vec::new();
    unsafe {
        let class = WNDCLASSEXW {
            cbSize: size_of::<WNDCLASSEXW>() as u32,
            lpfnWndProc: Some(window_proc),
            hInstance: windows::Win32::System::LibraryLoader::GetModuleHandleW(None)?.into(),
            lpszClassName: w!("TilingFixtureApp"),
            ..Default::default()
        };
        RegisterClassExW(&class);
        for i in 0..count {
            let title = HSTRING::from(format!("Glaze test window {}", i + 1));
            let window = CreateWindowExW(
                WINDOW_EX_STYLE(0),
                w!("TilingFixtureApp"),
                &title,
                WS_OVERLAPPEDWINDOW,
                monitor.work.x + 40 + (i % 4) as i32 * 40,
                monitor.work.y + 80 + (i % 4) as i32 * 35,
                500,
                420,
                None,
                None,
                None,
                None,
            )?;
            SetWindowLongPtrW(window, GWLP_USERDATA, i as isize + 1);
            let _ = ShowWindow(window, SW_SHOWNOACTIVATE);
            let _ = UpdateWindow(window);
            handles.push(window.0 as isize);
        }
    }
    std::fs::write(output.join("windows.json"), serde_json::to_vec(&handles)?)?;
    let mut msg = MSG::default();
    unsafe {
        while GetMessageW(&mut msg, None, 0, 0).0 > 0 {
            DispatchMessageW(&msg);
        }
        for h in handles {
            let _ = DestroyWindow(HWND(h as *mut _));
        }
    }
    Ok(())
}
unsafe extern "system" fn window_proc(h: HWND, msg: u32, w: WPARAM, l: LPARAM) -> LRESULT {
    if msg == WM_APP + 92 {
        CHILD_OUTPUT.with(|path| {
            let _ = std::fs::write(path.borrow().join("hung-ready.txt"), "ready");
        });
        // Deliberately hang only this fixture child's UI thread, never another application.
        std::thread::sleep(Duration::from_secs(2));
        return LRESULT(0);
    }
    if msg == WM_APP + 91 {
        // SAFETY: this is the fixture's own HWND, on its own thread. The app publishes its
        // late name as Firefox/JetBrains do, without another process generating the event.
        unsafe {
            let style = GetWindowLongPtrW(h, GWL_STYLE) as u32;
            SetWindowLongPtrW(h, GWL_STYLE, (style | WS_CAPTION.0) as isize);
            let _ = SetWindowPos(
                h,
                None,
                0,
                0,
                0,
                0,
                SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE | SWP_FRAMECHANGED,
            );
            let _ = SetWindowTextW(h, w!("Late named fixture window"));
            windows::Win32::UI::Accessibility::NotifyWinEvent(
                EVENT_OBJECT_NAMECHANGE,
                h,
                OBJID_WINDOW.0,
                CHILDID_SELF as i32,
            );
        }
        return LRESULT(0);
    }
    if msg == WM_APP + 90 {
        if let Err(e) = extra_window(h, w.0) {
            CHILD_OUTPUT.with(|path| {
                let _ = std::fs::write(path.borrow().join("error.txt"), format!("{e:#}"));
            });
        }
        return LRESULT(0);
    }
    if msg == WM_GETMINMAXINFO && unsafe { GetWindowLongPtrW(h, GWLP_USERDATA) } == 1007 {
        // SAFETY: WM_GETMINMAXINFO supplies a live MINMAXINFO; this window is ours.
        unsafe {
            (*(l.0 as *mut MINMAXINFO)).ptMinTrackSize.x = 1600;
        }
        return LRESULT(0);
    }
    if msg == WM_MOUSEACTIVATE {
        return LRESULT(MA_NOACTIVATE as isize);
    }
    if msg == WM_PAINT {
        unsafe {
            let mut ps = PAINTSTRUCT::default();
            let dc = BeginPaint(h, &mut ps);
            let index = GetWindowLongPtrW(h, GWLP_USERDATA);
            let colors = [0x00854f25, 0x00476789, 0x006b4f8e, 0x005f7741];
            let brush = CreateSolidBrush(windows::Win32::Foundation::COLORREF(
                colors[index as usize % colors.len()],
            ));
            FillRect(dc, &ps.rcPaint, brush);
            let _ = DeleteObject(HGDIOBJ(brush.0));
            let mut client = RECT::default();
            let _ = GetClientRect(h, &mut client);
            SetTextColor(dc, windows::Win32::Foundation::COLORREF(0x00ffffff));
            SetBkMode(dc, TRANSPARENT);
            let mut text: Vec<u16> =
                format!("Scrolling tiling\n\nWindow {index}\n\nCreated by the scoped fixture")
                    .encode_utf16()
                    .collect();
            let _ = DrawTextW(
                dc,
                &mut text,
                &mut client,
                DT_CENTER | DT_VCENTER | DT_WORDBREAK,
            );
            let _ = EndPaint(h, &ps);
        }
        return LRESULT(0);
    }
    unsafe { DefWindowProcW(h, msg, w, l) }
}
thread_local! {static CHILD_OUTPUT:std::cell::RefCell<PathBuf>=std::cell::RefCell::new(PathBuf::new());}
/// Additional windows are created on the child's existing UI thread without input or activation.
fn extra_window(owner: HWND, kind: usize) -> Result<()> {
    let mut parent = RECT::default();
    unsafe {
        GetWindowRect(owner, &mut parent)?;
    }
    let mut style = WS_OVERLAPPEDWINDOW;
    let mut ex = WINDOW_EX_STYLE(0);
    match kind {
        1 => style &= !WS_THICKFRAME,
        3 => ex |= WS_EX_TOPMOST,
        4 => {
            style = WS_POPUP;
            ex = WS_EX_TOOLWINDOW;
        }
        5 => ex |= WS_EX_LAYERED,
        6 => style = WS_POPUP | WS_THICKFRAME | WS_SYSMENU | WS_MINIMIZEBOX | WS_MAXIMIZEBOX,
        _ => {}
    }
    let title = HSTRING::from(format!("Fixture case {kind}"));
    let window = unsafe {
        CreateWindowExW(
            ex,
            w!("TilingFixtureApp"),
            &title,
            style,
            parent.left + 40,
            parent.top + 40,
            420,
            360,
            (kind == 2).then_some(owner),
            None,
            None,
            None,
        )?
    };
    unsafe {
        SetWindowLongPtrW(window, GWLP_USERDATA, 1000 + kind as isize);
        if kind == 5 {
            SetLayeredWindowAttributes(
                window,
                windows::Win32::Foundation::COLORREF(0),
                255,
                LWA_ALPHA,
            )?;
        }
        let _ = ShowWindow(window, SW_SHOWNOACTIVATE);
        let _ = UpdateWindow(window);
    }
    CHILD_OUTPUT.with(|path| {
        std::fs::write(
            path.borrow().join(format!("extra-{kind}.json")),
            serde_json::to_vec(&(window.0 as isize))?,
        )
    })?;
    Ok(())
}
fn exercise_cases(output: &Path, tiler: &mut Option<Tiler>, windows: &[Identity]) -> Result<()> {
    let controller = windows[0];
    let mut extras = Vec::new();
    let initial = tiler.as_ref().unwrap().placements();
    for kind in 1..=7 {
        unsafe {
            PostMessageW(
                Some(controller.window()),
                WM_APP + 90,
                WPARAM(kind),
                LPARAM(0),
            )?;
        }
        let path = output.join(format!("extra-{kind}.json"));
        let deadline = Instant::now() + Duration::from_secs(3);
        while !path.exists() {
            ensure!(Instant::now() < deadline, "extra window timed out");
            pump(tiler, Duration::from_millis(50));
        }
        let raw: isize = serde_json::from_slice(&std::fs::read(path)?)?;
        let identity = Identity::read(HWND(raw as *mut _))?;
        ensure!(
            identity.pid == controller.pid,
            "case window outside fixture"
        );
        extras.push(identity);
        pump(tiler, Duration::from_millis(300));
        let managed = tiler
            .as_ref()
            .unwrap()
            .placements()
            .iter()
            .any(|(id, _)| *id == identity);
        ensure!(
            managed == matches!(kind, 1 | 2 | 7),
            "incorrect admission for case {kind}"
        );
        if matches!(kind, 1 | 2) {
            let r = outer_rect(identity)?;
            ensure!(
                r[2] - r[0] == 420 && r[3] - r[1] == 360,
                "fixed/owned window was resized"
            );
        }
    }
    // A minimum larger than half the screen widens its own column, without resizing siblings.
    let large = extras[6];
    ensure!(
        outer_rect(large)?[2] - outer_rect(large)?[0] >= 1600,
        "minimum width was not learned"
    );
    let current = tiler.as_ref().unwrap().placements();
    for (id, before) in initial {
        let after = current
            .iter()
            .find(|(candidate, _)| *candidate == id)
            .unwrap()
            .1;
        ensure!(
            before.w == after.w && before.h == after.h,
            "opening resized a sibling"
        );
    }
    let late = extras[5];
    unsafe {
        PostMessageW(Some(late.window()), WM_APP + 91, WPARAM(0), LPARAM(0))?;
    }
    pump(tiler, Duration::from_millis(500));
    ensure!(
        tiler
            .as_ref()
            .unwrap()
            .placements()
            .iter()
            .any(|(id, _)| *id == late),
        "late NAMECHANGE was not admitted"
    );
    let tray = windows[1];
    unsafe {
        let _ = ShowWindowAsync(tray.window(), SW_HIDE);
    }
    pump(tiler, Duration::from_millis(300));
    ensure!(
        !tiler
            .as_ref()
            .unwrap()
            .placements()
            .iter()
            .any(|(id, _)| *id == tray),
        "HIDE retained its column"
    );
    unsafe {
        let _ = ShowWindowAsync(tray.window(), SW_SHOWNOACTIVATE);
    }
    pump(tiler, Duration::from_millis(400));
    ensure!(
        tiler
            .as_ref()
            .unwrap()
            .placements()
            .iter()
            .any(|(id, _)| *id == tray),
        "tray restore was not admitted"
    );
    // The real display-change grace period must expire before testing a user minimize.
    pump(tiler, Duration::from_secs(11));
    unsafe {
        let _ = ShowWindowAsync(tray.window(), SW_SHOWMINNOACTIVE);
    }
    pump(tiler, Duration::from_millis(300));
    ensure!(
        !tiler
            .as_ref()
            .unwrap()
            .placements()
            .iter()
            .any(|(id, _)| *id == tray),
        "minimize retained its column"
    );
    unsafe {
        let _ = ShowWindowAsync(tray.window(), SW_SHOWNOACTIVATE);
    }
    pump(tiler, Duration::from_millis(400));
    ensure!(
        tiler
            .as_ref()
            .unwrap()
            .placements()
            .iter()
            .any(|(id, _)| *id == tray),
        "minimize restore was not admitted"
    );
    unsafe {
        PostMessageW(Some(controller.window()), WM_APP + 92, WPARAM(0), LPARAM(0))?;
    }
    let deadline = Instant::now() + Duration::from_secs(1);
    while !output.join("hung-ready.txt").exists() {
        ensure!(Instant::now() < deadline, "hung fixture did not start");
        pump(tiler, Duration::from_millis(20));
    }
    tiler.as_mut().unwrap().command(Op::Maximize);
    pump(tiler, Duration::from_millis(300));
    ensure!(
        !glaze_shell::tiling::animation::busy(),
        "hung client blocked native projection"
    );
    pump(tiler, Duration::from_secs(2));
    verify_placements(tiler.as_ref().unwrap())?;
    tiler.as_mut().unwrap().command(Op::Maximize);
    pump(tiler, Duration::from_millis(300));
    let t = tiler.as_mut().unwrap();
    t.command(Op::MoveWorkspace(Direction::Down));
    t.command(Op::Overview);
    pump(tiler, Duration::from_millis(250));
    tiler.as_mut().unwrap().command(Op::Overview);
    tiler
        .as_mut()
        .unwrap()
        .command(Op::MoveWorkspace(Direction::Up));
    pump(tiler, Duration::from_millis(400));
    verify_placements(tiler.as_ref().unwrap())?;
    for id in &extras {
        unsafe {
            PostMessageW(Some(id.window()), WM_CLOSE, WPARAM(0), LPARAM(0))
                .with_context(|| format!("closing case {id:?}"))?;
        }
    }
    pump(tiler, Duration::from_millis(400));
    ensure!(
        tiler
            .as_ref()
            .unwrap()
            .placements()
            .iter()
            .all(|(id, _)| !extras.contains(id)),
        "destroyed case windows remained in the PID-scoped tiler"
    );
    Ok(())
}
fn capture(identity: Identity, path: &Path) -> Result<()> {
    ensure!(identity.valid(), "capture identity changed");
    unsafe {
        let mut r = RECT::default();
        GetWindowRect(identity.window(), &mut r)?;
        let width = r.right - r.left;
        let height = r.bottom - r.top;
        ensure!(
            width > 0 && height > 0 && width < 8192 && height < 8192,
            "invalid capture dimensions"
        );
        let dc = GetWindowDC(Some(identity.window()));
        let memory = CreateCompatibleDC(Some(dc));
        let info = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: width,
                biHeight: -height,
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut bits = std::ptr::null_mut();
        let bitmap = CreateDIBSection(Some(dc), &info, DIB_RGB_COLORS, &mut bits, None, 0)?;
        let old = SelectObject(memory, HGDIOBJ(bitmap.0));
        let printed = windows::Win32::Storage::Xps::PrintWindow(
            identity.window(),
            memory,
            windows::Win32::Storage::Xps::PRINT_WINDOW_FLAGS(2),
        )
        .as_bool();
        let mut rgba =
            std::slice::from_raw_parts(bits.cast::<u8>(), (width * height * 4) as usize).to_vec();
        for pixel in rgba.chunks_exact_mut(4) {
            pixel.swap(0, 2);
            pixel[3] = 255;
        }
        SelectObject(memory, old);
        let _ = DeleteObject(HGDIOBJ(bitmap.0));
        let _ = DeleteDC(memory);
        ReleaseDC(Some(identity.window()), dc);
        ensure!(printed, "PrintWindow failed");
        let mut encoder =
            png::Encoder::new(std::fs::File::create(path)?, width as u32, height as u32);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        encoder.write_header()?.write_image_data(&rgba)?;
    }
    Ok(())
}
