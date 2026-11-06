//! Shell names, namespace enumeration and image extraction. All slow shell calls run on STAs
//! owned by the browser's workers, never on the drawing thread.
use std::{path::{Path, PathBuf}, sync::Arc};
use anyhow::{Context, Result, ensure};
use mypaper_ui::Image;
use windows::{core::{HSTRING, PCWSTR, PWSTR}, Win32::{
    Foundation::*, Graphics::Gdi::*, Storage::FileSystem::*,
    System::{Com::*, Registry::*, SystemServices::*, Time::*},
    UI::{Shell::*, WindowsAndMessaging::*},
}};
use crate::model::Entry;

pub struct Apartment;
impl Apartment {
    pub fn new() -> Result<Self> {
        // SAFETY: each browser worker owns its STA from creation through destruction.
        unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED).ok()?; }
        Ok(Self)
    }
}
impl Drop for Apartment {
    fn drop(&mut self) { unsafe { CoUninitialize() }; }
}

pub fn item(path: &str) -> Result<IShellItem> {
    // SAFETY: terminated parsing name; caller has initialized COM.
    Ok(unsafe { SHCreateItemFromParsingName(&HSTRING::from(path), None) }?)
}

pub fn text(value: PWSTR) -> Result<String> {
    // SAFETY: shell getters return task-allocated terminated strings, released exactly once.
    let result = unsafe { value.to_string() };
    unsafe { CoTaskMemFree(Some(value.0.cast())) };
    Ok(result?)
}

pub fn display(item: &IShellItem, format: SIGDN) -> Result<String> {
    text(unsafe { item.GetDisplayName(format) }?)
}

#[derive(Debug, Clone)]
pub struct Location { pub path: String, pub name: String, pub parent: Option<String>, pub filesystem: bool }

pub fn resolve(path: &str, base: Option<&str>) -> Result<Location> {
    let path = path.trim().trim_matches('"');
    let path = if !path.starts_with("shell:") && !path.starts_with("::") && Path::new(path).is_relative() {
        base.filter(|b| Path::new(b).is_absolute()).map(|b| Path::new(b).join(path).to_string_lossy().into_owned())
            .unwrap_or_else(|| path.to_owned())
    } else { path.to_owned() };
    let shell = item(&path)?;
    let attrs = unsafe { shell.GetAttributes(SFGAO_FOLDER | SFGAO_FILESYSTEM) }?;
    ensure!(attrs.contains(SFGAO_FOLDER), "This location is not a folder");
    let filesystem = attrs.contains(SFGAO_FILESYSTEM);
    Ok(Location {
        path: display(&shell, if filesystem { SIGDN_FILESYSPATH } else { SIGDN_DESKTOPABSOLUTEPARSING })?,
        name: display(&shell, SIGDN_NORMALDISPLAY)?,
        parent: unsafe { shell.GetParent() }.ok().and_then(|p| display(&p, SIGDN_DESKTOPABSOLUTEPARSING).ok()),
        filesystem,
    })
}

/// Explorer's hidden/protected-file preferences. Query once per enumeration, never per row.
pub fn hidden_settings() -> (bool, bool) {
    let read = |name: PCWSTR| {
        let mut value = 0u32;
        let mut size = 4;
        // SAFETY: read-only DWORD query into a correctly sized local.
        unsafe { RegGetValueW(HKEY_CURRENT_USER, windows::core::w!(r"Software\Microsoft\Windows\CurrentVersion\Explorer\Advanced"), name,
            RRF_RT_REG_DWORD, None, Some((&raw mut value).cast()), Some(&mut size)) }.is_ok().then_some(value)
    };
    (read(windows::core::w!("Hidden")) == Some(1), read(windows::core::w!("ShowSuperHidden")) == Some(1))
}

fn from_find(parent: &str, data: &WIN32_FIND_DATAW) -> Entry {
    let end = data.cFileName.iter().position(|c| *c == 0).unwrap_or(data.cFileName.len());
    let name = String::from_utf16_lossy(&data.cFileName[..end]);
    let directory = data.dwFileAttributes & FILE_ATTRIBUTE_DIRECTORY.0 != 0;
    let kind = if directory { "Folder".into() } else {
        Path::new(&name).extension().and_then(|s| s.to_str()).map(|s| format!("{} file", s.to_uppercase())).unwrap_or_else(|| "File".into())
    };
    Entry {
        path: Path::new(parent).join(&name).to_string_lossy().into_owned(), name, directory,
        size: (!directory).then_some((u64::from(data.nFileSizeHigh) << 32) | u64::from(data.nFileSizeLow)),
        modified: (u64::from(data.ftLastWriteTime.dwHighDateTime) << 32) | u64::from(data.ftLastWriteTime.dwLowDateTime),
        kind, hidden: data.dwFileAttributes & FILE_ATTRIBUTE_HIDDEN.0 != 0,
    }
}

struct Find(HANDLE);
impl Drop for Find { fn drop(&mut self) { let _ = unsafe { FindClose(self.0) }; } }

/// First page is delivered immediately; the rest is collected without opening each child.
pub fn enumerate(location: &Location, cancelled: impl Fn() -> bool, first: impl Fn(&[Entry])) -> Result<Vec<Entry>> {
    let (hidden, protected) = hidden_settings();
    let mut entries = Vec::new();
    if location.filesystem {
        let pattern = Path::new(&location.path).join("*");
        let mut data = WIN32_FIND_DATAW::default();
        // SAFETY: valid search pattern and writable find-data, handle closed by Find.
        let found = unsafe { FindFirstFileExW(&HSTRING::from(pattern.as_os_str()), FindExInfoBasic,
            (&raw mut data).cast(), FindExSearchNameMatch, None, FIND_FIRST_EX_LARGE_FETCH) };
        let found = match found {
            Ok(found) => Find(found),
            Err(error) if error.code() == ERROR_FILE_NOT_FOUND.to_hresult() => return Ok(entries),
            Err(error) => return Err(error.into()),
        };
        loop {
            if cancelled() { return Ok(Vec::new()); }
            let attr = data.dwFileAttributes;
            if (hidden || attr & FILE_ATTRIBUTE_HIDDEN.0 == 0) && (protected || attr & FILE_ATTRIBUTE_SYSTEM.0 == 0) {
                let entry = from_find(&location.path, &data);
                if entry.name != "." && entry.name != ".." {
                    entries.push(entry);
                    if entries.len() == 128 { first(&entries); }
                }
            }
            if let Err(error) = unsafe { FindNextFileW(found.0, &mut data) } {
                if error.code() != ERROR_NO_MORE_FILES.to_hresult() { return Err(error.into()); }
                break;
            }
        }
    } else {
        let folder = item(&location.path)?;
        let enumerator: IEnumShellItems = unsafe { folder.BindToHandler(None, &BHID_EnumItems) }?;
        loop {
            if cancelled() { return Ok(Vec::new()); }
            let mut chunk = [None];
            let mut fetched = 0;
            unsafe { enumerator.Next(&mut chunk, Some(&mut fetched)) }?;
            if fetched == 0 { break; }
            let shell = chunk[0].take().context("empty shell item")?;
            let attrs = unsafe { shell.GetAttributes(SFGAO_FOLDER | SFGAO_HIDDEN | SFGAO_SYSTEM) }.unwrap_or_default();
            if (!hidden && attrs.contains(SFGAO_HIDDEN)) || (!protected && attrs.contains(SFGAO_SYSTEM)) { continue; }
            entries.push(Entry {
                path: display(&shell, SIGDN_DESKTOPABSOLUTEPARSING)?, name: display(&shell, SIGDN_NORMALDISPLAY)?,
                directory: attrs.contains(SFGAO_FOLDER), size: None, modified: 0,
                kind: if attrs.contains(SFGAO_FOLDER) { "Folder" } else { "Shell item" }.into(), hidden: attrs.contains(SFGAO_HIDDEN),
            });
            if entries.len() == 128 { first(&entries); }
        }
    }
    Ok(entries)
}

#[derive(Debug, Clone)]
pub struct Decoration { pub name: String, pub kind: String, pub image: Option<Arc<Image>> }

pub fn decorate(path: &str, size: i32, thumbnail: bool) -> Result<Decoration> {
    let shell = item(path)?;
    let name = display(&shell, SIGDN_NORMALDISPLAY)?;
    let mut info = SHFILEINFOW::default();
    // SAFETY: only name/type fields are requested, no HICON to leak.
    unsafe { SHGetFileInfoW(&HSTRING::from(path), FILE_FLAGS_AND_ATTRIBUTES(0), Some(&mut info), size_of::<SHFILEINFOW>() as u32, SHGFI_TYPENAME); }
    let end = info.szTypeName.iter().position(|c| *c == 0).unwrap_or(info.szTypeName.len());
    let kind = String::from_utf16_lossy(&info.szTypeName[..end]);
    let image = (|| -> Result<Image> {
        let factory: IShellItemImageFactory = windows::core::Interface::cast(&shell)?;
        let bitmap = unsafe { factory.GetImage(SIZE { cx: size, cy: size }, if thumbnail { SIIGBF_RESIZETOFIT | SIIGBF_BIGGERSIZEOK } else { SIIGBF_ICONONLY }) }?;
        let result = bitmap_image(bitmap);
        unsafe { let _ = DeleteObject(bitmap.into()); }
        result
    })().ok().map(Arc::new);
    Ok(Decoration { name, kind, image })
}

fn bitmap_image(bitmap: HBITMAP) -> Result<Image> {
    let mut object = BITMAP::default();
    // SAFETY: bitmap from the shell and correctly sized BITMAP output.
    unsafe { GetObjectW(bitmap.into(), size_of::<BITMAP>() as i32, Some((&raw mut object).cast())); }
    let (width, height) = (object.bmWidth.unsigned_abs(), object.bmHeight.unsigned_abs());
    ensure!(width > 0 && height > 0 && width <= 1024 && height <= 1024, "invalid thumbnail size");
    let mut pixels = vec![0; width as usize * height as usize * 4];
    let mut info = BITMAPINFO { bmiHeader: BITMAPINFOHEADER {
        biSize: size_of::<BITMAPINFOHEADER>() as u32, biWidth: width as i32, biHeight: -(height as i32),
        biPlanes: 1, biBitCount: 32, biCompression: BI_RGB.0, ..Default::default()
    }, ..Default::default() };
    unsafe {
        let dc = GetDC(None);
        let lines = GetDIBits(dc, bitmap, 0, height, Some(pixels.as_mut_ptr().cast()), &mut info, DIB_RGB_COLORS);
        ReleaseDC(None, dc);
        ensure!(lines != 0, "reading shell thumbnail");
    }
    // Opaque thumbnails sometimes carry no alpha at all; shell icons already use premultiplied BGRA.
    if pixels.chunks_exact(4).all(|p| p[3] == 0) { for p in pixels.chunks_exact_mut(4) { p[3] = 255; } }
    Ok(Image { width, height, pixels })
}

pub fn modified_label(ticks: u64) -> String {
    if ticks == 0 { return String::new(); }
    let file = FILETIME { dwLowDateTime: ticks as u32, dwHighDateTime: (ticks >> 32) as u32 };
    let mut local = FILETIME::default();
    let mut time = SYSTEMTIME::default();
    unsafe {
        if FileTimeToLocalFileTime(&file, &mut local).is_err() || FileTimeToSystemTime(&local, &mut time).is_err() { return String::new(); }
    }
    format!("{:04}-{:02}-{:02}  {:02}:{:02}", time.wYear, time.wMonth, time.wDay, time.wHour, time.wMinute)
}

pub fn known_folder(id: &windows::core::GUID) -> Option<String> {
    text(unsafe { SHGetKnownFolderPath(id, KF_FLAG_DEFAULT, None) }.ok()?).ok()
}

pub fn drives() -> Vec<String> {
    let mask = unsafe { GetLogicalDrives() };
    (0..26).filter(|n| mask & (1 << n) != 0).map(|n| format!("{}:\\", char::from(b'A' + n))).collect()
}

pub fn open(path: &str) -> Result<()> {
    let result = unsafe { ShellExecuteW(None, windows::core::w!("open"), &HSTRING::from(path), PCWSTR::null(), PCWSTR::null(), SW_SHOWNORMAL) };
    ensure!(result.0 as isize > 32, "Windows could not open this item ({})", result.0 as isize);
    Ok(())
}

#[derive(Debug, Clone)]
pub enum Operation {
    Copy { paths: Vec<String>, destination: String, moving: bool },
    Delete { paths: Vec<String>, permanent: bool },
    Rename { path: String, name: String },
    NewFolder { destination: String, name: String },
}

/// Windows owns progress, elevation, conflict handling, recycle semantics and the undo record.
pub fn operate(operation: Operation, owner: HWND) -> Result<()> {
    unsafe {
        let file: IFileOperation = CoCreateInstance(&FileOperation, None, CLSCTX_ALL)?;
        file.SetOwnerWindow(owner)?;
        let permanent = matches!(&operation, Operation::Delete { permanent: true, .. });
        let flags = if permanent { FILEOPERATION_FLAGS(0) } else { FOF_ALLOWUNDO | FOFX_ADDUNDORECORD | FOFX_RECYCLEONDELETE };
        file.SetOperationFlags(flags)?;
        match operation {
            Operation::Copy { paths, destination, moving } => {
                let to = item(&destination)?;
                for path in paths {
                    let from = item(&path)?;
                    if moving { file.MoveItem(&from, &to, PCWSTR::null(), None)?; }
                    else { file.CopyItem(&from, &to, PCWSTR::null(), None)?; }
                }
            }
            Operation::Delete { paths, .. } => for path in paths { file.DeleteItem(&item(&path)?, None)?; },
            Operation::Rename { path, name } => { validate_name(&name)?; file.RenameItem(&item(&path)?, &HSTRING::from(name), None)?; }
            Operation::NewFolder { destination, name } => { validate_name(&name)?; file.NewItem(&item(&destination)?, FILE_ATTRIBUTE_DIRECTORY.0, &HSTRING::from(name), PCWSTR::null(), None)?; }
        }
        file.PerformOperations()?;
        ensure!(!file.GetAnyOperationsAborted()?.as_bool(), "Operation cancelled");
    }
    Ok(())
}

pub fn validate_name(name: &str) -> Result<()> {
    ensure!(!name.is_empty() && name != "." && name != ".." && !name.ends_with(['.', ' ']) &&
        !name.chars().any(|c| c < ' ' || "<>:\"/\\|?*".contains(c)), "Choose a valid file name");
    Ok(())
}

pub fn executable() -> Result<PathBuf> { Ok(std::env::current_exe()?) }
