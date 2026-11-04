//! A live harness for the desktop right-click menu: drives a running `mypaper-shell` (started by
//! core, or by hand) with injected input from this separate process, as hardware input would,
//! and reports what it measured.
//!
//! ```text
//! cargo run --release -p mypaper-shell --example menu_live -- [--x X --y Y] [--cycles N] [--hold MS] [--hover] [--shot PATH]
//! ```
//!
//! - opens the menu with right-clicks at (X, Y) (a point on the bare desktop; the default is the
//!   centre of the left-most monitor), `--cycles` times, two ways: a quick click (press and
//!   release together: UI Automation's check and the desktop's state are on the critical path)
//!   and a person's click (a 70 ms hold); reports how long the menu took to appear, from the
//!   press and from the release, and closes it with Escape (which proves it took the focus);
//! - with `--hold`, keeps one menu open that long (with `--hover`, over "View" so its submenu
//!   opens) and takes a screenshot with `--shot`.
//!
//! It moves the real cursor and restores it afterwards. Run it only with the desktop in front,
//! under the desktop lock, with no other mypaper menu running.

use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use windows::Win32::Foundation::{HWND, POINT};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    INPUT, INPUT_0, INPUT_KEYBOARD, INPUT_MOUSE, KEYBDINPUT, KEYEVENTF_KEYUP, MOUSEEVENTF_RIGHTDOWN,
    MOUSEEVENTF_RIGHTUP, MOUSEINPUT, SendInput, VK_ESCAPE,
};
use windows::Win32::UI::WindowsAndMessaging::{
    FindWindowExW, GetCursorPos, GetWindowRect, IsWindowVisible, SetCursorPos,
};
use windows::core::w;

const SHOT_SCRIPT: &str = r"C:\Users\you\AppData\Local\Temp\mypaper-tools\shot.ps1";

struct Args {
    x: Option<i32>,
    y: Option<i32>,
    cycles: u32,
    hold: u64,
    hover: bool,
    shot: Option<String>,
}

fn args() -> Result<Args> {
    let mut args = Args {
        x: None,
        y: None,
        cycles: 15,
        hold: 0,
        hover: false,
        shot: None,
    };
    let mut it = std::env::args().skip(1);
    while let Some(flag) = it.next() {
        let mut value = || it.next().context("a flag needs a value");
        match flag.as_str() {
            "--x" => args.x = Some(value()?.parse()?),
            "--y" => args.y = Some(value()?.parse()?),
            "--cycles" => args.cycles = value()?.parse()?,
            "--hold" => args.hold = value()?.parse()?,
            "--hover" => args.hover = true,
            "--shot" => args.shot = Some(value()?),
            other => bail!("unknown flag {other}"),
        }
    }
    Ok(args)
}

fn mouse(flags: windows::Win32::UI::Input::KeyboardAndMouse::MOUSE_EVENT_FLAGS) -> INPUT {
    INPUT {
        r#type: INPUT_MOUSE,
        Anonymous: INPUT_0 {
            mi: MOUSEINPUT {
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

/// The menu's visible panels (`mypaper-ui` surfaces named "mypaper desktop menu"), with their
/// window rectangles.
fn visible_panels() -> Vec<(HWND, (i32, i32, i32, i32))> {
    let mut panels = Vec::new();
    let mut after = None;
    // SAFETY: plain window lookups.
    unsafe {
        while let Ok(hwnd) = FindWindowExW(
            None,
            after,
            w!("mypaper.ui"),
            w!("mypaper desktop menu"),
        ) {
            if IsWindowVisible(hwnd).as_bool() {
                let mut r = Default::default();
                let _ = GetWindowRect(hwnd, &mut r);
                panels.push((hwnd, (r.left, r.top, r.right, r.bottom)));
            }
            after = Some(hwnd);
        }
    }
    panels
}

fn wait_for(panels: usize, limit: Duration) -> Option<Instant> {
    let start = Instant::now();
    while start.elapsed() < limit {
        if visible_panels().len() == panels {
            return Some(Instant::now());
        }
        std::thread::sleep(Duration::from_micros(250));
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

/// A right-click with `hold` between press and release. Returns how long after the press and
/// after the release the menu was visible.
fn right_click(at: (i32, i32), hold: Duration) -> Option<(Duration, Duration)> {
    // SAFETY: plain cursor move.
    let _ = unsafe { SetCursorPos(at.0, at.1) };
    std::thread::sleep(Duration::from_millis(40));
    let pressed = Instant::now();
    send(&[mouse(MOUSEEVENTF_RIGHTDOWN)]);
    if !hold.is_zero() {
        std::thread::sleep(hold);
    }
    let released = Instant::now();
    send(&[mouse(MOUSEEVENTF_RIGHTUP)]);
    let shown = wait_for(1, Duration::from_millis(1500))?;
    Some((shown - pressed, shown - released))
}

/// Closes the menu with Escape; says whether that worked (the menu had the keyboard focus).
fn close() -> bool {
    escape();
    let closed = wait_for(0, Duration::from_millis(800)).is_some();
    if !closed {
        send(&[mouse(MOUSEEVENTF_RIGHTDOWN), mouse(MOUSEEVENTF_RIGHTUP)]);
        wait_for(0, Duration::from_millis(500));
    }
    closed
}

fn report(name: &str, mut samples: Vec<Duration>) {
    if samples.is_empty() {
        println!("{name}: no samples");
        return;
    }
    samples.sort();
    let ms = |d: Duration| d.as_secs_f64() * 1000.0;
    println!(
        "{name}: median {:.1} ms, best {:.1} ms, worst {:.1} ms ({} runs)",
        ms(samples[samples.len() / 2]),
        ms(samples[0]),
        ms(samples[samples.len() - 1]),
        samples.len()
    );
}

fn main() -> Result<()> {
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

    let result = (|| -> Result<()> {
        println!("right-clicks at {at:?}");
        let (mut quick, mut person, mut person_release) = (Vec::new(), Vec::new(), Vec::new());
        let mut focused = 0;
        for _ in 0..args.cycles {
            if let Some((from_press, _)) = right_click(at, Duration::ZERO) {
                quick.push(from_press);
            }
            focused += usize::from(close());
            std::thread::sleep(Duration::from_millis(150));
            if let Some((from_press, from_release)) = right_click(at, Duration::from_millis(70)) {
                person.push(from_press);
                person_release.push(from_release);
            }
            focused += usize::from(close());
            std::thread::sleep(Duration::from_millis(150));
        }
        report("quick click, press to menu", quick);
        report("person's click (70 ms hold), press to menu", person);
        report("person's click, release to menu", person_release);
        println!(
            "Escape closed the menu {focused} of {} times (it had the focus)",
            args.cycles * 2
        );

        if args.hold > 0 {
            right_click(at, Duration::from_millis(70)).context("the menu did not appear")?;
            if args.hover {
                // Onto the first row ("View") so its submenu opens.
                let root = visible_panels()[0].1;
                // SAFETY: plain cursor move.
                let _ = unsafe { SetCursorPos(root.0 + 80, root.1 + 14 + 6 + 16) };
                if wait_for(2, Duration::from_millis(800)).is_some() {
                    let panels = visible_panels();
                    println!("submenu open; panels: {:?}", panels.iter().map(|p| p.1).collect::<Vec<_>>());
                }
            }
            std::thread::sleep(Duration::from_millis(args.hold / 2));
            if let Some(path) = &args.shot {
                let status = std::process::Command::new("powershell")
                    .args(["-NoProfile", "-File", SHOT_SCRIPT, "-Out", path])
                    .status();
                println!("screenshot {path}: {status:?}");
            }
            std::thread::sleep(Duration::from_millis(args.hold / 2));
            close();
        }
        Ok(())
    })();

    // SAFETY: plain cursor move back to where the user had it.
    let _ = unsafe { SetCursorPos(original.x, original.y) };
    result
}
