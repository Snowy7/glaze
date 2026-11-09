//! Windows' own desktop wallpaper, per monitor (`IDesktopWallpaper`), with an undo journal.
//!
//! The static snapshot sets a still of the theme as the Windows wallpaper, so Windows shows the
//! theme's look before Glaze starts. That replaces whatever the user had, so it is reversible:
//! before the first change of a monitor, its original path is written to
//! `%LOCALAPPDATA%\glaze\wallpaper.json`, and [`restore`] (feature turned off, Glaze disabled
//! or uninstalled, the panic hotkey, a crashed run replayed by core) puts every original back,
//! deletes the journal and our snapshot files. The logic runs against the [`Backend`] trait, so
//! it is tested without touching the real wallpaper, like `taskbar.rs` in the bars crate.
//!
//! Ownership is told by location, not by remembering the last file: a monitor whose current
//! wallpaper lives in our snapshot folder is ours. If it is anything else the user changed it
//! since, so theirs becomes the new original and [`Wallpapers::restore`] leaves it alone.
//!
//! Limits: a slideshow or Spotlight background cannot be put back from a path, so it is never
//! replaced ([`Backend::replaceable`]). Windows' fit mode is global; `Span` would stretch one
//! image over all monitors, so it is switched to `Fill` (and put back) while we have a snapshot.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use windows::Win32::Foundation::{ERROR_FILE_NOT_FOUND, RPC_E_CHANGED_MODE};
use windows::Win32::System::Com::{
    CLSCTX_LOCAL_SERVER, COINIT_APARTMENTTHREADED, CoCreateInstance, CoInitializeEx, CoTaskMemFree,
    CoUninitialize,
};
use windows::Win32::System::Registry::{HKEY_CURRENT_USER, RRF_RT_REG_DWORD, RegGetValueW};
use windows::Win32::UI::Shell::{
    DESKTOP_WALLPAPER_POSITION, DWPOS_FILL, DWPOS_SPAN, DesktopWallpaper, IDesktopWallpaper,
};
use windows::core::{HSTRING, PCWSTR, PWSTR, w};

use crate::Rect;

pub const JOURNAL: &str = "wallpaper.json";
/// Where our stills live, under `%LOCALAPPDATA%\glaze`.
pub const SNAPSHOT_DIR: &str = "snapshot";

/// `DESKTOP_WALLPAPER_POSITION` values we deal with.
const FILL: i32 = DWPOS_FILL.0;
const SPAN: i32 = DWPOS_SPAN.0;

const WALLPAPERS_KEY: PCWSTR = w!(r"Software\Microsoft\Windows\CurrentVersion\Explorer\Wallpapers");
const BACKGROUND_TYPE: PCWSTR = w!("BackgroundType");

/// One monitor as Windows' wallpaper settings see it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Slot {
    /// The monitor's device path, Windows' id for it.
    pub id: String,
    pub rect: Rect,
    /// The wallpaper file in use, or empty for a solid color.
    pub wallpaper: String,
}

impl Slot {
    /// Does this monitor currently show `file`?
    pub fn shows(&self, file: &Path) -> bool {
        normalize(&self.wallpaper) == normalize(&file.to_string_lossy())
    }
}

/// The Windows wallpaper as the operating system has it.
pub trait Backend {
    /// Every attached monitor with its current wallpaper.
    fn monitors(&self) -> Result<Vec<Slot>>;
    /// Sets one monitor's wallpaper; an empty path is a solid color.
    fn set_wallpaper(&mut self, id: &str, path: &str) -> Result<()>;
    /// The global fit mode, as a `DESKTOP_WALLPAPER_POSITION` value.
    fn position(&self) -> Result<i32>;
    fn set_position(&mut self, position: i32) -> Result<()>;
    /// Is the background a picture or solid color, i.e. something a path can bring back?
    /// False for a slideshow or Spotlight.
    fn replaceable(&self) -> Result<bool>;
}

/// The undo journal: what each thing was before we first changed it.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Journal {
    /// Monitor id to its wallpaper path before ours (empty: solid color).
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    monitors: BTreeMap<String, String>,
    /// The fit mode before we replaced `Span` with `Fill`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    position: Option<i32>,
}

/// Sets our stills as the Windows wallpaper, journaled at `journal`; our files live in `dir`.
pub struct Wallpapers<B> {
    backend: B,
    journal: PathBuf,
    dir: PathBuf,
}

impl Wallpapers<Native> {
    /// The real wallpaper, journaled under `%LOCALAPPDATA%\glaze`.
    pub fn native() -> Result<Self> {
        Self::native_in(&local_dir()?)
    }

    /// The real wallpaper, with the journal and snapshot folder under `base` (for tests).
    pub fn native_in(base: &Path) -> Result<Self> {
        Ok(Self::new(
            Native::new()?,
            base.join(JOURNAL),
            base.join(SNAPSHOT_DIR),
        ))
    }
}

impl<B: Backend> Wallpapers<B> {
    pub fn new(backend: B, journal: PathBuf, dir: PathBuf) -> Self {
        Self {
            backend,
            journal,
            dir,
        }
    }

    /// The folder our stills go in.
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    pub fn monitors(&self) -> Result<Vec<Slot>> {
        self.backend.monitors()
    }

    /// Is there a journal, i.e. something to undo?
    pub fn pending(&self) -> bool {
        self.journal.exists()
    }

    /// Sets `file` (inside [`dir`](Self::dir)) as monitor `id`'s wallpaper. The monitor's
    /// original is journaled first and kept through later calls; a wallpaper the user chose in
    /// the meantime becomes the new original. Refuses a slideshow or Spotlight background.
    pub fn apply(&mut self, id: &str, file: &Path) -> Result<()> {
        let slot = self
            .backend
            .monitors()?
            .into_iter()
            .find(|slot| slot.id == id)
            .with_context(|| format!("monitor {id} is not attached"))?;
        if !self.backend.replaceable()? {
            bail!("the desktop background is a slideshow or Spotlight: leaving it alone");
        }

        let mut journal = self.load()?.unwrap_or_default();
        let before = journal.clone();
        if !in_dir(&self.dir, &slot.wallpaper) {
            journal.monitors.insert(id.to_owned(), slot.wallpaper);
        } else if !journal.monitors.contains_key(id) {
            log::warn!("{id}: our still is set but the original is lost; it can't be put back");
        }
        let span = self.backend.position()? == SPAN;
        if span {
            journal.position.get_or_insert(SPAN);
        }
        // Write-ahead: the originals are on disk before anything changes.
        if journal != before {
            self.save(&journal)?;
        }

        if span {
            self.backend.set_position(FILL)?;
        }
        self.backend.set_wallpaper(id, &file.to_string_lossy())
    }

    /// Puts every original back, then deletes the journal and our stills. A monitor whose
    /// wallpaper is no longer ours (the user changed it, or it is unplugged) is left as it is.
    /// Failures keep the journal for what could not be undone. Returns whether there was a
    /// journal.
    pub fn restore(&mut self) -> Result<bool> {
        let Some(mut journal) = self.load()? else {
            return Ok(false);
        };
        let slots = self.backend.monitors()?;
        let mut failures = Vec::new();

        journal.monitors.retain(|id, original| {
            let ours = slots
                .iter()
                .find(|slot| slot.id == *id)
                .is_some_and(|slot| in_dir(&self.dir, &slot.wallpaper));
            if !ours {
                return false;
            }
            let mut restored = self.backend.set_wallpaper(id, original);
            if restored.is_err() && !original.is_empty() && !Path::new(original).exists() {
                // The original file is gone (deleted, or a still from before the rename), so it
                // can never come back; Windows' own default beats keeping the journal forever.
                log::warn!("{id}: the original wallpaper {original} no longer exists");
                restored = self.backend.set_wallpaper(id, &windows_default());
            }
            if let Err(e) = &restored {
                failures.push(format!("{id}: {e:#}"));
            }
            restored.is_err()
        });
        if let Some(position) = journal.position {
            match self.backend.set_position(position) {
                Ok(()) => journal.position = None,
                Err(e) => failures.push(format!("fit mode: {e:#}")),
            }
        }

        if !failures.is_empty() {
            self.save(&journal)?;
            bail!("wallpaper: {}", failures.join("; "));
        }
        match std::fs::remove_file(&self.journal) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(e).context("deleting the wallpaper journal"),
        }
        self.remove_stills(|_| true);
        Ok(true)
    }

    /// Deletes the stills in our folder that no monitor shows (older themes, leftovers).
    pub fn sweep(&mut self) -> Result<()> {
        let shown: Vec<String> = self
            .backend
            .monitors()?
            .iter()
            .map(|slot| normalize(&slot.wallpaper))
            .collect();
        self.remove_stills(|path| !shown.contains(&normalize(&path.to_string_lossy())));
        Ok(())
    }

    /// Deletes our `.jpg` stills (and unfinished `.tmp` ones) that `doomed` selects, then the
    /// folder if it is empty. Nothing else in the folder is touched.
    fn remove_stills(&self, doomed: impl Fn(&Path) -> bool) {
        let Ok(entries) = std::fs::read_dir(&self.dir) else {
            return;
        };
        for path in entries.flatten().map(|e| e.path()) {
            let ours = path
                .extension()
                .is_some_and(|ext| ext == "jpg" || ext == "tmp");
            if ours && doomed(&path) {
                let _ = std::fs::remove_file(&path);
            }
        }
        let _ = std::fs::remove_dir(&self.dir); // only succeeds when empty
    }

    /// `None` if there is no journal. An unreadable one is set aside as `wallpaper.json.bad`
    /// rather than blocking every start.
    fn load(&self) -> Result<Option<Journal>> {
        let text = match std::fs::read_to_string(&self.journal) {
            Ok(text) => text,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(e).context("reading the wallpaper journal"),
        };
        match serde_json::from_str(&text) {
            Ok(journal) => Ok(Some(journal)),
            Err(e) => {
                let bad = self.journal.with_extension("json.bad");
                let _ = std::fs::rename(&self.journal, &bad);
                bail!(
                    "the wallpaper journal is corrupt ({e}); moved to {}",
                    bad.display()
                )
            }
        }
    }

    fn save(&self, journal: &Journal) -> Result<()> {
        let text = serde_json::to_string_pretty(journal)? + "\n";
        write_atomic(&self.journal, text.as_bytes())
    }
}

/// Is there a wallpaper journal to undo?
pub fn pending() -> bool {
    local_dir().is_ok_and(|base| pending_in(&base))
}

/// [`pending`] for a journal under `base` instead of `%LOCALAPPDATA%\glaze`.
pub fn pending_in(base: &Path) -> bool {
    base.join(JOURNAL).exists()
}

/// Puts the user's wallpaper back from the journal (see [`Wallpapers::restore`]). Returns whether
/// there was anything to undo; without a journal it touches nothing, not even COM. Safe to call
/// from any thread, any number of times: on exit, from the panic hotkey, by core after a crash.
pub fn restore() -> Result<bool> {
    restore_in(&local_dir()?)
}

/// [`restore`] for a journal under `base` instead of `%LOCALAPPDATA%\glaze`.
pub fn restore_in(base: &Path) -> Result<bool> {
    if !pending_in(base) {
        return Ok(false);
    }
    Wallpapers::native_in(base)?.restore()
}

/// Windows' stock wallpaper, or a solid color (empty) if this edition doesn't have one. What a
/// monitor gets back when its original file was deleted.
fn windows_default() -> String {
    std::env::var_os("WINDIR")
        .map(|dir| PathBuf::from(dir).join(r"Web\Wallpaper\Windows\img0.jpg"))
        .filter(|path| path.exists())
        .map(|path| path.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// `%LOCALAPPDATA%\glaze`.
pub fn local_dir() -> Result<PathBuf> {
    let local = std::env::var_os("LOCALAPPDATA").context("LOCALAPPDATA is not set")?;
    Ok(PathBuf::from(local).join("glaze"))
}

/// Writes a file atomically (temp file plus rename).
fn write_atomic(path: &Path, bytes: &[u8]) -> Result<()> {
    std::fs::create_dir_all(path.parent().context("path has no parent")?)?;
    let mut temp = path.as_os_str().to_owned();
    temp.push(".tmp");
    let temp = PathBuf::from(temp);
    std::fs::write(&temp, bytes).with_context(|| format!("writing {}", temp.display()))?;
    std::fs::rename(&temp, path).with_context(|| format!("replacing {}", path.display()))
}

/// A path in a form two spellings of it share: Windows paths ignore case and take either slash.
fn normalize(path: &str) -> String {
    path.trim_start_matches(r"\\?\")
        .replace('/', r"\")
        .trim_end_matches('\\')
        .to_ascii_lowercase()
}

/// Is `wallpaper` a file in `dir`, i.e. one of our stills?
fn in_dir(dir: &Path, wallpaper: &str) -> bool {
    Path::new(wallpaper).parent().is_some_and(|parent| {
        normalize(&parent.to_string_lossy()) == normalize(&dir.to_string_lossy())
    })
}

/// Keeps COM initialized (single-threaded apartment) on the creating thread. Tolerates a thread
/// that already chose another apartment model.
pub struct Apartment {
    /// Whether our `CoInitializeEx` succeeded and so needs a matching uninit.
    owned: bool,
}

impl Apartment {
    pub fn enter() -> Result<Self> {
        // SAFETY: plain per-thread COM init; balanced by `Drop` on success.
        let hr = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) };
        if hr == RPC_E_CHANGED_MODE {
            return Ok(Self { owned: false });
        }
        hr.ok().context("initializing COM")?;
        Ok(Self { owned: true })
    }
}

impl Drop for Apartment {
    fn drop(&mut self) {
        if self.owned {
            // SAFETY: balances the successful `CoInitializeEx` in `enter`, on the same thread
            // (an `Apartment` lives next to COM objects, which are not `Send`).
            unsafe { CoUninitialize() };
        }
    }
}

/// The real Windows wallpaper. Bound to the thread that created it.
pub struct Native {
    // Field order matters: COM objects must be released before COM is uninitialized.
    wallpaper: IDesktopWallpaper,
    _apartment: Apartment,
}

impl Native {
    /// Initializes COM on this thread and connects to Explorer's wallpaper service.
    pub fn new() -> Result<Self> {
        let apartment = Apartment::enter()?;
        // SAFETY: COM is initialized on this thread; the CLSID names the desktop wallpaper service.
        let wallpaper: IDesktopWallpaper =
            unsafe { CoCreateInstance(&DesktopWallpaper, None, CLSCTX_LOCAL_SERVER) }
                .context("connecting to the desktop wallpaper service (is Explorer running?)")?;
        Ok(Self {
            wallpaper,
            _apartment: apartment,
        })
    }
}

/// Copies a COM-allocated wide string and frees it.
fn take(string: PWSTR) -> String {
    // SAFETY: `string` was just returned by a COM call: a valid, NUL-terminated, CoTaskMem-allocated
    // string that nothing else frees.
    unsafe {
        let text = String::from_utf16_lossy(string.as_wide());
        CoTaskMemFree(Some(string.as_ptr().cast_const().cast()));
        text
    }
}

impl Backend for Native {
    fn monitors(&self) -> Result<Vec<Slot>> {
        // SAFETY: COM calls on this thread's wallpaper service; strings are freed by `take`.
        unsafe {
            let count = self.wallpaper.GetMonitorDevicePathCount()?;
            let mut slots = Vec::new();
            for index in 0..count {
                let id = take(self.wallpaper.GetMonitorDevicePathAt(index)?);
                let id16 = HSTRING::from(id.as_str());
                // Fails for a monitor that isn't attached right now.
                let Ok(rect) = self.wallpaper.GetMonitorRECT(&id16) else {
                    continue;
                };
                let Ok(wallpaper) = self.wallpaper.GetWallpaper(&id16) else {
                    log::debug!("no wallpaper readable for {id}");
                    continue;
                };
                slots.push(Slot {
                    id,
                    rect: rect.into(),
                    wallpaper: take(wallpaper),
                });
            }
            Ok(slots)
        }
    }

    fn set_wallpaper(&mut self, id: &str, path: &str) -> Result<()> {
        // SAFETY: a COM call with strings that outlive it.
        unsafe {
            self.wallpaper
                .SetWallpaper(&HSTRING::from(id), &HSTRING::from(path))
        }
        .with_context(|| format!("setting the wallpaper of {id}"))
    }

    fn position(&self) -> Result<i32> {
        // SAFETY: a COM call on this thread's wallpaper service.
        Ok(unsafe { self.wallpaper.GetPosition() }?.0)
    }

    fn set_position(&mut self, position: i32) -> Result<()> {
        // SAFETY: a COM call on this thread's wallpaper service.
        unsafe {
            self.wallpaper
                .SetPosition(DESKTOP_WALLPAPER_POSITION(position))
        }
        .context("setting the wallpaper fit mode")
    }

    fn replaceable(&self) -> Result<bool> {
        let mut kind = 0u32;
        let mut bytes = size_of::<u32>() as u32;
        // SAFETY: `kind` and `bytes` outlive the call and match RRF_RT_REG_DWORD.
        let status = unsafe {
            RegGetValueW(
                HKEY_CURRENT_USER,
                WALLPAPERS_KEY,
                BACKGROUND_TYPE,
                RRF_RT_REG_DWORD,
                None,
                Some((&raw mut kind).cast()),
                Some(&mut bytes),
            )
        };
        if status == ERROR_FILE_NOT_FOUND {
            return Ok(true); // never set: a picture
        }
        status.ok().context("reading the background type")?;
        // 0 picture, 1 solid color; anything else (slideshow, Spotlight) is not ours to replace.
        Ok(kind <= 1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The Windows wallpaper in memory.
    struct Fake {
        slots: Vec<Slot>,
        position: i32,
        replaceable: bool,
        /// Monitors whose `SetWallpaper` fails.
        broken: Vec<String>,
        /// Files `SetWallpaper` can't find.
        missing: Vec<String>,
        sets: usize,
    }

    impl Backend for Fake {
        fn monitors(&self) -> Result<Vec<Slot>> {
            Ok(self.slots.clone())
        }
        fn set_wallpaper(&mut self, id: &str, path: &str) -> Result<()> {
            if self.broken.iter().any(|b| b == id) {
                bail!("Explorer said no");
            }
            if self.missing.iter().any(|m| m == path) {
                bail!("the file does not exist");
            }
            self.sets += 1;
            self.slots
                .iter_mut()
                .find(|s| s.id == id)
                .unwrap()
                .wallpaper = path.to_owned();
            Ok(())
        }
        fn position(&self) -> Result<i32> {
            Ok(self.position)
        }
        fn set_position(&mut self, position: i32) -> Result<()> {
            self.position = position;
            Ok(())
        }
        fn replaceable(&self) -> Result<bool> {
            Ok(self.replaceable)
        }
    }

    const FIRST: &str = r"\\?\DISPLAY#A";
    const SECOND: &str = r"\\?\DISPLAY#B";
    const THEIRS: &str = r"C:\Pictures\mine.jpg";
    const OTHER: &str = r"C:\Pictures\other.jpg";

    fn slot(id: &str, wallpaper: &str) -> Slot {
        Slot {
            id: id.into(),
            rect: Rect {
                x: 0,
                y: 0,
                width: 1920,
                height: 1080,
            },
            wallpaper: wallpaper.into(),
        }
    }

    /// Two monitors showing the user's own pictures, and a journal location in a fresh folder.
    fn wallpapers(name: &str) -> Wallpapers<Fake> {
        let base =
            std::env::temp_dir().join(format!("glaze-wallpaper-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(base.join(SNAPSHOT_DIR)).unwrap();
        let fake = Fake {
            slots: vec![slot(FIRST, THEIRS), slot(SECOND, OTHER)],
            position: 4,
            replaceable: true,
            broken: Vec::new(),
            missing: Vec::new(),
            sets: 0,
        };
        Wallpapers::new(fake, base.join(JOURNAL), base.join(SNAPSHOT_DIR))
    }

    /// Creates a still in the snapshot folder.
    fn still(w: &Wallpapers<Fake>, name: &str) -> PathBuf {
        let path = w.dir().join(name);
        std::fs::write(&path, b"jpg").unwrap();
        path
    }

    fn shown(w: &Wallpapers<Fake>, id: &str) -> String {
        w.backend
            .slots
            .iter()
            .find(|s| s.id == id)
            .unwrap()
            .wallpaper
            .clone()
    }

    #[test]
    fn originals_are_kept_through_later_changes_and_restored_exactly() {
        let mut w = wallpapers("originals");
        let (one, two, three) = (still(&w, "1.jpg"), still(&w, "2.jpg"), still(&w, "3.jpg"));
        w.apply(FIRST, &one).unwrap();
        w.apply(SECOND, &two).unwrap();
        w.apply(FIRST, &three).unwrap(); // a theme change: new file, same original
        assert!(w.pending());
        assert_eq!(shown(&w, FIRST), three.to_string_lossy());

        assert!(w.restore().unwrap());
        assert_eq!(
            (shown(&w, FIRST).as_str(), shown(&w, SECOND).as_str()),
            (THEIRS, OTHER)
        );
        assert!(!w.pending());
        assert!(!w.dir().exists(), "our stills are gone with the journal");
        assert!(!w.restore().unwrap(), "nothing left to undo");
    }

    #[test]
    fn a_wallpaper_the_user_picked_meanwhile_is_the_new_original() {
        let mut w = wallpapers("user-change");
        let (one, two) = (still(&w, "1.jpg"), still(&w, "2.jpg"));
        w.apply(FIRST, &one).unwrap();
        w.backend.slots[0].wallpaper = r"C:\Pictures\new.JPG".into(); // the user, in Settings
        w.apply(FIRST, &two).unwrap();

        w.restore().unwrap();
        assert_eq!(shown(&w, FIRST), r"C:\Pictures\new.JPG");
    }

    #[test]
    fn restore_leaves_a_monitor_the_user_changed_since() {
        let mut w = wallpapers("left-alone");
        let one = still(&w, "1.jpg");
        w.apply(FIRST, &one).unwrap();
        w.backend.slots[0].wallpaper = r"C:\Pictures\new.jpg".into();
        let sets = w.backend.sets;

        w.restore().unwrap();
        assert_eq!(shown(&w, FIRST), r"C:\Pictures\new.jpg");
        assert_eq!(
            w.backend.sets, sets,
            "no write for a monitor that isn't ours"
        );
        assert!(!w.pending());
    }

    #[test]
    fn spans_are_filled_while_ours_and_put_back() {
        let mut w = wallpapers("span");
        w.backend.position = SPAN;
        let one = still(&w, "1.jpg");
        w.apply(FIRST, &one).unwrap();
        assert_eq!(w.backend.position, FILL);
        w.apply(FIRST, &one).unwrap(); // the journal keeps Span, not our Fill

        w.restore().unwrap();
        assert_eq!(w.backend.position, SPAN);
    }

    #[test]
    fn a_failed_restore_keeps_the_journal_for_what_is_left() {
        let mut w = wallpapers("partial");
        let (one, two) = (still(&w, "1.jpg"), still(&w, "2.jpg"));
        w.apply(FIRST, &one).unwrap();
        w.apply(SECOND, &two).unwrap();
        w.backend.broken.push(SECOND.into());

        assert!(w.restore().is_err());
        assert_eq!(shown(&w, FIRST), THEIRS, "the other monitor was restored");
        assert!(w.pending());
        assert!(
            one.exists() && two.exists(),
            "stills stay while a monitor may show them"
        );

        w.backend.broken.clear();
        assert!(w.restore().unwrap());
        assert_eq!(shown(&w, SECOND), OTHER);
        assert!(!w.pending());
    }

    #[test]
    fn a_deleted_original_falls_back_to_the_windows_default_instead_of_failing_forever() {
        let mut w = wallpapers("deleted-original");
        let gone = r"C:\Glaze-test-nowhere\old-snapshot.jpg";
        w.backend.slots[0].wallpaper = gone.into();
        let one = still(&w, "1.jpg");
        w.apply(FIRST, &one).unwrap();
        w.backend.missing.push(gone.into());

        assert!(w.restore().unwrap());
        assert_eq!(shown(&w, FIRST), windows_default());
        assert!(!w.pending(), "an original that can't come back isn't kept");
    }

    #[test]
    fn a_slideshow_is_never_replaced() {
        let mut w = wallpapers("slideshow");
        w.backend.replaceable = false;
        let one = still(&w, "1.jpg");
        assert!(w.apply(FIRST, &one).is_err());
        assert_eq!((w.backend.sets, w.pending()), (0, false));
    }

    #[test]
    fn sweep_deletes_only_stills_no_monitor_shows() {
        let mut w = wallpapers("sweep");
        let (old, current) = (still(&w, "old.jpg"), still(&w, "current.jpg"));
        let leftover = still(&w, "half.jpg.tmp");
        let foreign = still(&w, "notes.txt");
        w.apply(FIRST, &current).unwrap();

        w.sweep().unwrap();
        assert!(current.exists() && foreign.exists());
        assert!(!old.exists() && !leftover.exists());
    }

    #[test]
    fn the_journal_round_trips_and_a_corrupt_one_is_set_aside() {
        let journal = Journal {
            monitors: BTreeMap::from([("a".to_owned(), String::new())]),
            position: Some(SPAN),
        };
        let text = serde_json::to_string(&journal).unwrap();
        assert_eq!(text, r#"{"monitors":{"a":""},"position":5}"#);
        assert_eq!(serde_json::from_str::<Journal>(&text).unwrap(), journal);
        assert_eq!(
            serde_json::from_str::<Journal>("{}").unwrap(),
            Journal::default()
        );

        let w = wallpapers("corrupt");
        std::fs::write(&w.journal, "{ not json").unwrap();
        assert!(w.load().is_err());
        assert!(!w.pending());
        assert!(w.journal.with_extension("json.bad").exists());
    }

    #[test]
    fn paths_compare_like_windows_does() {
        assert_eq!(normalize(r"C:\A\b.JPG"), normalize("c:/a/B.jpg"));
        assert_eq!(normalize(r"\\?\C:\A\"), normalize(r"c:\a"));
        assert!(slot("x", r"C:\Snap\a.jpg").shows(Path::new("c:/snap/A.JPG")));
    }
}
