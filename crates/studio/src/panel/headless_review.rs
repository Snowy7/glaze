//! Opt-in, isolated memory review. No app startup, live settings, IPC, input or activation.
//!
//! Run this test alone in release mode with `GLAZE_STUDIO_MEMORY_REPORT` pointing to JSON.
//! The app window is created hidden and is never shown; fixtures keep every edit in memory.

use super::{Command, Page, measure};
use crate::actions::{Action, Bar, Slot};
use glaze_ui::{Kind, Role, Surface, Ui};
use windows::Win32::UI::WindowsAndMessaging::IsWindowVisible;

#[test]
#[ignore = "run alone: release-mode hidden-window memory measurement"]
fn hidden_window_memory_tour() {
    let report = std::env::var_os("GLAZE_STUDIO_MEMORY_REPORT")
        .expect("set GLAZE_STUDIO_MEMORY_REPORT to the output JSON path");
    let mut ui = Ui::new().unwrap();
    ui.text.set_capacity(64);
    let mut surface = Surface::new_deferred_accessibility(
        Kind::App { min: (960, 640) },
        "Glaze Studio (hidden memory fixture)",
        Role::Window,
    )
    .unwrap();
    surface.set_no_activate(true);
    let hidden = || {
        // SAFETY: a read-only visibility query on the window owned by this test.
        !unsafe { IsWindowVisible(surface.hwnd()) }.as_bool()
    };
    assert!(hidden());
    let mut app = crate::goldens::window_app(true, &ui, (1280, 800));
    app.images.borrow_mut().set_budget(2 << 20);
    let untouched = crate::docs::DocId::ALL.map(|id| app.docs.doc(id).raw.clone());
    let baseline = measure::memory(false).unwrap();
    let mut pages = Vec::new();
    for page in [
        Page::Root,
        Page::Wallpapers,
        Page::Wallpaper,
        Page::AddWallpaper,
        Page::Look,
        Page::Windows,
        Page::Widgets,
        Page::Widget,
        Page::AddWidget,
        Page::Bars,
        Page::BarDesign,
        Page::Top,
        Page::Dock,
        Page::Module,
        Page::AddModule,
        Page::Panels,
        Page::BarPanel,
        Page::DesktopMenu,
        Page::Connections,
        Page::Connection,
        Page::Settings,
        Page::General,
        Page::Performance,
        Page::Keyboard,
        Page::Troubleshoot,
        Page::About,
        Page::History,
        Page::Search,
        Page::FirstRun,
    ] {
        app.run(Action::Panel(Command::Open(page)));
        app.nav.widget = Some("weather-1".into());
        if page == Page::Module {
            app.nav.bar = Some(Bar::Top);
            app.nav.module = Some(Slot {
                bar: Bar::Top,
                list: "right",
                index: 6,
            });
        }
        if page == Page::AddModule {
            app.nav.module = None;
        }
        // Fixtures disable live page reclamation to stabilize other image tests. Apply the
        // same cache retirement here while retaining fixture guards against IPC and writes.
        surface.release();
        measure::release_page_resources(&app, &mut ui.text);
        let built = crate::view::build(&app, &mut ui);
        surface.set_chrome(&built.chrome);
        surface.set_accessibility_semantics(built.semantics);
        surface
            .present(&mut ui, (0, 0), app.size, built.list)
            .unwrap();
        super::after_present(&app);
        measure::optimize_heap();
        // SAFETY: no messages are pumped and no show/activate operation is ever called.
        assert!(!unsafe { IsWindowVisible(surface.hwnd()) }.as_bool());
        let memory = measure::memory(false).unwrap();
        pages.push(serde_json::json!({
            "page": format!("{page:?}"),
            "working_set_mib": memory.working_mb,
            "private_mib": memory.private_mb,
        }));
    }
    let after_tour = measure::memory(false).unwrap();
    use windows::Win32::System::{
        ProcessStatus::{GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS_EX},
        Threading::GetCurrentProcess,
    };
    let mut counters = PROCESS_MEMORY_COUNTERS_EX {
        cb: size_of::<PROCESS_MEMORY_COUNTERS_EX>() as u32,
        ..Default::default()
    };
    // SAFETY: the current-process handle and a local, correctly sized writable buffer.
    unsafe { GetProcessMemoryInfo(GetCurrentProcess(), (&raw mut counters).cast(), counters.cb) }
        .unwrap();
    let max_page = pages
        .iter()
        .filter_map(|p| p["working_set_mib"].as_f64())
        .fold(0.0_f64, f64::max);
    assert_eq!(
        untouched,
        crate::docs::DocId::ALL.map(|id| app.docs.doc(id).raw.clone())
    );
    drop(surface);
    std::fs::write(
        report,
        serde_json::to_vec_pretty(&serde_json::json!({
            "mode": "release test executable, isolated fixtures, hidden native app surface",
            "window_size": [1280, 800],
            "window_ever_shown": false,
            "input_or_activation_used": false,
            "baseline_working_set_mib": baseline.working_mb,
            "after_tour_working_set_mib": after_tour.working_mb,
            "after_tour_private_mib": after_tour.private_mb,
            "max_page_working_set_mib": max_page,
            "process_peak_working_set_mib": counters.PeakWorkingSetSize as f64 / 1048576.0,
            "target_mib": 60,
            "within_target": counters.PeakWorkingSetSize <= 60 << 20,
            "pages": pages,
        }))
        .unwrap(),
    )
    .unwrap();
}
