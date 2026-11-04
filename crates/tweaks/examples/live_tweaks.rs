//! Applies a cursor scheme and the classic-menu key to the REAL registry for a few seconds and
//! verifies that turning them off puts every value back exactly. Run it with a scratch
//! `LOCALAPPDATA` (the undo journals go there) and `APPDATA`:
//!
//! ```text
//! cargo run -p mypaper-tweaks --example live_tweaks -- [mypaper-dark|mypaper-light]
//! ```
//!
//! It never restarts Explorer. A crash between the steps leaves the journals in the scratch
//! folder, so run the example again with the same folders to undo.

use anyhow::{Result, bail};
use mypaper_theme::WindowsStyle;
use mypaper_tweaks::Tweaks;
use mypaper_tweaks::classic_menu::{self, ClassicMenu};
use mypaper_tweaks::cursors::{CURSORS_KEY, ROLES};
use mypaper_tweaks::registry::Registry;
use mypaper_tweaks::undo::{Store, Value};
use windows::Win32::Graphics::Gdi::{
    BI_RGB, BITMAPINFO, BITMAPINFOHEADER, CreateCompatibleDC, CreateDIBSection, DIB_RGB_COLORS,
    DeleteDC, DeleteObject, SelectObject,
};
use windows::Win32::UI::WindowsAndMessaging::{
    DI_NORMAL, DrawIconEx, HICON, IDC_ARROW, LoadCursorW,
};

/// Every value this example may touch, with its current content (`None`: absent).
fn snapshot(store: &Registry) -> Result<Vec<(String, Option<Value>)>> {
    let mut names: Vec<&str> = ROLES.iter().map(|r| r.value).collect();
    names.extend([
        "",
        "Scheme Source",
        "CursorBaseSize",
        "ContactVisualization",
    ]);
    names
        .into_iter()
        .map(|name| Ok((name.to_owned(), store.read(CURSORS_KEY, name)?)))
        .collect()
}

fn show(label: &str, values: &[(String, Option<Value>)]) {
    println!("{label}:");
    for (name, value) in values {
        let name = if name.is_empty() { "(default)" } else { name };
        match value {
            Some(Value::String(s)) => println!("  {name:<22} REG_SZ        {s}"),
            Some(Value::ExpandString(s)) => println!("  {name:<22} REG_EXPAND_SZ {s}"),
            Some(Value::Dword(d)) => println!("  {name:<22} REG_DWORD     {d:#x}"),
            Some(Value::Binary(b)) => println!("  {name:<22} REG_BINARY    {} bytes", b.len()),
            None => println!("  {name:<22} (absent)"),
        }
    }
}

/// The color of the system arrow cursor, 12 pixels down and 6 across (inside the pointer).
fn arrow_pixel() -> Result<[u8; 4]> {
    // SAFETY: GDI objects created and destroyed here; the DIB memory is only read after drawing.
    unsafe {
        let cursor = LoadCursorW(None, IDC_ARROW)?;
        let dc = CreateCompatibleDC(None);
        let info = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: 32,
                biHeight: -32,
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut bits = std::ptr::null_mut();
        let dib = CreateDIBSection(Some(dc), &info, DIB_RGB_COLORS, &mut bits, None, 0)?;
        let old = SelectObject(dc, dib.into());
        std::ptr::write_bytes(bits.cast::<u8>(), 0x80, 32 * 32 * 4); // mid gray background
        DrawIconEx(dc, 0, 0, HICON(cursor.0), 32, 32, 0, None, DI_NORMAL)?;
        let at = (12 * 32 + 6) * 4;
        let pixel = std::slice::from_raw_parts(bits.cast::<u8>().add(at), 4).try_into()?;
        SelectObject(dc, old);
        let _ = DeleteObject(dib.into());
        let _ = DeleteDC(dc);
        Ok(pixel)
    }
}

fn main() -> Result<()> {
    let set = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "mypaper-dark".into());
    let registry = Registry;

    // ---- cursors -------------------------------------------------------------------------
    let before = snapshot(&registry)?;
    show("Cursors before", &before);
    println!("system arrow pixel before (BGRA): {:?}", arrow_pixel()?);

    let style: WindowsStyle = serde_json::from_str(&format!(r#"{{ "cursors": "{set}" }}"#))?;
    let mut tweaks = Tweaks::new()?;
    tweaks.activate(&style, None, std::path::Path::new("."));
    std::thread::sleep(std::time::Duration::from_millis(400));
    let during = snapshot(&registry)?;
    show("Cursors while active", &during);
    println!(
        "system arrow pixel while active (BGRA): {:?}",
        arrow_pixel()?
    );
    if during == before {
        bail!("the scheme was not applied");
    }

    tweaks.deactivate();
    std::thread::sleep(std::time::Duration::from_millis(400));
    let after = snapshot(&registry)?;
    show("Cursors after", &after);
    println!("system arrow pixel after (BGRA): {:?}", arrow_pixel()?);
    println!("cursors restored exactly: {}", after == before);
    if after != before {
        bail!("cursor values differ after the restore");
    }

    // ---- classic context menu key -------------------------------------------------------------
    let key = classic_menu::KEY;
    let parent = key.rsplit_once('\\').map_or(key, |(parent, _)| parent);
    let state = |label: &str| -> Result<()> {
        let value = registry.read(key, "")?;
        let key_exists = registry.first_missing(key)?.is_none();
        let parent_exists = registry.first_missing(parent)?.is_none();
        println!(
            "classic menu {label}: InprocServer32 exists={key_exists}, CLSID key exists={parent_exists}, default value={value:?}"
        );
        Ok(())
    };
    state("before")?;
    let was_there = registry.first_missing(key)?.is_none();
    let mut menu = ClassicMenu::new()?;
    println!("set(true) -> {:?}", menu.set(true)?);
    state("while on")?;
    println!("set(false) -> {:?}", menu.set(false)?);
    state("after")?;
    println!(
        "classic menu key restored: {}",
        registry.first_missing(key)?.is_none() == was_there
    );
    Ok(())
}
