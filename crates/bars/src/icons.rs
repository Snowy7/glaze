//! App names, shortcut identities and 64 px PNG icons, resolved on an STA worker and cached
//! on disk. Native callers receive file paths; there is no virtual host or browser cache.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Sender};

use windows::Win32::Foundation::{GENERIC_WRITE, SIZE};
use windows::Win32::Graphics::Gdi::{
    BI_RGB, BITMAP, BITMAPINFO, BITMAPINFOHEADER, CreateCompatibleDC, DIB_RGB_COLORS, DeleteDC,
    DeleteObject, GetDIBits, GetObjectW, HBITMAP,
};
use windows::Win32::Graphics::Imaging::{
    CLSID_WICImagingFactory, GUID_ContainerFormatPng, GUID_WICPixelFormat32bppBGRA,
    IWICImagingFactory, WICBitmapEncoderNoCache,
};
use windows::Win32::Storage::EnhancedStorage::PKEY_AppUserModel_ID;
use windows::Win32::Storage::FileSystem::{
    GetFileVersionInfoSizeW, GetFileVersionInfoW, VerQueryValueW,
};
use windows::Win32::System::Com::StructuredStorage::PropVariantToStringAlloc;
use windows::Win32::System::Com::{
    CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, CoCreateInstance, CoInitializeEx,
    CoTaskMemFree, IPersistFile, STGM_READ,
};
use windows::Win32::UI::Shell::PropertiesSystem::IPropertyStore;
use windows::Win32::UI::Shell::{
    FOLDERID_CommonPrograms, FOLDERID_Programs, IShellItem, IShellItemImageFactory, IShellLinkW,
    KF_FLAG_DEFAULT, SHCreateItemFromParsingName, SHGetKnownFolderPath, SIGDN_NORMALDISPLAY,
    SIIGBF_ICONONLY, ShellLink,
};
use windows::core::{GUID, HSTRING, Interface, PWSTR, w};

use crate::apps::file_stem;
use crate::wake::{Msg, Waker};

/// The virtual host the icon cache is mapped to.
const ICON_SIZE: i32 = 64;

/// Resolves toast app identities using the same shell images and PNG cache as dock icons.
/// Construct on a COM-initialized worker; Start Menu shortcuts are indexed only on a miss.
pub struct AppIconResolver {
    wic: Option<IWICImagingFactory>,
    shortcuts: std::cell::OnceCell<HashMap<String, PathBuf>>,
}

impl AppIconResolver {
    pub fn new() -> Self {
        // SAFETY: COM is initialized by the lane; the factory stays on that thread.
        let wic =
            unsafe { CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER) }.ok();
        Self {
            wic,
            shortcuts: std::cell::OnceCell::new(),
        }
    }

    pub fn icon(&self, aumid: &str, dir: &Path) -> Option<String> {
        if aumid.is_empty() {
            return None;
        }
        let wic = self.wic.as_ref()?;
        let from_path = |path: &str| {
            shell_item(path).and_then(|item| icon(&item, &path.to_lowercase(), wic, dir))
        };
        let shortcuts = self.shortcuts.get_or_init(start_menu_app_ids);
        shortcuts
            .get(&aumid.to_lowercase())
            .and_then(|path| {
                let path = path.to_string_lossy();
                from_path(&path).or_else(|| resolve_link(&path).0.and_then(|exe| from_path(&exe)))
            })
            .or_else(|| from_path(&format!(r"shell:AppsFolder\{aumid}")))
    }
}

/// The real user/common Start Menu folders, also when APPDATA is redirected for a preview.
pub fn start_menu_dirs() -> Vec<PathBuf> {
    [FOLDERID_Programs, FOLDERID_CommonPrograms]
        .iter()
        .filter_map(|id| {
            // SAFETY: the returned string is ours to free.
            unsafe {
                let path = SHGetKnownFolderPath(id, KF_FLAG_DEFAULT, None).ok()?;
                let text = path.to_string().ok();
                CoTaskMemFree(Some(path.0.cast()));
                text.map(PathBuf::from)
            }
        })
        .collect()
}

/// Match System.AppUserModel.ID, never a display-name guess; user shortcuts take precedence.
fn start_menu_app_ids() -> HashMap<String, PathBuf> {
    let mut found = HashMap::new();
    for root in start_menu_dirs() {
        let mut dirs = vec![root];
        while let Some(dir) = dirs.pop() {
            let mut entries: Vec<_> = std::fs::read_dir(dir)
                .into_iter()
                .flatten()
                .flatten()
                .collect();
            entries.sort_by_key(|entry| entry.path());
            for entry in entries {
                let path = entry.path();
                if entry.file_type().is_ok_and(|kind| kind.is_dir()) {
                    dirs.push(path);
                } else if path
                    .extension()
                    .is_some_and(|ext| ext.eq_ignore_ascii_case("lnk"))
                    && let Some(aumid) = resolve_link(&path.to_string_lossy()).1
                {
                    found.entry(aumid.to_lowercase()).or_insert(path);
                }
            }
        }
    }
    found
}

/// Something to name and draw.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Source {
    /// A pinned path: an exe, a `.lnk`, a folder or `shell:AppsFolder\<AUMID>`.
    Path(String),
    /// A running app: through the Start menu's entry for its AUMID when there is one, else its
    /// exe (lowercase).
    App {
        aumid: Option<String>,
        exe: Option<String>,
    },
}

/// What the worker found out.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Described {
    /// What launches it.
    pub path: Option<String>,
    pub name: Option<String>,
    /// The icon's URL.
    pub icon: Option<String>,
    /// For a shortcut or AppsFolder pin: the app it starts, to match running windows.
    pub aumid: Option<String>,
    /// Lowercase.
    pub exe: Option<String>,
}

/// The main thread's side: what's known, what's asked, and the worker.
pub struct Icons {
    worker: Option<Sender<Source>>,
    known: HashMap<Source, Described>,
    pending: HashSet<Source>,
    waker: Waker,
    dir: PathBuf,
}

impl Icons {
    pub fn new(waker: Waker, dir: PathBuf) -> Self {
        Self {
            worker: None,
            known: HashMap::new(),
            pending: HashSet::new(),
            waker,
            dir,
        }
    }

    /// The cache folder used by the native renderer.
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// What's known about `source`. The first time, the worker is asked; the answer arrives as
    /// [`Msg::Described`].
    pub fn get(&mut self, source: &Source) -> Option<&Described> {
        if !self.known.contains_key(source) && self.pending.insert(source.clone()) {
            let (waker, dir) = (self.waker.clone(), self.dir.clone());
            let worker = self.worker.get_or_insert_with(|| spawn(waker, dir));
            let _ = worker.send(source.clone());
        }
        self.known.get(source)
    }

    pub fn on_described(&mut self, source: Source, described: Described) {
        self.pending.remove(&source);
        self.known.insert(source, described);
    }
}

fn spawn(waker: Waker, dir: PathBuf) -> Sender<Source> {
    let (tx, rx) = mpsc::channel::<Source>();
    let spawned = std::thread::Builder::new()
        .name("icons".into())
        .spawn(move || {
            // SAFETY: COM for this thread (shell items want an STA); it lives for the process.
            let _ = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) };
            // SAFETY: plain COM creation.
            let wic: Option<IWICImagingFactory> =
                unsafe { CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER) }
                    .inspect_err(|e| log::warn!("no icons (WIC): {e}"))
                    .ok();
            let _ = std::fs::create_dir_all(&dir);
            for source in rx {
                let described = describe(&source, wic.as_ref(), &dir);
                waker.send(Msg::Described(source, described));
            }
        });
    if let Err(e) = spawned {
        log::warn!("no icon worker: {e}");
    }
    tx
}

fn describe(source: &Source, wic: Option<&IWICImagingFactory>, dir: &Path) -> Described {
    match source {
        Source::Path(path) => describe_path(path, wic, dir),
        Source::App { aumid, exe } => {
            let via_start = aumid.as_ref().and_then(|aumid| {
                let path = format!(r"shell:AppsFolder\{aumid}");
                let item = shell_item(&path)?;
                Some(Described {
                    name: display_name(&item),
                    icon: wic.and_then(|wic| icon(&item, &path, wic, dir)),
                    path: Some(path),
                    aumid: Some(aumid.clone()),
                    exe: exe.clone(),
                })
            });
            via_start
                .or_else(|| exe.as_ref().map(|exe| describe_path(exe, wic, dir)))
                .unwrap_or_default()
        }
    }
}

fn describe_path(path: &str, wic: Option<&IWICImagingFactory>, dir: &Path) -> Described {
    let lower = path.to_lowercase();
    let mut described = Described {
        path: Some(path.to_owned()),
        name: Some(file_stem(path)),
        ..Described::default()
    };
    if let Some(aumid) = lower
        .strip_prefix(r"shell:appsfolder\")
        .map(|_| path[r"shell:AppsFolder\".len()..].to_owned())
    {
        described.aumid = Some(aumid);
    } else if lower.ends_with(".lnk") {
        let (target, aumid) = resolve_link(path);
        described.exe = target.filter(|t| t.ends_with(".exe"));
        described.aumid = aumid;
    } else if lower.ends_with(".exe") {
        described.exe = Some(lower.clone());
        described.name = version_description(path).or(described.name);
    }
    if let Some(item) = shell_item(path) {
        if !lower.ends_with(".exe") {
            described.name = display_name(&item).or(described.name);
        }
        described.icon = wic.and_then(|wic| icon(&item, &lower, wic, dir));
    }
    described
}

pub(crate) fn shell_item(path: &str) -> Option<IShellItem> {
    // SAFETY: a valid string; the item is a new COM object we own.
    unsafe { SHCreateItemFromParsingName(&HSTRING::from(path), None) }.ok()
}

fn display_name(item: &IShellItem) -> Option<String> {
    // SAFETY: COM call; the returned string is ours to free.
    unsafe {
        let name = item.GetDisplayName(SIGDN_NORMALDISPLAY).ok()?;
        let text = name.to_string().ok();
        CoTaskMemFree(Some(name.0.cast()));
        text.filter(|t| !t.is_empty())
    }
}

/// A shortcut's target (lowercase) and its AppUserModelID, if it declares one.
pub(crate) fn resolve_link(path: &str) -> (Option<String>, Option<String>) {
    // SAFETY: COM calls on objects we create; buffers outlive the calls.
    unsafe {
        let Ok(link) = CoCreateInstance::<_, IShellLinkW>(&ShellLink, None, CLSCTX_INPROC_SERVER)
        else {
            return (None, None);
        };
        let loaded = link
            .cast::<IPersistFile>()
            .and_then(|file| file.Load(&HSTRING::from(path), STGM_READ));
        if loaded.is_err() {
            return (None, None);
        }
        let mut buffer = [0u16; 1024];
        let target = link
            .GetPath(&mut buffer, std::ptr::null_mut(), 0)
            .ok()
            .map(|()| {
                let len = buffer.iter().position(|&c| c == 0).unwrap_or(buffer.len());
                String::from_utf16_lossy(&buffer[..len]).to_lowercase()
            })
            .filter(|t| !t.is_empty());
        let aumid = link.cast::<IPropertyStore>().ok().and_then(|store| {
            let value = store.GetValue(&PKEY_AppUserModel_ID).ok()?;
            let text: PWSTR = PropVariantToStringAlloc(&value).ok()?;
            let id = text.to_string().ok();
            CoTaskMemFree(Some(text.0.cast()));
            id.filter(|id| !id.is_empty())
        });
        (target, aumid)
    }
}

/// An exe's `FileDescription` ("Visual Studio Code" for `Code.exe`).
pub fn version_description(path: &str) -> Option<String> {
    let path = HSTRING::from(path);
    // SAFETY: the buffer is as long as the size asked for; VerQueryValueW returns pointers into
    // it, read while it's alive.
    unsafe {
        let size = GetFileVersionInfoSizeW(&path, None);
        if size == 0 {
            return None;
        }
        let mut data = vec![0u8; size as usize];
        GetFileVersionInfoW(&path, None, size, data.as_mut_ptr().cast()).ok()?;
        let mut pointer = std::ptr::null_mut();
        let mut len = 0u32;
        let translation = w!(r"\VarFileInfo\Translation");
        let (language, codepage) =
            if VerQueryValueW(data.as_ptr().cast(), translation, &mut pointer, &mut len).as_bool()
                && len >= 4
            {
                let pair = pointer.cast::<u16>();
                (pair.read_unaligned(), pair.add(1).read_unaligned())
            } else {
                (0x0409, 0x04b0)
            };
        let query = HSTRING::from(format!(
            r"\StringFileInfo\{language:04x}{codepage:04x}\FileDescription"
        ));
        if !VerQueryValueW(data.as_ptr().cast(), &query, &mut pointer, &mut len).as_bool()
            || len == 0
        {
            return None;
        }
        let text = std::slice::from_raw_parts(pointer.cast::<u16>(), len as usize);
        let end = text.iter().position(|&c| c == 0).unwrap_or(text.len());
        let name = String::from_utf16_lossy(&text[..end]).trim().to_owned();
        (!name.is_empty()).then_some(name)
    }
}

/// FNV-1a: a stable file name for a cache key.
pub fn cache_name(key: &str) -> String {
    let hash = key.bytes().fold(0xcbf2_9ce4_8422_2325u64, |hash, byte| {
        (hash ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3)
    });
    format!("{hash:016x}.png")
}

/// Stores ready-made PNG bytes in the cache (named by their content) and returns the URL.
pub fn store_png(dir: &Path, png: &[u8]) -> Option<String> {
    let name = cache_name(&String::from_utf8_lossy(png));
    let file = dir.join(&name);
    if !file.exists() {
        crate::files::write_atomic(&file, png).ok()?;
    }
    Some(file.display().to_string())
}

/// The item's icon as a cached PNG, by URL.
pub(crate) fn icon(
    item: &IShellItem,
    key: &str,
    wic: &IWICImagingFactory,
    dir: &Path,
) -> Option<String> {
    let name = cache_name(key);
    let file = dir.join(&name);
    if file.exists() {
        return Some(file.display().to_string());
    }
    let (width, height, mut pixels) = image(item)?;
    fix_alpha(&mut pixels);
    let temp = dir.join(format!("{name}.tmp"));
    if let Err(e) = encode_png(wic, &temp, width, height, &pixels) {
        log::debug!("icon for {key}: {e}");
        let _ = std::fs::remove_file(&temp);
        return None;
    }
    std::fs::rename(&temp, &file).ok()?;
    Some(file.display().to_string())
}

/// The item's icon as top-down BGRA pixels.
fn image(item: &IShellItem) -> Option<(u32, u32, Vec<u8>)> {
    let size = SIZE {
        cx: ICON_SIZE,
        cy: ICON_SIZE,
    };
    // SAFETY: COM calls and GDI on a bitmap we own (deleted below); the buffer matches the
    // header we pass.
    unsafe {
        let factory: IShellItemImageFactory = item.cast().ok()?;
        let bitmap: HBITMAP = factory.GetImage(size, SIIGBF_ICONONLY).ok()?;
        let mut info = BITMAP::default();
        let read = GetObjectW(
            bitmap.into(),
            size_of::<BITMAP>() as i32,
            Some((&raw mut info).cast()),
        );
        let (width, height) = (info.bmWidth, info.bmHeight.abs());
        let mut pixels = vec![0u8; (width * height * 4).max(0) as usize];
        let mut header = BITMAPINFO {
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
        let dc = CreateCompatibleDC(None);
        let lines = if read > 0 && width > 0 && height > 0 {
            GetDIBits(
                dc,
                bitmap,
                0,
                height as u32,
                Some(pixels.as_mut_ptr().cast()),
                &mut header,
                DIB_RGB_COLORS,
            )
        } else {
            0
        };
        let _ = DeleteDC(dc);
        let _ = DeleteObject(bitmap.into());
        (lines == height).then_some((width as u32, height as u32, pixels))
    }
}

/// The shell hands out premultiplied alpha (or none at all for old icons); PNG wants straight.
fn fix_alpha(bgra: &mut [u8]) {
    let (pixels, _) = bgra.as_chunks_mut::<4>();
    if pixels.iter().all(|p| p[3] == 0) {
        pixels.iter_mut().for_each(|p| p[3] = 255);
        return;
    }
    let premultiplied = pixels
        .iter()
        .all(|p| p[0] <= p[3] && p[1] <= p[3] && p[2] <= p[3]);
    if premultiplied {
        for p in pixels {
            let alpha = u32::from(p[3]);
            if alpha > 0 && alpha < 255 {
                for c in &mut p[..3] {
                    *c = ((u32::from(*c) * 255 + alpha / 2) / alpha).min(255) as u8;
                }
            }
        }
    }
}

fn encode_png(
    wic: &IWICImagingFactory,
    path: &Path,
    width: u32,
    height: u32,
    bgra: &[u8],
) -> windows::core::Result<()> {
    // SAFETY: COM calls on objects we create; the pixel buffer matches width × height × 4.
    unsafe {
        let stream = wic.CreateStream()?;
        stream.InitializeFromFilename(&HSTRING::from(path.as_os_str()), GENERIC_WRITE.0)?;
        let encoder = wic.CreateEncoder(&GUID_ContainerFormatPng, std::ptr::null::<GUID>())?;
        encoder.Initialize(&stream, WICBitmapEncoderNoCache)?;
        let mut frame = None;
        encoder.CreateNewFrame(&mut frame, std::ptr::null_mut())?;
        let frame = frame.ok_or(windows::core::Error::from(
            windows::Win32::Foundation::E_FAIL,
        ))?;
        frame.Initialize(None)?;
        frame.SetSize(width, height)?;
        let mut format = GUID_WICPixelFormat32bppBGRA;
        frame.SetPixelFormat(&mut format)?;
        frame.WritePixels(height, width * 4, bgra)?;
        frame.Commit()?;
        encoder.Commit()?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cache_names_are_stable_and_distinct() {
        assert_eq!(cache_name("a"), cache_name("a"));
        assert_ne!(cache_name(r"c:\a.exe"), cache_name(r"c:\b.exe"));
        assert!(cache_name("x").ends_with(".png"));
        assert_eq!(cache_name("x").len(), 16 + 4);
    }

    #[test]
    fn alpha_becomes_straight_and_opaque_when_missing() {
        // Premultiplied half-transparent white becomes straight white at half alpha.
        let mut pixels = vec![128, 128, 128, 128, 0, 0, 0, 0];
        fix_alpha(&mut pixels);
        assert_eq!(pixels, [255, 255, 255, 128, 0, 0, 0, 0]);
        // No alpha at all: an old 24-bit icon, fully opaque.
        let mut pixels = vec![10, 20, 30, 0];
        fix_alpha(&mut pixels);
        assert_eq!(pixels, [10, 20, 30, 255]);
        // Already straight (a channel above alpha): untouched.
        let mut pixels = vec![200, 10, 10, 100];
        fix_alpha(&mut pixels);
        assert_eq!(pixels, [200, 10, 10, 100]);
    }
}

impl Default for AppIconResolver {
    fn default() -> Self {
        Self::new()
    }
}
