//! Wallpaper files use a separate validated writer; keep its failures visible too.
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use serde_json::Value;

use super::{Command, Page};
use crate::{
    actions::Action,
    app::App,
    ui::{B, Col},
};

#[derive(Debug)]
pub struct Failure {
    reason: String,
    outcome: &'static str,
    retry: Action,
}

pub fn failed(app: &mut App, doc: Option<crate::docs::DocId>, reason: &str) {
    let doc = app.docs.failed_doc().or(doc);
    if let Some(panel) = &app.panel {
        if let Some(doc) = doc {
            panel.save_doc.set(Some(doc));
            panel.save_errors.borrow_mut().insert(
                doc,
                Failure {
                    reason: reason.into(),
                    outcome: app.docs.save_outcome(),
                    retry: Action::Panel(Command::RetryDoc(doc)),
                },
            );
        }
        app.docs.saved = crate::docs::Saved::Failed(reason.into());
    }
}

#[derive(Debug, Default)]
pub struct ThemeCache(std::collections::HashMap<PathBuf, ThemeData>);

#[derive(Debug)]
struct ThemeData {
    stamp: Option<(std::time::SystemTime, u64)>,
    value: std::rc::Rc<Value>,
    source: Option<std::rc::Rc<str>>,
}

/// Watch the cheap file stamp; parse a wallpaper manifest only when its file changes.
pub fn theme_value(app: &App, wallpaper: &crate::library::Wallpaper) -> std::rc::Rc<Value> {
    let path = wallpaper.dir.join(mypaper_theme::MANIFEST);
    let stamp = std::fs::metadata(&path)
        .ok()
        .and_then(|m| Some((m.modified().ok()?, m.len())));
    let mut cache = app.panel.as_ref().unwrap().theme_data.borrow_mut();
    if let Some(data) = cache.0.get(&path)
        && data.stamp == stamp
    {
        return data.value.clone();
    }
    let source: Option<std::rc::Rc<str>> = std::fs::read_to_string(&path).ok().map(Into::into);
    let value = std::rc::Rc::new(
        source
            .as_deref()
            .and_then(|source| serde_json::from_str(source).ok())
            .unwrap_or_else(|| serde_json::json!({})),
    );
    cache.0.insert(
        path,
        ThemeData {
            stamp,
            value: value.clone(),
            source,
        },
    );
    value
}

fn theme_source(app: &App, wallpaper: &crate::library::Wallpaper) -> Option<std::rc::Rc<str>> {
    theme_value(app, wallpaper);
    app.panel
        .as_ref()
        .unwrap()
        .theme_data
        .borrow()
        .0
        .get(&wallpaper.dir.join(mypaper_theme::MANIFEST))
        .and_then(|data| data.source.clone())
}

/// Alerts use the same readable treatment in the header and beside a failed file.
pub fn emphasize_feedback(b: &mut B, from: usize) {
    let style = super::controls::font(b, 12.0, 500);
    for op in &mut b.list.ops[from..] {
        match op {
            mypaper_ui::Op::Text {
                style: text_style,
                color,
                ..
            } if text_style.size <= style.size + 0.1 => {
                *text_style = style.clone();
                *color = b.p.ink1;
            }
            mypaper_ui::Op::Icon { color, .. } => *color = b.p.ink1,
            _ => {}
        }
    }
}

/// Make a file's unavailable controls visibly inert. The kit owns selection appearance.
pub fn finish_page(app: &App, b: &mut B, page: Page, first_op: usize, first_target: usize) {
    use mypaper_ui::Op;
    if !super::controls::relevant_docs(page)
        .iter()
        .any(|id| app.docs.doc(*id).issue.is_some())
    {
        return;
    }
    let disabled: Vec<_> = b
        .targets
        .iter()
        .enumerate()
        .skip(first_target)
        .filter(|(_, target)| {
            !matches!(
                &target.action,
                Action::Panel(
                    Command::Open(_)
                        | Command::Back
                        | Command::Display(_)
                        | Command::Wallpaper(_)
                        | Command::Connection(_)
                ) | Action::SelectWidget(_)
                    | Action::SelectModule(_)
                    | Action::SelectPanel(_)
                    | Action::More(_)
            )
        })
        .map(|(index, _)| index)
        .collect();
    for &index in &disabled {
        b.targets[index].action = Action::None;
        b.targets[index].visible = None;
    }
    for hit in &mut b.list.hits {
        if disabled.contains(&(hit.id.saturating_sub(1) as usize)) {
            hit.enabled = false;
        }
    }
    for op in &mut b.list.ops[first_op..] {
        match op {
            Op::Text { color, .. }
            | Op::Icon { color, .. }
            | Op::Fill { color, .. }
            | Op::Stroke { color, .. }
            | Op::Line { color, .. } => color.a *= 0.5,
            Op::Image { opacity, .. } | Op::RoundedImage { opacity, .. } => *opacity *= 0.5,
            _ => {}
        }
    }
}

pub fn theme_saved(app: &mut App, dir: &Path) {
    if let Some(panel) = &app.panel {
        panel.theme_errors.borrow_mut().remove(dir);
    }
}

/// Healthy parts have the same backup-first reset path as malformed files.
pub fn reset_part(app: &mut App, doc: crate::docs::DocId, path: Option<&'static str>) {
    use crate::docs::{DocId, Op, StartedOver};
    app.docs.reload(doc);
    let defaults = if let Some(path) = path {
        let mut raw = app.docs.doc(doc).raw.clone();
        crate::docs::apply_op(
            &mut raw,
            &Op {
                doc,
                path: path.into(),
                value: None,
            },
        );
        serde_json::to_string_pretty(&raw).unwrap_or_default()
    } else {
        match doc {
            DocId::Bars => "{\"version\":1}".into(),
            DocId::Widgets => format!(
                "{{\"version\":{},\"instances\":[]}}",
                mypaper_widgets::layout::VERSION
            ),
            _ => "{}".into(),
        }
    };
    let owner = app.window;
    let result = app.docs.start_over(doc, &defaults, |backup| {
        use windows::Win32::UI::WindowsAndMessaging::{
            IDOK, MB_DEFBUTTON2, MB_ICONWARNING, MB_OKCANCEL, MessageBoxW,
        };
        let kept = backup
            .and_then(|p| p.file_name())
            .map(|n| n.to_string_lossy())
            .unwrap_or_default();
        let part = match path {
            Some("/look") => "Your look",
            Some("/look/windows") => "Windows look",
            Some("/desktop_menu") => "Desktop menu",
            _ => doc.label(),
        };
        let message = windows::core::HSTRING::from(format!(
            "Start over?\n\n{part} returns to its defaults.\nThe current file is kept as {kept}."
        ));
        // SAFETY: a confirmation owned by our panel; Cancel is the default.
        (unsafe {
            MessageBoxW(
                owner,
                &message,
                windows::core::w!("Start over"),
                MB_OKCANCEL | MB_ICONWARNING | MB_DEFBUTTON2,
            )
        }) == IDOK
    });
    match result {
        Ok(StartedOver::Done(backup)) => {
            if doc == DocId::Config && path.is_none_or(|path| path.starts_with("/look")) {
                app.apply(
                    "Start over",
                    vec![Op {
                        doc,
                        path: path.unwrap_or("/look").into(),
                        value: None,
                    }],
                );
            }
            app.panel
                .as_ref()
                .unwrap()
                .save_errors
                .borrow_mut()
                .remove(&doc);
            app.retheme();
            app.library
                .set_active(crate::app::theme_name(&app.docs).as_deref());
            super::sync_connections(app);
            super::history::record_drag(app, "Start over");
            app.toast(
                format!(
                    "Started over{}",
                    backup.map_or(String::new(), |p| format!(
                        "; backup: {}",
                        p.file_name().unwrap().to_string_lossy()
                    ))
                ),
                None,
            );
        }
        Ok(StartedOver::Declined) => {}
        Err(error) => app.alert(format!("{error:#}")),
    }
}

pub fn theme_failed(app: &mut App, dir: &Path, error: &str) {
    if let Some(panel) = &app.panel {
        panel.save_doc.set(None);
        panel.theme_errors.borrow_mut().insert(
            dir.to_owned(),
            Failure {
                reason: error.into(),
                outcome: "Change did not take effect",
                retry: panel
                    .theme_retry
                    .clone()
                    .unwrap_or(Action::Panel(Command::RetryTheme)),
            },
        );
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
    if let Some(wallpaper) = selected
        && let Some(error) = &wallpaper.error
    {
        let parsed = theme_source(app, wallpaper);
        let parse_error = parsed.as_deref().and_then(|source| {
            mypaper_theme::LoadedTheme::parse(wallpaper.dir.clone(), source).err()
        });
        let newer = (error.contains("unknown field") || error.contains("unknown variant"))
            || error.contains("newer")
            || parse_error.as_ref().is_some_and(|error| {
                error.chain().any(|cause| {
                    let message = cause.to_string();
                    message.contains("unknown field") || message.contains("unknown variant")
                })
            });
        let line = parse_error
            .as_ref()
            .and_then(|error| {
                error
                    .chain()
                    .find_map(|cause| cause.downcast_ref::<serde_json::Error>())
            })
            .map_or(1, |error| error.line().max(1));
        let message = if newer {
            crate::docs::NEWER.into()
        } else if parse_error.is_some() {
            format!("Can't read theme.json, line {line}")
        } else {
            "Can't load this wallpaper. Check its source file.".into()
        };
        let mut actions = vec![(
            "Show file",
            Action::Panel(Command::ShowTheme(wallpaper.dir.clone())),
        )];
        if !newer && wallpaper.editable() && parse_error.is_some() {
            actions.push((
                "Start over",
                Action::Panel(Command::ResetTheme(wallpaper.dir.clone())),
            ));
        }
        super::controls::feedback(b, c, &message, actions);
    }
}

/// Write feedback is pinned above every page's scroll area until that file saves.
pub fn write_banners(app: &App, b: &mut B, c: &mut Col) {
    let panel = app.panel.as_ref().unwrap();
    for (doc, failure) in panel.save_errors.borrow().iter() {
        failure_banner(
            app,
            b,
            c,
            doc.filename(),
            mypaper_ui::controls::key(("panel-save-reason", doc)),
            failure,
        );
    }
    for (dir, failure) in panel.theme_errors.borrow().iter() {
        let name = dir.file_name().unwrap_or_default().to_string_lossy();
        failure_banner(
            app,
            b,
            c,
            &format!("{name}/theme.json"),
            mypaper_ui::controls::key(("panel-theme-reason", dir)),
            failure,
        );
    }
}

fn failure_banner(
    app: &App,
    b: &mut B,
    c: &mut Col,
    file: &str,
    key: mypaper_ui::controls::Key,
    failure: &Failure,
) {
    use super::controls::{group, height, notice};
    group(b, c, "", |b, c| {
        let style = super::controls::font(b, 13.0, 600);
        b.icon(
            mypaper_ui::Rect::new(c.x, c.y + b.px(2.0), b.px(16.0), height(b, 20.0)),
            mypaper_ui::icons::Icon::Warning,
            15.0,
            b.p.ink1,
        );
        c.y += b.paragraph(
            c.x + b.px(24.0),
            c.y,
            c.w - b.px(24.0),
            &format!("Couldn't save {file}"),
            &style,
            height(b, 20.0),
            b.p.ink1,
        ) + b.px(8.0);
        notice(b, c, failure.outcome);
        let width = (c.w - b.px(8.0)) / 2.0;
        let r = mypaper_ui::Rect::new(c.x, c.y, width, height(b, 30.0));
        b.button(
            mypaper_ui::controls::key((key, "retry")),
            r,
            "Retry",
            None,
            mypaper_ui::controls::ButtonKind::Secondary,
            true,
            failure.retry.clone(),
        );
        b.button(
            mypaper_ui::controls::key((key, "reason")),
            r.offset(width + b.px(8.0), 0.0),
            if app.is_open(key) {
                "Hide reason"
            } else {
                "Show reason"
            },
            None,
            mypaper_ui::controls::ButtonKind::Secondary,
            true,
            Action::More(key),
        );
        c.y = r.bottom + b.px(8.0);
        if app.is_open(key) {
            notice(b, c, &failure.reason);
        }
    });
}

pub fn reset_theme(app: &mut App, dir: &Path) {
    if app.fixture
        || !app
            .library
            .items
            .iter()
            .any(|wallpaper| wallpaper.dir == dir && wallpaper.error.is_some())
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
    if let Ok(source) = std::str::from_utf8(&bytes)
        && let Err(error) = mypaper_theme::LoadedTheme::parse(dir.clone(), source)
        && error.chain().any(|cause| {
            let message = cause.to_string();
            message.contains("unknown field") || message.contains("unknown variant")
        })
    {
        bail!("{}", crate::docs::NEWER);
    }
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
    fn a_save_failure_stays_pinned_across_pages_and_explains_its_effect() {
        let mut ui = mypaper_ui::Ui::new().unwrap();
        let mut app = crate::goldens::app(true, &ui);
        app.panel = Some(super::super::State::default());
        app.size = (360, 760);
        let edit =
            crate::actions::Edit::new(crate::docs::DocId::Config, "/engine/fps", "Frame rate");
        app.set(&edit, serde_json::json!("invalid"));
        for page in [
            Page::Root,
            Page::General,
            Page::Keyboard,
            Page::Connections,
            Page::Bars,
            Page::History,
        ] {
            app.panel.as_mut().unwrap().page = page;
            let built = super::super::build(&app, &mut ui.text);
            assert!(built.list.ops.iter().any(|op| matches!(op, mypaper_ui::Op::Text { text, .. } if text.as_ref() == "Couldn't save config.json")), "{page:?}");
            assert!(built.list.ops.iter().any(|op| matches!(op, mypaper_ui::Op::Text { text, .. } if text.as_ref() == "Change did not take effect")));
            assert!(built.targets.iter().any(|target| target.action
                == Action::Panel(Command::RetryDoc(crate::docs::DocId::Config))
                && target.rect.top < 160.0));
        }
        app.set(&edit, serde_json::json!(60));
        assert!(app.panel.as_ref().unwrap().save_errors.borrow().is_empty());
    }

    #[test]
    fn expanded_more_keeps_its_theme_contrast_instead_of_segment_text() {
        let mut ui = mypaper_ui::Ui::new().unwrap();
        for dark in [true, false] {
            let mut app = crate::goldens::app(dark, &ui);
            app.panel = Some(super::super::State::default());
            app.panel.as_mut().unwrap().page = Page::Look;
            app.size = (480, 760);
            let built = super::super::build(&app, &mut ui.text);
            let action = built
                .targets
                .iter()
                .find(|target| matches!(target.action, Action::More(_)))
                .unwrap()
                .action
                .clone();
            app.run(action);
            let built = super::super::build(&app, &mut ui.text);
            assert!(built.list.ops.iter().any(|op| matches!(
                op,
                mypaper_ui::Op::Text { text, color, .. }
                    if text.as_ref() == "More"
                        && *color == super::super::palette_for(&app).ink2
            )));
        }
    }

    #[test]
    fn short_segment_labels_have_the_same_selected_fill() {
        let mut ui = mypaper_ui::Ui::new().unwrap();
        for dark in [true, false] {
            let mut app = crate::goldens::app(dark, &ui);
            app.panel = Some(super::super::State::default());
            app.panel.as_mut().unwrap().page = Page::Widget;
            app.nav.widget = Some("system-monitor-1".into());
            app.size = (480, 2200);
            let built = super::super::build(&app, &mut ui.text);
            let selected = built
                .list
                .hits
                .iter()
                .find(|hit| hit.label.as_ref() == "Update every: 2 s")
                .unwrap();
            assert_eq!(selected.checked, Some(true));
            assert!(built.list.ops.iter().any(|op| matches!(
                op,
                mypaper_ui::Op::Fill { rect, color, .. }
                    if *rect == selected.rect && *color == super::super::palette_for(&app).selected
            )));
        }
    }

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
