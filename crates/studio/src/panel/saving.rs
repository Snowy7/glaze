//! Wallpaper files use a separate validated writer; keep its failures visible too.
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use mypaper_ui::Rect;
use serde_json::Value;

use super::{
    Command, Page,
    controls::button,
};
use crate::{
    actions::Action,
    app::App,
    ui::{B, Col},
};

pub fn theme_saved(app: &mut App, dir: &Path) {
    if let Some(panel) = &app.panel {
        panel.theme_errors.borrow_mut().remove(dir);
    }
}

pub fn theme_failed(app: &mut App, dir: &Path, error: &str) {
    if let Some(panel) = &app.panel {
        panel.save_doc.set(None);
        panel
            .theme_errors
            .borrow_mut()
            .insert(dir.to_owned(), error.into());
        app.docs.saved = crate::docs::Saved::Failed(error.into());
    }
}

pub fn banner(app: &App, b: &mut B, c: &mut Col, page: Page) {
    let selected = match page {
        Page::Look if app.nav.look_theme => app.library.active(),
        Page::Wallpaper => app.nav.wallpaper.or_else(|| app.library.active()),
        _ => None,
    }
    .and_then(|i| app.library.items.get(i));
    for (dir, _error) in app.panel.as_ref().unwrap().theme_errors.borrow().iter() {
        if !selected.is_some_and(|wallpaper| wallpaper.dir == *dir) { continue; }
        let r = Rect::new(c.x, c.y, c.w, b.px(36.0));
        b.icon(Rect::new(r.left, r.top + b.px(10.0), b.px(16.0), b.px(16.0)), mypaper_ui::icons::Icon::Warning, 14.0, b.p.ink2);
        super::controls::text(b, Rect::new(r.left + b.px(22.0), r.top, c.w - b.px(190.0), r.height()), "Couldn't save", 11.0, 400, b.p.ink2, mypaper_ui::controls::Align::Start);
        for (index, label, command) in [(0, "Retry", Command::RetryTheme), (1, "Show folder", Command::ShowThemeFolder(dir.clone()))] {
            button(b, Rect::new(r.right - b.px(160.0) + b.px(index as f32 * 80.0), r.top + b.px(4.0), b.px(76.0), b.px(28.0)), label, None, false, true, Action::Panel(command));
        }
        c.y = r.bottom;
    }
}

pub fn reset_theme(app: &mut App, dir: &Path) {
    if app.fixture
        || !app
            .panel
            .as_ref()
            .unwrap()
            .theme_errors
            .borrow()
            .contains_key(dir)
    {
        return;
    }
    use windows::Win32::UI::WindowsAndMessaging::{
        IDOK, MB_DEFBUTTON2, MB_ICONWARNING, MB_OKCANCEL, MessageBoxW,
    };
    let message = windows::core::HSTRING::from(
        "Start over?\n\nThis wallpaper's look and fit go back to defaults. Its source and playback settings stay. The current theme.json is backed up first.",
    );
    // SAFETY: modal dialog owned by this panel, with Cancel selected by default.
    if unsafe {
        MessageBoxW(
            app.window,
            &message,
            windows::core::w!("Start over"),
            MB_OKCANCEL | MB_ICONWARNING | MB_DEFBUTTON2,
        )
    } != IDOK
    {
        return;
    }
    let result = mypaper_config::user_themes_dir().and_then(|root| reset_file(dir, &root));
    match result {
        Ok(backup) => {
            theme_saved(app, dir);
            app.library =
                crate::library::Library::scan(crate::app::theme_name(&app.docs).as_deref());
            app.docs.saved = crate::docs::Saved::Done(std::time::Instant::now());
            app.toast(
                format!(
                    "Started over; the previous file is {}",
                    backup.file_name().unwrap().to_string_lossy()
                ),
                None,
            );
        }
        Err(error) => theme_failed(app, dir, &format!("{error:#}")),
    }
}

/// Only immediate user-theme children can be reset. Validate before backing up or writing.
fn reset_file(dir: &Path, user_root: &Path) -> Result<PathBuf> {
    let dir = dir.canonicalize().context("finding the wallpaper folder")?;
    let user_root = user_root
        .canonicalize()
        .context("finding your wallpapers folder")?;
    if dir.parent() != Some(user_root.as_path()) {
        bail!("A built-in wallpaper cannot be reset; make a copy first");
    }
    let file = dir.join(mypaper_theme::MANIFEST);
    let bytes = std::fs::read(&file).context("reading theme.json")?;
    let mut root: Value =
        serde_json::from_slice(&bytes).context("reading the wallpaper settings")?;
    let object = root
        .as_object_mut()
        .context("theme.json must contain an object")?;
    for key in ["widgets", "bars", "windows"] {
        object.remove(key);
    }
    if let Some(wallpaper) = root.get_mut("wallpaper").and_then(Value::as_object_mut) {
        wallpaper.remove("fit");
    }
    let text = serde_json::to_string_pretty(&root)? + "\n";
    mypaper_theme::LoadedTheme::parse(dir.clone(), &text)
        .context("checking the reset wallpaper")?;
    let backup = (1..)
        .map(|n| dir.join(format!("theme.json.{n}.bak")))
        .find(|p| !p.exists())
        .unwrap();
    use std::io::Write;
    let mut kept = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&backup)
        .context("keeping the previous theme.json")?;
    kept.write_all(&bytes)?;
    kept.sync_all()?;
    crate::docs::write_atomic(&file, text.as_bytes())?;
    Ok(backup)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn theme_reset_preserves_source_and_playback_keeps_a_backup_and_refuses_other_folders() {
        let root = std::env::temp_dir().join(format!(
            "mypaper-panel-reset-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let dir = root.join("own");
        std::fs::create_dir(&root).unwrap();
        std::fs::create_dir(&dir).unwrap();
        let source = r#"{"name":"Own","wallpaper":{"kind":"video","source":"clip.mp4","fit":"contain","speed":1.5},"widgets":{"glass":{"blur":12}}}"#;
        std::fs::write(dir.join("theme.json"), source).unwrap();
        let backup = reset_file(&dir, &root).unwrap();
        assert_eq!(std::fs::read_to_string(backup).unwrap(), source);
        let theme: Value =
            serde_json::from_str(&std::fs::read_to_string(dir.join("theme.json")).unwrap())
                .unwrap();
        assert_eq!(
            theme.pointer("/wallpaper/source"),
            Some(&Value::String("clip.mp4".into()))
        );
        assert_eq!(
            theme.pointer("/wallpaper/speed"),
            Some(&serde_json::json!(1.5))
        );
        assert!(theme.get("widgets").is_none());
        assert!(theme.pointer("/wallpaper/fit").is_none());
        assert!(reset_file(&dir, &dir).is_err());
        // The uniquely named test directory contains only files created above.
        assert_eq!(
            root.canonicalize().unwrap().parent(),
            Some(std::env::temp_dir().canonicalize().unwrap().as_path())
        );
        std::fs::remove_dir_all(root).unwrap();
    }
}
