//! Live N4 harness. Run under the desktop lock with isolated APPDATA/LOCALAPPDATA. It uses the
//! production widget host and services, six instances across the real monitors, plus a warmed
//! native menu. It never stops the user's Glaze or changes taskbar state.
use glaze_shell::{bars, inbox, integrations, widgets};
// Use the product's performance harness math, including wall time normalization.
#[allow(dead_code)]
#[path = "../../../../tools/perf/src/cpu.rs"]
mod perf_cpu;

use anyhow::Result;
use glaze_ui::{Entry, Menu, Tokens, Ui, next_event};
use glaze_widgets::{files, layout::Layout, registry, storage::Storage};
use std::time::{Duration, Instant};
use windows::Win32::Foundation::WAIT_OBJECT_0;
use windows::Win32::UI::WindowsAndMessaging::*;

fn main() -> Result<()> {
    anyhow::ensure!(
        std::env::var_os("GLAZE_N4_FIXTURE").is_some(),
        "set GLAZE_N4_FIXTURE and isolated APPDATA before running"
    );
    glaze_desktop::init_logging();
    glaze_desktop::enable_dpi_awareness();
    let _restore = RestoreInput::new();
    let _awake = Awake::new()?;
    let seconds = std::env::args()
        .nth(1)
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(240);
    let monitors = glaze_desktop::monitor_infos();
    anyhow::ensure!(!monitors.is_empty(), "no monitors");
    let registry = registry::builtins();
    let mut layout = Layout::default();
    layout.appearance.style = Some(glaze_theme::WidgetStyle::Liquid);
    for (i, id) in [
        "clock",
        "calendar",
        "now-playing",
        "weather",
        "system-monitor",
        "notes",
    ]
    .into_iter()
    .enumerate()
    {
        let monitor = &monitors[i % monitors.len()];
        let row = i / monitors.len();
        let instance = layout.add(
            id,
            &monitor.device,
            [18, row as u32 * 3],
            registry.get(id).unwrap().manifest.size.default,
        );
        if id == "weather" {
            layout.set_settings(
                &instance,
                serde_json::from_value(
                    serde_json::json!({"city":{"name":"Oslo","lat":59.91,"lon":10.75}}),
                )?,
            )?;
        }
    }
    layout.save(&files::layout()?)?;
    Storage::new(files::user("widget-data")?).set(
        "notes-1",
        "text",
        Some(serde_json::json!(
            "A native desktop\nمرحبا بالعالم\n日本語 中文 한국어"
        )),
    )?;
    let mut ui = Ui::new()?;
    let _media_pause = PausedMedia::new();
    if std::env::var_os("GLAZE_N4_SEED_ONLY").is_some() {
        return seed_only(seconds, monitors.len());
    }
    let inbox = inbox::Inbox::new()?;
    let mut host = widgets::Widgets::new(&mut ui, inbox.hwnd(), &[])?;
    let mut menu = Menu::<u8>::new("N4 fixture menu")?;
    let screen = glaze_ui::screen_at(monitors[0].work.x + 40, monitors[0].work.y + 40);
    menu.open(
        &mut ui,
        vec![
            Entry::action("View", 0),
            Entry::action("Sort by", 1),
            Entry::action("Refresh", 2),
        ],
        (monitors[0].work.x + 40, monitors[0].work.y + 40),
        screen,
        &Tokens::default(),
    )?;
    menu.close();
    if std::env::var_os("GLAZE_N4_INTERACTIONS").is_some() {
        exercise(&mut ui, &mut host)?;
    }
    let mut bars = if std::env::var_os("GLAZE_N4_BARS").is_some() {
        Some(bars::Bars::new(&mut ui, bars::prepare()?)?)
    } else {
        None
    };
    let _visible_desktop = if std::env::var_os("GLAZE_N4_DESKTOP_VISIBLE").is_some() {
        Some(VisibleDesktop::new()?)
    } else {
        None
    };
    pump_for(&mut ui, &mut host, Duration::from_millis(500));
    println!(
        "N4_READY pid={} monitors={} seconds={seconds}",
        std::process::id(),
        monitors.len()
    );
    println!(
        "N4_MONITORS {:?}",
        monitors
            .iter()
            .map(|m| (&m.device, m.dpi))
            .collect::<Vec<_>>()
    );
    let stop_name = format!("Local\\glaze-n4-fixture-stop-{}", std::process::id());
    // SAFETY: an event owned by this harness, signalled by measure.ps1 after its last sample.
    let stop = unsafe {
        windows::Win32::System::Threading::CreateEventW(
            None,
            true,
            false,
            &windows::core::HSTRING::from(stop_name),
        )
    }?;
    // SAFETY: this named auto-reset event requests diagnostics only outside CPU windows.
    let snapshot = unsafe {
        windows::Win32::System::Threading::CreateEventW(
            None,
            false,
            false,
            &windows::core::HSTRING::from(format!(
                "Local\\glaze-n4-fixture-snapshot-{}",
                std::process::id()
            )),
        )
    }?;
    let started = Instant::now();
    let deadline = Instant::now() + Duration::from_secs(seconds);
    let profiling = std::env::var_os("GLAZE_N4_PROFILE").is_some();
    let mut profile_phase = 0;
    let mut sample_at = if profiling {
        Instant::now() + Duration::from_secs(20)
    } else {
        deadline
    };
    let mut sample: Option<(Instant, u64, u64)> = None;
    let mut samples = Vec::new();
    let mut message = MSG::default();
    loop {
        let timeout = host
            .timeout_ms()
            .min(bars.as_ref().map_or(u32::MAX, bars::Bars::timeout))
            .min(
                sample_at
                    .saturating_duration_since(Instant::now())
                    .as_nanos()
                    .div_ceil(1_000_000)
                    .min(u128::from(u32::MAX - 1)) as u32,
            )
            .min(
                deadline
                    .saturating_duration_since(Instant::now())
                    .as_nanos()
                    .div_ceil(1_000_000)
                    .min(u128::from(u32::MAX - 1)) as u32,
            );
        // SAFETY: the ticker handle and all windows live through this standard UI pump.
        let handles: Vec<_> = [host.timer_handle(), stop, snapshot]
            .into_iter()
            .chain(bars.as_ref().into_iter().flat_map(bars::Bars::handles))
            .collect();
        // SAFETY: every handle and the message queue belong to this live fixture.
        let woke = unsafe {
            MsgWaitForMultipleObjectsEx(Some(&handles), timeout, QS_ALLINPUT, MWMO_INPUTAVAILABLE)
        };
        if woke.0 == WAIT_OBJECT_0.0 + 1 {
            break;
        }
        if woke.0 == WAIT_OBJECT_0.0 + 2 {
            checkpoint(&host, started);
            continue;
        }
        if woke == WAIT_OBJECT_0 {
            host.frame_ready();
        }
        // SAFETY: ordinary dispatch on the owning UI thread.
        unsafe {
            while PeekMessageW(&mut message, None, 0, 0, PM_REMOVE).as_bool() {
                if message.message == WM_QUIT {
                    return Ok(());
                }
                host.raw_message(&message);
                if let Some(bars) = &mut bars {
                    bars.message(&message);
                }
                if message.hwnd.is_invalid() && message.message == glaze_desktop::WM_FACT {
                    host.fact(message.wParam.0, message.lParam.0);
                    if let Some(bars) = &mut bars {
                        bars.fact(message.wParam.0, message.lParam.0);
                    }
                }
                let _ = TranslateMessage(&message);
                DispatchMessageW(&message);
            }
        }
        while let Some(message) = inbox::next() {
            host.handle(&mut ui, message);
        }
        while let Some((surface, event)) = next_event() {
            host.on_event(&mut ui, surface, &event);
            if let Some(bars) = &mut bars {
                bars.event(&mut ui, surface, &event);
            }
        }
        if integrations::process()
            && let Some(bars) = &mut bars
        {
            bars.integration_changed();
        }
        host.poll(&mut ui);
        if let Some(bars) = &mut bars {
            bars.tick(&mut ui);
        }
        if profiling && Instant::now() >= sample_at {
            let at = Instant::now();
            let busy = busy_ticks(false);
            let ui_busy = busy_ticks(true);
            if let Some((start, before, ui_before)) = sample {
                let cpu =
                    perf_cpu::cpu_use(Duration::from_nanos((busy - before) * 100), at - start, 16);
                println!(
                    "N4_SAMPLE wall_s={:.3} cpu_core_pct={:.5} counters={:?}",
                    (at - start).as_secs_f64(),
                    cpu.one_core,
                    host.counters()
                );
                println!("N4_COST {:?}", host.costs());
                let ui_cpu = perf_cpu::cpu_use(
                    Duration::from_nanos((ui_busy - ui_before) * 100),
                    at - start,
                    16,
                );
                println!(
                    "N4_THREADS ui_core_pct={:.5} worker_core_pct={:.5}",
                    ui_cpu.one_core,
                    (cpu.one_core - ui_cpu.one_core).max(0.0)
                );
                if profiling {
                    println!(
                        "N4_PROFILE phase={} cpu_core_pct={:.5}",
                        ["six", "without-system", "bars-only"][profile_phase],
                        cpu.one_core
                    );
                }
                let probes = host.live_windows();
                // SAFETY: read-only hit queries at coordinates of the fixture's own windows.
                let hits=probes.iter().filter(|p|unsafe {WindowFromPoint(windows::Win32::Foundation::POINT {x:((p.rect.left+p.rect.right)/2.0) as i32,y:((p.rect.top+p.rect.bottom)/2.0) as i32})}==p.hwnd).count();
                println!("N4_VISIBLE_HITS {hits}/{}", probes.len());
                if hits != probes.len() {
                    for probe in &probes {
                        let mut name = [0u16; 128];
                        // SAFETY: read-only queries of the observed window at our surface's center.
                        let (len, hit) = unsafe {
                            let hit = WindowFromPoint(windows::Win32::Foundation::POINT {
                                x: ((probe.rect.left + probe.rect.right) / 2.0) as i32,
                                y: ((probe.rect.top + probe.rect.bottom) / 2.0) as i32,
                            });
                            (GetClassNameW(hit, &mut name), hit)
                        };
                        println!(
                            "N4_OBSCURED {} hit={:?} class={} ordered={} visible={}",
                            probe.instance,
                            hit,
                            String::from_utf16_lossy(&name[..len.max(0) as usize]),
                            probe.ordered,
                            probe.visible
                        );
                    }
                }
                samples.push(cpu.one_core);
            }
            if profiling && sample.is_some() {
                profile_phase += 1;
                if profile_phase == 3 {
                    println!("N4_PROFILE_DONE");
                    break;
                }
                if profile_phase == 1 {
                    layout.instances.retain(|i| i.widget != "system-monitor");
                } else {
                    layout.instances.clear();
                }
                layout.save(&files::layout()?)?;
                host.handle(
                    &mut ui,
                    inbox::Msg {
                        message: widgets::WM_LAYOUT,
                        wparam: 0,
                        lparam: 0,
                    },
                );
                sample = None;
                sample_at = Instant::now() + Duration::from_secs(10);
                continue;
            }
            if sample.is_none() {
                println!("N4_BASELINE counters={:?}", host.counters());
                println!("N4_COST_BASELINE {:?}", host.costs());
            }
            sample = Some((at, busy, ui_busy));
            sample_at = at + Duration::from_secs(60);
        }
        if Instant::now() >= deadline {
            break;
        }
    }
    checkpoint(&host, started);
    println!("N4_DONE drivers={:?}", glaze_ui::modules::gpu_drivers());
    // SAFETY: the harness owns this event and its wait loop has ended.
    unsafe {
        let _ = windows::Win32::Foundation::CloseHandle(stop);
        let _ = windows::Win32::Foundation::CloseHandle(snapshot);
    }
    Ok(())
}

/// Seed the same six-instance fixture and hold its paused-media/visible-desktop guards while
/// the actual shell is measured. This helper creates no widget, bar or menu windows.
fn seed_only(seconds: u64, monitors: usize) -> Result<()> {
    let _visible = VisibleDesktop::new()?;
    // SAFETY: this helper owns its named stop event until the bounded wait ends.
    let stop = unsafe {
        windows::Win32::System::Threading::CreateEventW(
            None,
            true,
            false,
            &windows::core::HSTRING::from(format!(
                "Local\\glaze-n4-fixture-stop-{}",
                std::process::id()
            )),
        )
    }?;
    println!(
        "N4_READY pid={} monitors={monitors} seconds={seconds} seed_only=true",
        std::process::id()
    );
    let deadline = Instant::now() + Duration::from_secs(seconds);
    let mut message = MSG::default();
    // SAFETY: this thread owns its queue and event. Pump COM's hidden STA windows as well as
    // waiting for stop, so the media guard cannot obstruct another process's SMTC calls.
    unsafe {
        while Instant::now() < deadline {
            if MsgWaitForMultipleObjectsEx(
                Some(&[stop]),
                deadline
                    .saturating_duration_since(Instant::now())
                    .as_millis()
                    .max(1)
                    .min(u128::from(u32::MAX - 1)) as u32,
                QS_ALLINPUT,
                MWMO_INPUTAVAILABLE,
            ) == WAIT_OBJECT_0
            {
                break;
            }
            while PeekMessageW(&mut message, None, 0, 0, PM_REMOVE).as_bool() {
                let _ = TranslateMessage(&message);
                DispatchMessageW(&message);
            }
        }
        let _ = windows::Win32::Foundation::CloseHandle(stop);
    }
    println!("N4_DONE drivers={:?}", glaze_ui::modules::gpu_drivers());
    Ok(())
}

/// Window probes and logging are explicit checkpoints, never part of an external CPU sample.
fn checkpoint(host: &widgets::Widgets, started: Instant) {
    let probes = host.live_windows();
    // SAFETY: read-only hit queries at the centers of this fixture's own windows.
    let visible = probes.iter().filter(|p| unsafe { WindowFromPoint(windows::Win32::Foundation::POINT {
        x: ((p.rect.left + p.rect.right) / 2.0) as i32,
        y: ((p.rect.top + p.rect.bottom) / 2.0) as i32,
    }) } == p.hwnd).count();
    println!(
        "N4_CHECKPOINT elapsed_s={:.3} counters={:?} visible={visible}/{} costs={:?}",
        started.elapsed().as_secs_f64(),
        host.counters(),
        probes.len(),
        host.costs()
    );
}

fn busy_ticks(thread: bool) -> u64 {
    use windows::Win32::Foundation::FILETIME;
    use windows::Win32::System::Threading::{
        GetCurrentProcess, GetCurrentThread, GetProcessTimes, GetThreadTimes,
    };
    let (mut created, mut exited, mut kernel, mut user) = (
        FILETIME::default(),
        FILETIME::default(),
        FILETIME::default(),
        FILETIME::default(),
    );
    // SAFETY: own process/thread, valid FILETIME outputs.
    unsafe {
        let _ = if thread {
            GetThreadTimes(
                GetCurrentThread(),
                &mut created,
                &mut exited,
                &mut kernel,
                &mut user,
            )
        } else {
            GetProcessTimes(
                GetCurrentProcess(),
                &mut created,
                &mut exited,
                &mut kernel,
                &mut user,
            )
        };
    }
    let ticks = |t: FILETIME| (u64::from(t.dwHighDateTime) << 32) | u64::from(t.dwLowDateTime);
    ticks(kernel) + ticks(user)
}

struct RestoreInput {
    foreground: windows::Win32::Foundation::HWND,
    cursor: windows::Win32::Foundation::POINT,
}

/// Both displays must remain on throughout the live fixture. Restore this thread's previous
/// execution-state requirements on every exit; no power-plan setting is changed.
struct Awake(windows::Win32::System::Power::EXECUTION_STATE);
impl Awake {
    fn new() -> Result<Self> {
        use windows::Win32::System::Power::{
            ES_CONTINUOUS, ES_DISPLAY_REQUIRED, ES_SYSTEM_REQUIRED, SetThreadExecutionState,
        };
        // SAFETY: this is a temporary requirement owned by the fixture's main thread.
        let previous = unsafe {
            SetThreadExecutionState(ES_CONTINUOUS | ES_DISPLAY_REQUIRED | ES_SYSTEM_REQUIRED)
        };
        anyhow::ensure!(previous.0 != 0, "could not keep the fixture displays awake");
        let guard = Self(previous);
        use windows::Win32::UI::Input::KeyboardAndMouse::{
            INPUT, INPUT_0, INPUT_MOUSE, MOUSEEVENTF_MOVE, MOUSEINPUT, SendInput,
        };
        // A display requirement prevents timeout but doesn't wake a display already off.
        // SAFETY: a zero-distance mouse move wakes it without clicking or moving the cursor.
        let sent = unsafe {
            SendInput(
                &[INPUT {
                    r#type: INPUT_MOUSE,
                    Anonymous: INPUT_0 {
                        mi: MOUSEINPUT {
                            dwFlags: MOUSEEVENTF_MOVE,
                            ..Default::default()
                        },
                    },
                }],
                size_of::<INPUT>() as i32,
            )
        };
        anyhow::ensure!(sent == 1, "could not wake the fixture displays");
        Ok(guard)
    }
}
impl Drop for Awake {
    fn drop(&mut self) {
        // SAFETY: restore exactly the flags returned on this same thread at fixture startup.
        let _ = unsafe { windows::Win32::System::Power::SetThreadExecutionState(self.0) };
    }
}
impl RestoreInput {
    fn new() -> Self {
        let mut cursor = Default::default();
        // SAFETY: read-only user state, restored when the harness exits.
        unsafe {
            let _ = GetCursorPos(&mut cursor);
            Self {
                foreground: GetForegroundWindow(),
                cursor,
            }
        }
    }
}
impl Drop for RestoreInput {
    fn drop(&mut self) {
        // SAFETY: restoring the cursor and the originally foreground window, if it still lives.
        unsafe {
            let _ = SetCursorPos(self.cursor.x, self.cursor.y);
            if IsWindow(Some(self.foreground)).as_bool() {
                let _ = SetForegroundWindow(self.foreground);
            }
        }
    }
}
struct TestApp(windows::Win32::Foundation::HWND);
impl Drop for TestApp {
    fn drop(&mut self) {
        // SAFETY: a test-owned window, destroyed on its UI thread.
        unsafe {
            let _ = DestroyWindow(self.0);
        }
    }
}
struct DesktopToggle {
    shell: windows::Win32::UI::Shell::IShellDispatch4,
    toggled: bool,
}
impl Drop for DesktopToggle {
    fn drop(&mut self) {
        if self.toggled {
            // SAFETY: paired ToggleDesktop call restores the app state changed by this test.
            unsafe {
                let _ = self.shell.ToggleDesktop();
            }
        }
    }
}

fn pump_for(ui: &mut Ui, host: &mut widgets::Widgets, duration: Duration) {
    let end = Instant::now() + duration;
    let mut message = MSG::default();
    while Instant::now() < end {
        // SAFETY: standard message pumping on the thread that owns the fixture windows.
        unsafe {
            while PeekMessageW(&mut message, None, 0, 0, PM_REMOVE).as_bool() {
                host.raw_message(&message);
                if message.hwnd.is_invalid() && message.message == glaze_desktop::WM_FACT {
                    host.fact(message.wParam.0, message.lParam.0);
                }
                let _ = TranslateMessage(&message);
                DispatchMessageW(&message);
            }
        }
        while let Some(message) = inbox::next() {
            host.handle(ui, message);
        }
        while let Some((surface, event)) = next_event() {
            host.on_event(ui, surface, &event);
        }
        integrations::process();
        host.poll(ui);
        // SAFETY: bounded fixture-only wait; the production loop uses its real deadline queue.
        unsafe {
            MsgWaitForMultipleObjectsEx(None, 10, QS_ALLINPUT, MWMO_INPUTAVAILABLE);
        }
    }
}

fn exercise(ui: &mut Ui, host: &mut widgets::Widgets) -> Result<()> {
    use std::os::windows::process::CommandExt;
    use windows::Win32::Foundation::{POINT, WPARAM};
    use windows::Win32::System::Com::{CLSCTX_INPROC_SERVER, CoCreateInstance};
    use windows::Win32::UI::Input::KeyboardAndMouse::*;
    use windows::core::w;
    let monitor = &glaze_desktop::monitor_infos()[0];
    // SAFETY: built-in STATIC window class; this test owns and destroys the app window.
    let app = TestApp(unsafe {
        CreateWindowExW(
            Default::default(),
            w!("STATIC"),
            w!("N4 z-order probe app"),
            WS_OVERLAPPEDWINDOW,
            monitor.work.x + 40,
            monitor.work.y + 180,
            300,
            160,
            None,
            None,
            None,
            None,
        )
    }?);
    // SAFETY: the ordinary app window intentionally activates during this z-order test.
    unsafe {
        let _ = ShowWindow(app.0, SW_SHOWNORMAL);
    }
    for _ in 0..12 {
        // SAFETY: own test window, simulating app activation without changing user apps.
        unsafe {
            let _ = SetForegroundWindow(app.0);
        }
        pump_for(ui, host, Duration::from_millis(40));
        anyhow::ensure!(
            host.live_windows().iter().all(|w| w.visible && w.ordered),
            "widget escaped the desktop band after app activation"
        );
    }
    println!("N4_CHECK app_activation=12/12");
    // Shell's ToggleDesktop is the same Show Desktop operation as Win+D; no shortcut is sent
    // into a user's app. The paired call is protected by Drop even if a check fails.
    let shell = unsafe {
        // SAFETY: documented Shell automation interface in this UI thread's COM apartment.
        CoCreateInstance(
            &windows::Win32::UI::Shell::Shell,
            None,
            CLSCTX_INPROC_SERVER,
        )
    }?;
    let mut desktop = DesktopToggle {
        shell,
        toggled: false,
    };
    // SAFETY: requested Show Desktop check, with the inverse operation paired below.
    unsafe {
        desktop.shell.ToggleDesktop()?;
    }
    desktop.toggled = true;
    pump_for(ui, host, Duration::from_millis(450));
    anyhow::ensure!(
        host.live_windows().iter().all(|w| w.visible && w.ordered),
        "Show Desktop hid/reordered a widget"
    );
    let note = host
        .live_windows()
        .into_iter()
        .find(|w| w.instance == "notes-1")
        .ok_or_else(|| anyhow::anyhow!("notes fixture missing"))?;
    let point = POINT {
        x: note.rect.right as i32 - 24,
        y: note.rect.bottom as i32 - 24,
    };
    // SAFETY: a point measured from the fixture's own window; no guessed user-app coordinates.
    let hit = unsafe { WindowFromPoint(point) };
    anyhow::ensure!(
        hit == note.hwnd,
        "notes alpha-1 body is not visibly hit-testable (hit {:?}, expected {:?})",
        hit,
        note.hwnd
    );
    // SAFETY: only the just-observed fixture window receives these pointer events.
    unsafe {
        SetCursorPos(point.x, point.y)?;
    }
    let mouse = |flags| INPUT {
        r#type: INPUT_MOUSE,
        Anonymous: INPUT_0 {
            mi: MOUSEINPUT {
                dwFlags: flags,
                ..Default::default()
            },
        },
    };
    let click = [mouse(MOUSEEVENTF_LEFTDOWN), mouse(MOUSEEVENTF_LEFTUP)];
    // SAFETY: exact INPUT array and ABI size, targeting the measured note body.
    anyhow::ensure!(
        // SAFETY: the INPUT buffer targets the measured fixture window.
        unsafe { SendInput(&click, size_of::<INPUT>() as i32) } == 2,
        "click injection failed"
    );
    pump_for(ui, host, Duration::from_millis(100));
    anyhow::ensure!(
        host.live_windows()
            .iter()
            .any(|w| w.instance == "notes-1" && w.editing_text && w.ordered),
        "note click did not focus a bottom-band RichEdit field: {:?}",
        host.live_windows()
    );
    let typed = "فحص 東京測試 한글입력";
    // SAFETY: read-only focus/style probes of this test's own surface before keyboard input.
    unsafe {
        anyhow::ensure!(
            GetForegroundWindow() == note.hwnd && GetFocus() == note.hwnd,
            "note field lacks real focus: foreground={:?} focus={:?} expected={:?} ex={:x}",
            GetForegroundWindow(),
            GetFocus(),
            note.hwnd,
            GetWindowLongPtrW(note.hwnd, GWL_EXSTYLE)
        );
    }
    let keys: Vec<_> = typed
        .encode_utf16()
        .flat_map(|ch| {
            [false, true].map(move |up| INPUT {
                r#type: INPUT_KEYBOARD,
                Anonymous: INPUT_0 {
                    ki: KEYBDINPUT {
                        wScan: ch,
                        dwFlags: KEYEVENTF_UNICODE
                            | if up {
                                KEYEVENTF_KEYUP
                            } else {
                                KEYBD_EVENT_FLAGS(0)
                            },
                        ..Default::default()
                    },
                },
            })
        })
        .collect();
    // SAFETY: Unicode input into the field whose focus was verified above.
    anyhow::ensure!(
        // SAFETY: the verified focused field receives this Unicode INPUT buffer.
        unsafe { SendInput(&keys, size_of::<INPUT>() as i32) } == keys.len() as u32,
        "Unicode typing failed"
    );
    pump_for(ui, host, Duration::from_millis(150));
    // SAFETY: Escape is delivered to our own note surface, then handled through its event queue.
    unsafe {
        SendMessageW(note.hwnd, WM_KEYDOWN, Some(WPARAM(27)), None);
    }
    pump_for(ui, host, Duration::from_millis(150));
    let saved = Storage::new(files::user("widget-data")?).get("notes-1", "text")?;
    anyhow::ensure!(
        saved.as_str().is_some_and(|s| s.contains(typed)),
        "the native field did not persist keyboard-entered Arabic/CJK text: {saved}"
    );
    println!("N4_CHECK alpha_hit=1 widget_click=1 unicode_keyboard=1 z_order_after_click=1");
    // SAFETY: inverse of the Show Desktop operation above.
    unsafe {
        desktop.shell.ToggleDesktop()?;
    }
    desktop.toggled = false;
    pump_for(ui, host, Duration::from_millis(450));
    anyhow::ensure!(
        host.live_windows().iter().all(|w| w.visible && w.ordered),
        "restoring apps broke the desktop band: {:?}",
        host.live_windows()
    );
    println!("N4_CHECK show_desktop_and_restore=1");
    drop(desktop);
    drop(app);
    let before = host
        .live_windows()
        .iter()
        .map(|w| w.hwnd.0 as usize)
        .collect::<Vec<_>>();
    // The user explicitly requested Explorer-restart verification. Keep user apps running and
    // start Explorer immediately after stopping its shell process, before waiting on our loop.
    let killed = std::process::Command::new("taskkill.exe")
        .args(["/f", "/im", "explorer.exe"])
        .creation_flags(0x08000000)
        .output()?;
    anyhow::ensure!(
        killed.status.success(),
        "Explorer restart could not stop the existing shell"
    );
    let _explorer = std::process::Command::new(
        std::path::PathBuf::from(
            std::env::var_os("WINDIR").unwrap_or_else(|| r"C:\Windows".into()),
        )
        .join("explorer.exe"),
    )
    .spawn()?;
    let deadline = Instant::now() + Duration::from_secs(15);
    let mut restored = false;
    while Instant::now() < deadline {
        pump_for(ui, host, Duration::from_millis(100));
        let windows = host.live_windows();
        if windows.len() == 6
            && windows
                .iter()
                .all(|w| w.visible && w.ordered && !before.contains(&(w.hwnd.0 as usize)))
        {
            restored = true;
            break;
        }
    }
    anyhow::ensure!(restored, "widgets did not recover after Explorer restart");
    println!(
        "N4_CHECK explorer_restart=1 recovered_widgets=6 dpi={:?}",
        host.live_windows()
            .iter()
            .map(|w| w.dpi)
            .collect::<Vec<_>>()
    );
    // Exercise the production edit-mode handlers with pointer messages addressed only to our
    // measured window, then read the persisted grid data rather than merely checking hit boxes.
    host.handle(
        ui,
        inbox::Msg {
            message: widgets::WM_EDIT,
            wparam: 0,
            lparam: 1,
        },
    );
    pump_for(ui, host, Duration::from_millis(100));
    let before_layout = Layout::load(&files::layout()?)?;
    let note = host
        .live_windows()
        .into_iter()
        .find(|w| w.instance == "notes-1")
        .unwrap();
    let point = |x: i32, y: i32| {
        windows::Win32::Foundation::LPARAM(((y as u16 as u32) << 16 | x as u16 as u32) as isize)
    };
    // SAFETY: synchronous input to our known note HWND with valid mouse-message coordinates.
    unsafe {
        SendMessageW(
            note.hwnd,
            WM_LBUTTONDOWN,
            Some(WPARAM(1)),
            Some(point(32, 50)),
        );
        SendMessageW(
            note.hwnd,
            WM_MOUSEMOVE,
            Some(WPARAM(1)),
            Some(point(-160, 50)),
        );
    }
    pump_for(ui, host, Duration::from_millis(80));
    // SAFETY: finish the captured gesture on the same fixture window.
    unsafe {
        SendMessageW(
            note.hwnd,
            WM_LBUTTONUP,
            Some(WPARAM(0)),
            Some(point(32, 50)),
        );
    }
    pump_for(ui, host, Duration::from_millis(80));
    let moved = Layout::load(&files::layout()?)?;
    anyhow::ensure!(
        moved.get("notes-1").unwrap().cell != before_layout.get("notes-1").unwrap().cell,
        "edit drag did not persist snapped placement"
    );
    let note = host
        .live_windows()
        .into_iter()
        .find(|w| w.instance == "notes-1")
        .unwrap();
    let x = note.rect.width() as i32 - 10;
    let y = note.rect.height() as i32 - 10;
    // SAFETY: resize handle coordinates measured from the production fixture surface.
    unsafe {
        SendMessageW(
            note.hwnd,
            WM_LBUTTONDOWN,
            Some(WPARAM(1)),
            Some(point(x, y)),
        );
        SendMessageW(
            note.hwnd,
            WM_MOUSEMOVE,
            Some(WPARAM(1)),
            Some(point(x + 100, y + 100)),
        );
    }
    pump_for(ui, host, Duration::from_millis(80));
    // SAFETY: finish this own-window gesture.
    unsafe {
        SendMessageW(note.hwnd, WM_LBUTTONUP, Some(WPARAM(0)), Some(point(x, y)));
    }
    pump_for(ui, host, Duration::from_millis(80));
    let resized = Layout::load(&files::layout()?)?;
    anyhow::ensure!(
        resized.get("notes-1").unwrap().size != moved.get("notes-1").unwrap().size,
        "edit resize did not persist"
    );
    host.handle(
        ui,
        inbox::Msg {
            message: widgets::WM_EDIT,
            wparam: 0,
            lparam: -1,
        },
    );
    before_layout.save(&files::layout()?)?;
    host.handle(
        ui,
        inbox::Msg {
            message: widgets::WM_LAYOUT,
            wparam: 0,
            lparam: 0,
        },
    );
    pump_for(ui, host, Duration::from_millis(100));
    println!("N4_CHECK edit_drag=1 edit_resize=1 persisted_grid=1");
    let mut overlay_layout = before_layout.clone();
    let overlay_id = overlay_layout.add("media-pill", &monitor.device, [1, 1], [4, 1]);
    overlay_layout.overlay = true;
    overlay_layout.save(&files::layout()?)?;
    host.handle(
        ui,
        inbox::Msg {
            message: widgets::WM_LAYOUT,
            wparam: 0,
            lparam: 0,
        },
    );
    for (editing, topmost) in [(false, true), (true, false), (false, true)] {
        host.handle(
            ui,
            inbox::Msg {
                message: widgets::WM_EDIT,
                wparam: 0,
                lparam: isize::from(editing),
            },
        );
        pump_for(ui, host, Duration::from_millis(100));
        let pill = host
            .live_windows()
            .into_iter()
            .find(|p| p.instance == overlay_id)
            .unwrap();
        // SAFETY: style query of this fixture's verified overlay/desktop surface.
        let ex = unsafe { GetWindowLongPtrW(pill.hwnd, GWL_EXSTYLE) } as u32;
        anyhow::ensure!(
            pill.visible && (ex & WS_EX_TOPMOST.0 != 0) == topmost && ex & WS_EX_NOACTIVATE.0 == 0,
            "overlay/edit layer or keyboard activation is incorrect"
        );
    }
    host.handle(
        ui,
        inbox::Msg {
            message: widgets::WM_OVERLAY,
            wparam: 0,
            lparam: 0,
        },
    );
    pump_for(ui, host, Duration::from_millis(100));
    anyhow::ensure!(
        host.live_windows()
            .iter()
            .any(|p| p.instance == overlay_id && !p.visible),
        "overlay toggle did not hide the pill"
    );
    before_layout.save(&files::layout()?)?;
    host.handle(
        ui,
        inbox::Msg {
            message: widgets::WM_LAYOUT,
            wparam: 0,
            lparam: 0,
        },
    );
    pump_for(ui, host, Duration::from_millis(100));
    println!("N4_CHECK overlay_topmost=1 overlay_desktop_while_editing=1 overlay_toggle=1");
    Ok(())
}

struct VisibleDesktop {
    shell: windows::Win32::UI::Shell::IShellDispatch4,
}
impl VisibleDesktop {
    fn new() -> Result<Self> {
        use windows::Win32::System::Com::{CLSCTX_INPROC_SERVER, CoCreateInstance};
        // SAFETY: requested idle-fixture desktop visibility, paired with UndoMinimizeALL.
        let shell: windows::Win32::UI::Shell::IShellDispatch4 = unsafe {
            CoCreateInstance(
                &windows::Win32::UI::Shell::Shell,
                None,
                CLSCTX_INPROC_SERVER,
            )
        }?;
        // SAFETY: the fixture owns the desktop lock while it temporarily reveals both monitors.
        unsafe {
            shell.MinimizeAll()?;
        }
        Ok(Self { shell })
    }
}
impl Drop for VisibleDesktop {
    fn drop(&mut self) {
        // SAFETY: restores the user app state changed by MinimizeAll at fixture startup.
        unsafe {
            let _ = self.shell.UndoMinimizeALL();
        }
    }
}

// The idle fixture requires paused media. Restore only the session that was playing at entry.
struct PausedMedia(Option<windows::Media::Control::GlobalSystemMediaTransportControlsSession>);
impl PausedMedia {
    fn new() -> Self {
        use windows::Media::Control::{
            GlobalSystemMediaTransportControlsSessionManager as Manager,
            GlobalSystemMediaTransportControlsSessionPlaybackStatus as Status,
        };
        let session = Manager::RequestAsync()
            .and_then(|op| op.join())
            .and_then(|m| m.GetCurrentSession())
            .ok();
        let paused = session.filter(|s| {
            s.GetPlaybackInfo().and_then(|i| i.PlaybackStatus()).ok() == Some(Status::Playing)
                && s.TryPauseAsync().and_then(|op| op.join()).unwrap_or(false)
        });
        println!("N4_MEDIA paused_existing={}", paused.is_some());
        Self(paused)
    }
}
impl Drop for PausedMedia {
    fn drop(&mut self) {
        if let Some(s) = &self.0 {
            let _ = s.TryPlayAsync().and_then(|op| op.join());
        }
    }
}
