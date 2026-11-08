//! Picking files (docs/widgets.md, "Picking files"): the system file dialog, the copy into the
//! instance's data folder, and the URLs that name those copies.
//!
//! A picked file lives at `%APPDATA%\glaze\widget-data\<instance>\files\<name>` and is served
//! to its widget (and only that widget's origin) as
//! `https://<widget>.widget.glaze/__data/<instance>/<name>`. Everything that turns a name into
//! a path is pure and tested here; the dialog runs on a worker thread of its own.

use std::fs::{File, OpenOptions};
use std::io::{self, Read};
use std::path::{Path, PathBuf};

use serde::Serialize;
use windows::Win32::Foundation::{ERROR_CANCELLED, HWND};
use windows::Win32::System::Com::{
    CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, COINIT_DISABLE_OLE1DDE, CoCreateInstance,
    CoInitializeEx, CoTaskMemFree, CoUninitialize,
};
use windows::Win32::UI::Shell::Common::COMDLG_FILTERSPEC;
use windows::Win32::UI::Shell::{
    FOS_FILEMUSTEXIST, FOS_FORCEFILESYSTEM, FOS_PATHMUSTEXIST, FileOpenDialog, IFileOpenDialog,
    SIGDN_FILESYSPATH,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DestroyWindow, SW_SHOWNOACTIVATE, SetForegroundWindow, ShowWindow,
    WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_POPUP,
};
use windows::core::{HRESULT, HSTRING, PCWSTR, w};

use crate::bridge::{ErrorCode, Target, WidgetError};
use crate::manifest::valid_id;
use crate::net::WIDGET_SUFFIX;
use crate::wake::{Msg, Waker};

/// The biggest file a widget may pick.
pub const MAX_BYTES: u64 = 64 << 20;
/// The first path segment of a data URL.
pub const DATA_SEGMENT: &str = "__data";
/// Picked names are cut to this many characters (the extension is kept).
const MAX_NAME: usize = 100;
/// How many `name-2`, `name-3`, … to try before giving up.
const MAX_COPIES: u32 = 1000;
const RESERVED: [&str; 22] = [
    "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8",
    "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
];

/// An extension as the dialog filter and manifests spell it: letters and digits, no dot.
pub fn valid_extension(extension: &str) -> bool {
    (1..=16).contains(&extension.len()) && extension.chars().all(|c| c.is_ascii_alphanumeric())
}

/// A name that is safe as a single Windows path component, with no length limit applied:
/// the directory part of `raw` is dropped, characters Windows forbids become `_`, trailing dots
/// and spaces go, reserved device names get a `_` prefix, and nothing at all becomes `file`.
fn clean_name(raw: &str) -> String {
    let base = raw.rsplit(['/', '\\']).next().unwrap_or_default();
    let replaced: String = base
        .chars()
        .map(|c| {
            if c.is_control() || "<>:\"/\\|?*".contains(c) {
                '_'
            } else {
                c
            }
        })
        .collect();
    let trimmed = replaced.trim_start().trim_end_matches([' ', '.']);
    if trimmed.is_empty() {
        return "file".into();
    }
    let device = trimmed
        .split('.')
        .next()
        .unwrap_or_default()
        .to_ascii_uppercase();
    if RESERVED.contains(&device.as_str()) {
        format!("_{trimmed}")
    } else {
        trimmed.to_owned()
    }
}

/// `name.ext` as `("name", ".ext")`; a leading dot alone isn't an extension.
fn split_extension(name: &str) -> (&str, &str) {
    match name.rfind('.') {
        Some(at) if at > 0 => name.split_at(at),
        _ => (name, ""),
    }
}

/// The name a picked file is stored under: [`clean_name`], cut to [`MAX_NAME`] characters with
/// the extension kept.
pub fn sanitize_name(raw: &str) -> String {
    let name = clean_name(raw);
    if name.chars().count() <= MAX_NAME {
        return name;
    }
    let (stem, extension) = split_extension(&name);
    let extension = if extension.chars().count() <= 17 {
        extension
    } else {
        ""
    };
    let room = MAX_NAME - extension.chars().count();
    let stem: String = stem.chars().take(room).collect();
    clean_name(&format!("{stem}{extension}"))
}

/// Whether `name` is exactly what [`sanitize_name`] could have produced, up to its length
/// limit: one path component, nothing Windows treats specially. The only names ever served.
pub fn is_stored_name(name: &str) -> bool {
    clean_name(name) == name
}

/// `name`, then `stem-2.ext`, `stem-3.ext`, … for when `name` is taken.
fn candidate_names(name: &str) -> impl Iterator<Item = String> + '_ {
    let (stem, extension) = split_extension(name);
    std::iter::once(name.to_owned())
        .chain((2..MAX_COPIES).map(move |n| format!("{stem}-{n}{extension}")))
}

/// The first of [`candidate_names`] that `taken` doesn't claim.
#[cfg(test)]
fn unique_name(name: &str, taken: impl Fn(&str) -> bool) -> Option<String> {
    candidate_names(name).find(|candidate| !taken(candidate))
}

/// Percent-encodes everything but the URL's unreserved characters.
fn encode_segment(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for byte in text.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~') {
            out.push(char::from(byte));
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}

fn decode_segment(text: &str) -> Option<String> {
    let mut bytes = Vec::with_capacity(text.len());
    let mut raw = text.bytes();
    while let Some(b) = raw.next() {
        if b == b'%' {
            let hex = [raw.next()?, raw.next()?];
            bytes.push(u8::from_str_radix(std::str::from_utf8(&hex).ok()?, 16).ok()?);
        } else {
            bytes.push(b);
        }
    }
    String::from_utf8(bytes).ok()
}

/// `https://<widget>.widget.glaze/__data/<instance>/<name>`.
pub fn data_url(widget: &str, instance: &str, name: &str) -> String {
    format!(
        "https://{widget}{WIDGET_SUFFIX}/{DATA_SEGMENT}/{instance}/{}",
        encode_segment(name)
    )
}

/// The parts of a data URL.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DataUrl {
    pub widget: String,
    pub instance: String,
    pub name: String,
}

/// Reads a URL built by [`data_url`]; anything else (another host, a query, a name that isn't
/// a stored name) is `None`.
pub fn parse_data_url(url: &str) -> Option<DataUrl> {
    let (host, path) = url.strip_prefix("https://")?.split_once('/')?;
    let widget = host.strip_suffix(WIDGET_SUFFIX)?;
    let mut parts = path.split('/');
    let (segment, instance, name) = (parts.next()?, parts.next()?, parts.next()?);
    // `data_url` encodes everything else, so a raw `?` or `#` is a query or fragment.
    let plain = |c: char| c.is_ascii_alphanumeric() || "-._~%".contains(c);
    if !name.chars().all(plain) {
        return None;
    }
    let name = decode_segment(name)?;
    (segment == DATA_SEGMENT
        && parts.next().is_none()
        && valid_id(widget)
        && valid_id(instance)
        && is_stored_name(&name))
    .then(|| DataUrl {
        widget: widget.to_owned(),
        instance: instance.to_owned(),
        name,
    })
}

/// The same file under another instance's folder (a duplicated instance keeps its pictures).
pub fn rebase(url: &str, instance: &str) -> Option<String> {
    let DataUrl { widget, name, .. } = parse_data_url(url)?;
    Some(data_url(&widget, instance, &name))
}

/// Rebase only URLs owned by the duplicated instance, including nested settings/storage.
pub fn rebase_value(value: &mut serde_json::Value, from: &str, to: &str) {
    use serde_json::Value;
    match value {
        Value::String(url) => {
            if let Some(parts) = parse_data_url(url).filter(|p| p.instance == from) {
                *url = data_url(&parts.widget, to, &parts.name);
            }
        }
        Value::Array(values) => values.iter_mut().for_each(|v| rebase_value(v, from, to)),
        Value::Object(values) => values.values_mut().for_each(|v| rebase_value(v, from, to)),
        _ => {}
    }
}

/// `%APPDATA%\glaze\widget-data\<instance>\files`.
fn files_dir(root: &Path, instance: &str) -> PathBuf {
    root.join(instance).join("files")
}

/// The file behind `https://<widget>.widget.glaze/__data/<path>`, where `path` is
/// `<instance>/<name>` (already percent-decoded). `is_instance` says whether the instance
/// belongs to the widget being served. `None` for anything that isn't exactly that.
pub fn served_file(root: &Path, is_instance: impl Fn(&str) -> bool, path: &str) -> Option<PathBuf> {
    let (instance, name) = path.split_once('/')?;
    (valid_id(instance) && is_instance(instance) && is_stored_name(name))
        .then(|| files_dir(root, instance).join(name))
}

/// What a pick hands back: the URL to keep and the stored name.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Picked {
    pub url: String,
    pub name: String,
}

/// Creates `dir/<name>` (or `<name>-2`, …) without overwriting anything.
fn create_unique(dir: &Path, name: &str) -> io::Result<(File, String)> {
    for candidate in candidate_names(name) {
        match OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(dir.join(&candidate))
        {
            Ok(file) => return Ok((file, candidate)),
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {}
            Err(e) => return Err(e),
        }
    }
    Err(io::Error::new(
        io::ErrorKind::AlreadyExists,
        "too many files with that name",
    ))
}

/// Copies `source` into the instance's data folder. Files over [`MAX_BYTES`] are refused
/// (`InvalidParams`), and nothing is left behind.
pub fn import(
    root: &Path,
    widget: &str,
    instance: &str,
    source: &Path,
) -> Result<Picked, WidgetError> {
    let internal = |e: io::Error| WidgetError::new(ErrorCode::Internal, e.to_string());
    let too_big = || {
        WidgetError::new(
            ErrorCode::InvalidParams,
            format!("that file is larger than {} MB", MAX_BYTES >> 20),
        )
    };
    let meta = std::fs::metadata(source).map_err(internal)?;
    if !meta.is_file() {
        return Err(WidgetError::new(
            ErrorCode::InvalidParams,
            "that isn't a file",
        ));
    }
    if meta.len() > MAX_BYTES {
        return Err(too_big());
    }
    let name = sanitize_name(&source.file_name().unwrap_or_default().to_string_lossy());
    let dir = files_dir(root, instance);
    std::fs::create_dir_all(&dir).map_err(internal)?;
    let (mut copy, stored) = create_unique(&dir, &name).map_err(internal)?;
    let path = dir.join(&stored);
    // `take` guards against a file that grew after the check above.
    let copied = File::open(source)
        .and_then(|file| io::copy(&mut file.take(MAX_BYTES + 1), &mut copy))
        .inspect_err(|_| {
            let _ = std::fs::remove_file(&path);
        })
        .map_err(internal)?;
    if copied > MAX_BYTES {
        let _ = std::fs::remove_file(&path);
        return Err(too_big());
    }
    Ok(Picked {
        url: data_url(widget, instance, &stored),
        name: stored,
    })
}

/// Deletes an instance's data folder (it was removed).
pub fn remove_instance(root: &Path, instance: &str) {
    if !valid_id(instance) {
        return;
    }
    match std::fs::remove_dir_all(root.join(instance)) {
        Ok(()) => log::debug!("removed the files of {instance}"),
        Err(e) if e.kind() == io::ErrorKind::NotFound => {}
        Err(e) => log::warn!("couldn't remove the files of {instance}: {e}"),
    }
}

/// Copies one instance's files to another (a duplicate).
pub fn copy_instance(root: &Path, from: &str, to: &str) {
    if !valid_id(from) || !valid_id(to) {
        return;
    }
    let (source, target) = (files_dir(root, from), files_dir(root, to));
    let Ok(entries) = std::fs::read_dir(&source) else {
        return;
    };
    if let Err(e) = std::fs::create_dir_all(&target) {
        return log::warn!("couldn't copy the files of {from}: {e}");
    }
    for entry in entries.flatten().filter(|e| e.path().is_file()) {
        if let Err(e) = std::fs::copy(entry.path(), target.join(entry.file_name())) {
            log::warn!("couldn't copy {:?}: {e}", entry.file_name());
        }
    }
}

/// Native duplication must report a failed copy rather than saving a broken photo instance.
pub fn copy_instance_checked(root: &Path, from: &str, to: &str) -> anyhow::Result<()> {
    anyhow::ensure!(
        valid_id(from) && valid_id(to) && from != to,
        "invalid duplicate ids"
    );
    anyhow::ensure!(!root.join(to).exists(), "duplicate files already exist");
    let (source, target) = (files_dir(root, from), files_dir(root, to));
    let entries = match std::fs::read_dir(source) {
        Ok(entries) => entries,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(e.into()),
    };
    std::fs::create_dir_all(&target)?;
    for entry in entries {
        let entry = entry?;
        if entry.file_type()?.is_file() {
            std::fs::copy(entry.path(), target.join(entry.file_name()))?;
        }
    }
    Ok(())
}

// --- The dialog -----------------------------------------------------------------

/// Who asked: a widget's `pickFile` call (answered with the result), or a `file`/`image`
/// setting in the settings form (the setting is set to the URL).
#[derive(Debug)]
pub enum Origin {
    Call(Target),
    Setting { instance: String, key: String },
}

/// What a dialog needs to run and where its result goes.
pub struct Request {
    pub origin: Origin,
    pub widget: String,
    pub instance: String,
    pub extensions: Vec<String>,
    pub title: Option<String>,
    /// `widget-data`.
    pub root: PathBuf,
}

/// A finished pick: `Ok(None)` is a cancelled dialog.
pub struct Done {
    pub origin: Origin,
    pub result: Result<Option<Picked>, WidgetError>,
}

/// Shows the file dialog on a thread of its own and reports the result as [`Msg::Picked`].
pub fn spawn(request: Request, waker: Waker) -> io::Result<()> {
    std::thread::Builder::new()
        .name("pick-file".into())
        .spawn(move || {
            let Request {
                origin,
                widget,
                instance,
                extensions,
                title,
                root,
            } = request;
            // SAFETY: COM for the dialog on this short-lived thread; balanced below.
            let com =
                unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED | COINIT_DISABLE_OLE1DDE) }
                    .is_ok();
            let result = match choose(title.as_deref(), &extensions) {
                Ok(Some(path)) => import(&root, &widget, &instance, &path).map(Some),
                Ok(None) => Ok(None),
                Err(e) => Err(WidgetError::new(
                    ErrorCode::Internal,
                    format!("the file dialog failed: {e}"),
                )),
            };
            if com {
                // SAFETY: balances the CoInitializeEx above, on the same thread.
                unsafe { CoUninitialize() };
            }
            waker.send(Msg::Picked(Done { origin, result }));
        })
        .map(|_| ())
}

/// A zero-size, topmost, hidden-from-the-taskbar owner, so the dialog opens in front of
/// everything even though Glaze has no visible window of its own to own it.
struct Owner(HWND);

impl Owner {
    fn new() -> Self {
        // SAFETY: a built-in window class and no parent; destroyed in `drop` on this thread.
        let hwnd = unsafe {
            CreateWindowExW(
                WS_EX_TOPMOST | WS_EX_TOOLWINDOW,
                w!("STATIC"),
                w!("Glaze"),
                WS_POPUP,
                0,
                0,
                0,
                0,
                None,
                None,
                None,
                None,
            )
        }
        .unwrap_or_default();
        if !hwnd.is_invalid() {
            // SAFETY: our window; failing to take the foreground is harmless.
            unsafe {
                let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);
                let _ = SetForegroundWindow(hwnd);
            }
        }
        Self(hwnd)
    }
}

impl Drop for Owner {
    fn drop(&mut self) {
        if !self.0.is_invalid() {
            // SAFETY: the window `new` created, on its creating thread.
            let _ = unsafe { DestroyWindow(self.0) };
        }
    }
}

/// The path the user picked, or `None` if they cancelled.
fn choose(title: Option<&str>, extensions: &[String]) -> windows::core::Result<Option<PathBuf>> {
    let owner = Owner::new();
    // SAFETY: COM calls on a dialog this thread created; the strings outlive the calls.
    unsafe {
        let dialog: IFileOpenDialog =
            CoCreateInstance(&FileOpenDialog, None, CLSCTX_INPROC_SERVER)?;
        dialog.SetOptions(
            dialog.GetOptions()? | FOS_FORCEFILESYSTEM | FOS_FILEMUSTEXIST | FOS_PATHMUSTEXIST,
        )?;
        if let Some(title) = title {
            dialog.SetTitle(&HSTRING::from(title))?;
        }
        let name = HSTRING::from("Supported files");
        let pattern = HSTRING::from(
            extensions
                .iter()
                .map(|e| format!("*.{e}"))
                .collect::<Vec<_>>()
                .join(";"),
        );
        if !extensions.is_empty() {
            dialog.SetFileTypes(&[COMDLG_FILTERSPEC {
                pszName: PCWSTR(name.as_ptr()),
                pszSpec: PCWSTR(pattern.as_ptr()),
            }])?;
        }
        let owner = (!owner.0.is_invalid()).then_some(owner.0);
        if let Err(e) = dialog.Show(owner) {
            return if e.code() == HRESULT::from_win32(ERROR_CANCELLED.0) {
                Ok(None)
            } else {
                Err(e)
            };
        }
        let path = dialog.GetResult()?.GetDisplayName(SIGDN_FILESYSPATH)?;
        let picked = PathBuf::from(path.to_string()?);
        CoTaskMemFree(Some(path.as_ptr().cast()));
        Ok(Some(picked))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_are_made_safe() {
        for (raw, safe) in [
            ("photo.png", "photo.png"),
            ("My Photo (1).jpg", "My Photo (1).jpg"),
            ("..\\..\\evil.png", "evil.png"),
            ("C:\\x\\y\\a.txt", "a.txt"),
            ("a:b*c?.png", "a_b_c_.png"),
            ("trail. . ", "trail"),
            ("  lead.png", "lead.png"),
            ("..", "file"),
            ("", "file"),
            ("nul.txt", "_nul.txt"),
            ("COM1", "_COM1"),
            ("con", "_con"),
            ("tab\tname.png", "tab_name.png"),
            ("日本語.png", "日本語.png"),
        ] {
            assert_eq!(sanitize_name(raw), safe, "{raw:?}");
            assert!(is_stored_name(safe), "{safe:?}");
        }
        for raw in ["a/b.png", "x\\y", "..", ".", "a.", "a ", "nul", "a|b", ""] {
            assert!(!is_stored_name(raw), "{raw:?} is not a stored name");
        }
    }

    #[test]
    fn sanitizing_is_idempotent_and_bounded() {
        let long = format!("{}.png", "n".repeat(300));
        let name = sanitize_name(&long);
        assert_eq!(name.chars().count(), MAX_NAME);
        assert!(name.ends_with(".png"));
        for raw in [long.as_str(), "a/b", " x. ", "CON.txt", "a:b", "ok.txt"] {
            let once = sanitize_name(raw);
            assert_eq!(sanitize_name(&once), once, "{raw:?}");
        }
        // A huge "extension" is part of the stem instead of eating the whole name.
        let odd = format!("a.{}", "e".repeat(200));
        assert!(sanitize_name(&odd).chars().count() <= MAX_NAME);
    }

    #[test]
    fn taken_names_get_a_numeric_suffix() {
        let existing = ["a.png", "a-2.png", "a-3.png"];
        let taken = |candidate: &str| existing.contains(&candidate);
        assert_eq!(unique_name("b.png", taken).as_deref(), Some("b.png"));
        assert_eq!(unique_name("a.png", taken).as_deref(), Some("a-4.png"));
        assert_eq!(unique_name("a.png", |_| false).as_deref(), Some("a.png"));
        assert_eq!(unique_name("a.png", |_| true), None, "gives up eventually");
        // No extension, and a leading dot isn't one.
        assert_eq!(
            unique_name("README", |c| c == "README").as_deref(),
            Some("README-2")
        );
        assert_eq!(
            unique_name(".env", |c| c == ".env").as_deref(),
            Some(".env-2")
        );
    }

    #[test]
    fn data_urls_round_trip() {
        for name in [
            "photo.png",
            "My Photo (1).jpg",
            "日本語.png",
            "a%b#c&d=e.txt",
        ] {
            let url = data_url("viz", "viz-1", name);
            assert!(url.starts_with("https://viz.widget.glaze/__data/viz-1/"));
            assert!(!url.contains([' ', '(', '#']), "{url}");
            let sanitized = sanitize_name(name);
            let parsed = parse_data_url(&url);
            assert_eq!(
                parsed,
                (sanitized == name).then(|| DataUrl {
                    widget: "viz".into(),
                    instance: "viz-1".into(),
                    name: name.into()
                }),
                "{url}"
            );
        }
        assert_eq!(
            data_url("viz", "viz-1", "My Photo.png"),
            "https://viz.widget.glaze/__data/viz-1/My%20Photo.png"
        );
    }

    #[test]
    fn only_data_urls_parse() {
        for bad in [
            "",
            "https://viz.widget.glaze/__data/viz-1/",
            "https://viz.widget.glaze/__data/viz-1/a.png?x=1",
            "https://viz.widget.glaze/__data/viz-1/a.png#x",
            "https://viz.widget.glaze/__data/viz-1/a/b.png",
            "https://viz.widget.glaze/__data/viz-1/..%2Fa.png",
            "https://viz.widget.glaze/__data/viz-1/%2e%2e",
            "https://viz.widget.glaze/__data/viz-1/a%5Cb.png",
            "https://viz.widget.glaze/__data/../a.png",
            "https://viz.widget.glaze/__data/Viz-1/a.png",
            "https://viz.widget.glaze/other/viz-1/a.png",
            "https://viz.widget.Glaze:8443/__data/viz-1/a.png",
            "http://viz.widget.glaze/__data/viz-1/a.png",
            "https://evil.com/__data/viz-1/a.png",
            "https://a.b.widget.glaze/__data/viz-1/a.png",
            "https://viz.widget.glaze/__data/viz-1/%zz",
            "https://viz.widget.glaze/__data/viz-1/%FF.png",
        ] {
            assert_eq!(parse_data_url(bad), None, "{bad}");
        }
    }

    #[test]
    fn duplicates_point_at_their_own_copy() {
        let url = data_url("viz", "viz-1", "My Photo.png");
        assert_eq!(
            rebase(&url, "viz-2"),
            Some(data_url("viz", "viz-2", "My Photo.png"))
        );
        assert_eq!(rebase("https://evil.com/x", "viz-2"), None);
    }

    #[test]
    fn only_a_widgets_own_instances_are_served() {
        let root = Path::new("data");
        let mine = |instance: &str| instance == "viz-1";
        assert_eq!(
            served_file(root, mine, "viz-1/a.png"),
            Some(root.join("viz-1").join("files").join("a.png"))
        );
        for bad in [
            "viz-2/a.png",
            "viz-1/../viz-2/a.png",
            "viz-1/..",
            "viz-1/a/b.png",
            "viz-1/a\\b.png",
            "viz-1/nul",
            "viz-1/",
            "viz-1",
            "../viz-1/a.png",
            "viz-1/C:evil",
        ] {
            assert_eq!(served_file(root, mine, bad), None, "{bad}");
        }
    }

    #[test]
    fn imports_copy_rename_and_refuse_nothing_silently() {
        let dir = crate::files::temp_dir("pick");
        let root = dir.join("widget-data");
        let source = dir.join("my pic.png");
        std::fs::write(&source, b"png").unwrap();

        let first = import(&root, "viz", "viz-1", &source).unwrap();
        assert_eq!(first.name, "my pic.png");
        assert_eq!(
            first.url,
            "https://viz.widget.glaze/__data/viz-1/my%20pic.png"
        );
        let second = import(&root, "viz", "viz-1", &source).unwrap();
        assert_eq!(second.name, "my pic-2.png");
        let served = served_file(&root, |i| i == "viz-1", "viz-1/my pic-2.png").unwrap();
        assert_eq!(std::fs::read(served).unwrap(), b"png");

        // A duplicate gets its own copy; a removed instance takes its files with it.
        copy_instance(&root, "viz-1", "viz-2");
        assert!(files_dir(&root, "viz-2").join("my pic.png").is_file());
        remove_instance(&root, "viz-1");
        assert!(!root.join("viz-1").exists());
        assert!(root.join("viz-2").exists());
        remove_instance(&root, "viz-1"); // already gone: fine
        remove_instance(&root, "..\\viz-2"); // not an id: ignored
        assert!(root.join("viz-2").exists());

        assert_eq!(
            import(&root, "viz", "viz-1", &dir).unwrap_err().code,
            ErrorCode::InvalidParams
        );
        assert_eq!(
            import(&root, "viz", "viz-1", &dir.join("gone.png"))
                .unwrap_err()
                .code,
            ErrorCode::Internal
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn files_over_the_limit_are_refused() {
        let dir = crate::files::temp_dir("pick-big");
        let source = dir.join("big.bin");
        let file = File::create(&source).unwrap();
        file.set_len(MAX_BYTES + 1).unwrap(); // sparse: no real disk use
        drop(file);
        let error = import(&dir.join("data"), "viz", "viz-1", &source).unwrap_err();
        assert_eq!(error.code, ErrorCode::InvalidParams);
        assert!(
            !dir.join("data")
                .join("viz-1")
                .join("files")
                .join("big.bin")
                .exists()
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn extensions_are_letters_and_digits() {
        for ok in ["png", "JPG", "mp3", "7z"] {
            assert!(valid_extension(ok), "{ok}");
        }
        for bad in ["", ".png", "p/g", "p g", "*", &"x".repeat(17)] {
            assert!(!valid_extension(bad), "{bad:?}");
        }
    }
}
