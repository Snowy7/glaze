//! A live harness for the desktop right-click menu: starts the real menu (hook thread, menu
//! thread), then drives it with injected input and reports what it measured.
//!
//! ```text
//! cargo run --release -p mypaper-desktop --example menu_live -- [--x X --y Y] [--hold MS] [--moves N] [--cycles N]
//! ```
//!
//! - opens the menu with a right-click at (X, Y) (a point on the bare desktop; the default is the
//!   centre of the left-most monitor) and reports how long it took to appear;
//! - keeps it open for `--hold` ms (take a screenshot meanwhile), then closes it with Escape and
//!   reports whether that worked (i.e. the menu really took the focus);
//! - injects `--moves` mouse moves and `--cycles` more right-click cycles and prints what the
//!   hook callback cost per event.
//!
//! It moves the real cursor and restores it afterwards. Run it only with the desktop in front.

use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use mypaper_config::DesktopMenu as MenuConfig;
use mypaper_desktop::desktop_menu::{DesktopMenu, Look, Settings, hook_stats, measure_hook};
use windows::Win32::Foundation::POINT;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    INPUT, INPUT_0, INPUT_KEYBOARD, INPUT_MOUSE, KEYBDINPUT, KEYEVENTF_KEYUP, MOUSEEVENTF_MOVE,
    MOUSEEVENTF_RIGHTDOWN, MOUSEEVENTF_RIGHTUP, MOUSEINPUT, SendInput, VK_ESCAPE,
};
use windows::Win32::UI::WindowsAndMessaging::{
    FindWindowW, GetCursorPos, IsWindowVisible, SetCursorPos,
};
use windows::core::{PCWSTR, w};

const SHOT_SCRIPT: &str = r"C:\Users\you\AppData\Local\Temp\mypaper-tools\shot.ps1";

struct Args {
    x: Option<i32>,
    y: Option<i32>,
    hold: u64,
    moves: u32,
    cycles: u32,
    /// Where to save a screenshot of the secondary monitor while the menu is held open.
    shot: Option<String>,
    /// Hover the first item before the screenshot, to show its submenu.
    hover: bool,
}

fn args() -> Result<Args> {
    let mut args = Args {
        x: None,
        y: None,
        hold: 0,
        moves: 3000,
        cycles: 10,
        shot: None,
        hover: false,
    };
    let mut it = std::env::args().skip(1);
    while let Some(flag) = it.next() {
        let mut value = || it.next().context("a flag needs a value");
        match flag.as_str() {
            "--x" => args.x = Some(value()?.parse()?),
            "--y" => args.y = Some(value()?.parse()?),
            "--hold" => args.hold = value()?.parse()?,
            "--moves" => args.moves = value()?.parse()?,
            "--cycles" => args.cycles = value()?.parse()?,
            "--shot" => args.shot = Some(value()?),
            "--hover" => args.hover = true,
            other => bail!("unknown flag {other}"),
        }
    }
    Ok(args)
}

fn mouse(
    flags: windows::Win32::UI::Input::KeyboardAndMouse::MOUSE_EVENT_FLAGS,
    dx: i32,
    dy: i32,
) -> INPUT {
    INPUT {
        r#type: INPUT_MOUSE,
        Anonymous: INPUT_0 {
            mi: MOUSEINPUT {
                dx,
                dy,
                dwFlags: flags,
                ..Default::default()
            },
        },
    }
}

fn send(inputs: &[INPUT]) {
    // SAFETY: fully initialized inputs.
    unsafe { SendInput(inputs, size_of::<INPUT>() as i32) };
}

fn menu_window_visible() -> bool {
    // SAFETY: plain lookups.
    unsafe {
        FindWindowW(w!("mypaper.desktopmenu"), PCWSTR::null())
            .is_ok_and(|hwnd| IsWindowVisible(hwnd).as_bool())
    }
}

fn wait_for(visible: bool, limit: Duration) -> Option<Duration> {
    let start = Instant::now();
    while start.elapsed() < limit {
        if menu_window_visible() == visible {
            return Some(start.elapsed());
        }
        std::thread::sleep(Duration::from_micros(500));
    }
    None
}

fn escape() {
    let key = |flags| INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: VK_ESCAPE,
                dwFlags: flags,
                ..Default::default()
            },
        },
    };
    send(&[key(Default::default()), key(KEYEVENTF_KEYUP)]);
}

/// A right-click like a person's: press, a short hold, release. Returns how long after the
/// release the menu was visible.
fn right_click(at: (i32, i32)) -> Option<Duration> {
    // SAFETY: plain cursor move.
    let _ = unsafe { SetCursorPos(at.0, at.1) };
    std::thread::sleep(Duration::from_millis(40));
    send(&[mouse(MOUSEEVENTF_RIGHTDOWN, 0, 0)]);
    std::thread::sleep(Duration::from_millis(70));
    send(&[mouse(MOUSEEVENTF_RIGHTUP, 0, 0)]);
    wait_for(true, Duration::from_millis(1500))
}

fn main() -> Result<()> {
    mypaper_desktop::init_logging();
    mypaper_desktop::enable_dpi_awareness();
    let args = args()?;

    let monitors = mypaper_desktop::monitors();
    let leftmost = monitors.iter().min_by_key(|m| m.x).context("no monitors")?;
    let at = (
        args.x.unwrap_or(leftmost.x + leftmost.width / 2),
        args.y.unwrap_or(leftmost.y + leftmost.height / 2),
    );
    let mut original = POINT::default();
    // SAFETY: the out pointer is a valid local.
    unsafe { GetCursorPos(&mut original)? };

    measure_hook(true);
    let menu = DesktopMenu::start(
        Settings {
            menu: MenuConfig {
                enabled: true,
                ..Default::default()
            },
            look: Look::default(),
        },
        |action| println!("app action: {action:?}"),
    )?;
    std::thread::sleep(Duration::from_millis(1500)); // let the first icon read and UI Automation warm up

    let result = (|| -> Result<()> {
        println!("right-click at {at:?}");
        let first = right_click(at).context("the menu did not appear")?;
        println!("menu visible {first:.1?} after the release");
        if args.hold > 0 {
            println!("holding for {} ms", args.hold);
            std::thread::sleep(Duration::from_millis(args.hold / 2));
            if args.hover {
                // Onto the first row ("View") so its submenu opens.
                // SAFETY: plain cursor move.
                let _ = unsafe { SetCursorPos(at.0 + 70, at.1 + 22) };
                std::thread::sleep(Duration::from_millis(400));
            }
            if let Some(path) = &args.shot {
                // The helper captures the secondary monitor without activating anything.
                let status = std::process::Command::new("powershell")
                    .args(["-NoProfile", "-File", SHOT_SCRIPT, "-Out", path])
                    .status();
                println!("screenshot {path}: {status:?}");
            }
            std::thread::sleep(Duration::from_millis(args.hold / 2));
        }
        escape();
        match wait_for(false, Duration::from_millis(800)) {
            Some(took) => {
                println!("Escape closed it in {took:.1?} (the menu has the keyboard focus)")
            }
            None => println!("Escape did NOT close it (no focus); closing with a click elsewhere"),
        }
        if menu_window_visible() {
            send(&[
                mouse(MOUSEEVENTF_RIGHTDOWN, 0, 0),
                mouse(MOUSEEVENTF_RIGHTUP, 0, 0),
            ]);
            wait_for(false, Duration::from_millis(500));
        }

        // The hook, under load: many moves, then many menu cycles.
        measure_hook(true);
        for i in 0..args.moves {
            let step = if i % 2 == 0 { 1 } else { -1 };
            send(&[mouse(MOUSEEVENTF_MOVE, step, 0)]);
            std::thread::sleep(Duration::from_micros(300));
        }
        let mut shown = Vec::new();
        for _ in 0..args.cycles {
            if let Some(took) = right_click(at) {
                shown.push(took);
            }
            escape();
            wait_for(false, Duration::from_millis(500));
            std::thread::sleep(Duration::from_millis(150));
        }
        let stats = hook_stats();
        println!(
            "hook callback cost, our own work (and, for context, the other programs' hooks after ours):"
        );
        for (name, cost) in [
            ("mouse move", stats.moves),
            ("other buttons / wheel", stats.other),
            ("right button (decision)", stats.right_button),
        ] {
            println!(
                "  {name:<24} {:>6} events  own {:>7.2} us mean {:>8.2} us worst   | chain {:>7.2} us mean {:>8.2} us worst",
                cost.events, cost.mean_us, cost.max_us, cost.chain_mean_us, cost.chain_max_us
            );
        }
        if !shown.is_empty() {
            shown.sort();
            println!(
                "menu appearance after release over {} runs: median {:.1?}, worst {:.1?}",
                shown.len(),
                shown[shown.len() / 2],
                shown[shown.len() - 1]
            );
        }
        Ok(())
    })();

    drop(menu);
    // SAFETY: plain cursor move back to where the user had it.
    let _ = unsafe { SetCursorPos(original.x, original.y) };
    result
}
