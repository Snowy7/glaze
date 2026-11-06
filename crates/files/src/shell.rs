//! Shell names, namespace enumeration and image extraction. All slow shell calls run on STAs
//! owned by the browser's workers, never on the drawing thread.
use crate::model::Entry;
use anyhow::{Context, Result, ensure};
use mypaper_ui::Image;
use std::{path::Path, sync::Arc};
use windows::{
    Win32::{
        Foundation::*,
        Graphics::Gdi::*,
        Storage::FileSystem::*,
        System::{Com::*, Registry::*, SystemServices::*, Time::*},
        UI::{Shell::*, WindowsAndMessaging::*},
    },
    core::{HSTRING, PCWSTR, PWSTR},
};

pub struct Apartment;
impl Apartment {
    pub fn new() -> Result<Self> {
        // SAFETY: each browser worker owns its STA from creation through destruction.
        unsafe {
            CoInitializeEx(None, COINIT_APARTMENTTHREADED).ok()?;
        }
        Ok(Self)
    }
}
impl Drop for Apartment {
    fn drop(&mut self) {
        // SAFETY: this guard balances the successful CoInitializeEx on its creating worker thread.
        unsafe { CoUninitialize() };
    }
}

pub fn item(path: &str) -> Result<IShellItem> {
    // Win32 filesystem calls accept '/', but shell parsing names require Windows separators.
    let path = path.replace('/', "\\");
    // SAFETY: terminated parsing name; caller has initialized COM.
    Ok(unsafe { SHCreateItemFromParsingName(&HSTRING::from(path), None) }?)
}

pub fn text(value: PWSTR) -> Result<String> {
    // SAFETY: shell getters return task-allocated terminated strings, released exactly once.
    let result = unsafe { value.to_string() };
    // SAFETY: the shell allocated this string with the task allocator; it is released exactly once.
    unsafe { CoTaskMemFree(Some(value.0.cast())) };
    Ok(result?)
}

pub fn display(item: &IShellItem, format: SIGDN) -> Result<String> {
    // SAFETY: the live shell item returns a task-allocated string, released by text().
    text(unsafe { item.GetDisplayName(format) }?)
}

#[derive(Debug, Clone)]
pub struct Location {
    pub path: String,
    pub name: String,
    pub parent: Option<String>,
    pub filesystem: bool,
}

pub fn resolve(path: &str, base: Option<&str>) -> Result<Location> {
    let path = path.trim().trim_matches('"');
    let path = if !path.starts_with("shell:")
        && !path.starts_with("::")
        && Path::new(path).is_relative()
    {
        base.filter(|b| Path::new(b).is_absolute())
            .map(|b| Path::new(b).join(path).to_string_lossy().into_owned())
            .unwrap_or_else(|| path.to_owned())
    } else {
        path.to_owned()
    };
    let shell = item(&path)?;
    // SAFETY: the live shell item is queried for attributes on its initialized COM apartment.
    let attrs = unsafe { shell.GetAttributes(SFGAO_FOLDER | SFGAO_FILESYSTEM) }?;
    ensure!(
        attrs.contains(SFGAO_FOLDER),
        "This location is not a folder"
    );
    let filesystem = attrs.contains(SFGAO_FILESYSTEM);
    Ok(Location {
        path: display(
            &shell,
            if filesystem {
                SIGDN_FILESYSPATH
            } else {
                SIGDN_DESKTOPABSOLUTEPARSING
            },
        )?,
        name: display(&shell, SIGDN_NORMALDISPLAY)?,
        // SAFETY: the live shell item returns an owned interface; no borrowed PIDL escapes.
        parent: unsafe { shell.GetParent() }
            .ok()
            .and_then(|p| display(&p, SIGDN_DESKTOPABSOLUTEPARSING).ok()),
        filesystem,
    })
}

/// Explorer's hidden/protected-file preferences. Query once per enumeration, never per row.
pub fn hidden_settings() -> (bool, bool) {
    let read = |name: PCWSTR| {
        let mut value = 0u32;
        let mut size = 4;
        // SAFETY: read-only DWORD query into a correctly sized local.
        unsafe {
            RegGetValueW(
                HKEY_CURRENT_USER,
                windows::core::w!(r"Software\Microsoft\Windows\CurrentVersion\Explorer\Advanced"),
                name,
                RRF_RT_REG_DWORD,
                None,
                Some((&raw mut value).cast()),
                Some(&mut size),
            )
        }
        .is_ok()
        .then_some(value)
    };
    (
        read(windows::core::w!("Hidden")) == Some(1),
        read(windows::core::w!("ShowSuperHidden")) == Some(1),
    )
}

fn from_find(parent: &str, data: &WIN32_FIND_DATAW) -> Entry {
    let end = data
        .cFileName
        .iter()
        .position(|c| *c == 0)
        .unwrap_or(data.cFileName.len());
    let name = String::from_utf16_lossy(&data.cFileName[..end]);
    let directory = data.dwFileAttributes & FILE_ATTRIBUTE_DIRECTORY.0 != 0;
    let kind = if directory {
        "Folder".into()
    } else {
        Path::new(&name)
            .extension()
            .and_then(|s| s.to_str())
            .map(|s| format!("{} file", s.to_uppercase()))
            .unwrap_or_else(|| "File".into())
    };
    Entry {
        path: Path::new(parent).join(&name).to_string_lossy().into_owned(),
        name,
        directory,
        size: (!directory)
            .then_some((u64::from(data.nFileSizeHigh) << 32) | u64::from(data.nFileSizeLow)),
        modified: (u64::from(data.ftLastWriteTime.dwHighDateTime) << 32)
            | u64::from(data.ftLastWriteTime.dwLowDateTime),
        kind,
        hidden: data.dwFileAttributes & FILE_ATTRIBUTE_HIDDEN.0 != 0,
    }
}

struct Find(HANDLE);
impl Drop for Find {
    fn drop(&mut self) {
        // SAFETY: this guard owns a successful find handle and closes it once.
        let _ = unsafe { FindClose(self.0) };
    }
}

/// First page is delivered immediately; the rest is collected without opening each child.
pub fn enumerate(
    location: &Location,
    cancelled: impl Fn() -> bool,
    first: impl Fn(&[Entry]),
) -> Result<Vec<Entry>> {
    let (hidden, protected) = hidden_settings();
    let mut entries = Vec::new();
    if location.filesystem {
        let pattern = Path::new(&location.path).join("*");
        let mut data = WIN32_FIND_DATAW::default();
        // SAFETY: valid search pattern and writable find-data, handle closed by Find.
        let found = unsafe {
            FindFirstFileExW(
                &HSTRING::from(pattern.as_os_str()),
                FindExInfoBasic,
                (&raw mut data).cast(),
                FindExSearchNameMatch,
                None,
                FIND_FIRST_EX_LARGE_FETCH,
            )
        };
        let found = match found {
            Ok(found) => Find(found),
            Err(error) if error.code() == ERROR_FILE_NOT_FOUND.to_hresult() => return Ok(entries),
            Err(error) => return Err(error.into()),
        };
        loop {
            if cancelled() {
                return Ok(Vec::new());
            }
            let attr = data.dwFileAttributes;
            if (hidden || attr & FILE_ATTRIBUTE_HIDDEN.0 == 0)
                && (protected || attr & FILE_ATTRIBUTE_SYSTEM.0 == 0)
            {
                let entry = from_find(&location.path, &data);
                if entry.name != "." && entry.name != ".." {
                    entries.push(entry);
                    if entries.len() == 128 {
                        first(&entries);
                    }
                }
            }
            // SAFETY: the find guard is live and data is a correctly sized writable WIN32_FIND_DATAW.
            if let Err(error) = unsafe { FindNextFileW(found.0, &mut data) } {
                if error.code() != ERROR_NO_MORE_FILES.to_hresult() {
                    return Err(error.into());
                }
                break;
            }
        }
    } else {
        let folder = item(&location.path)?;
        // SAFETY: the live namespace item is bound on the enumerating worker STA.
        let enumerator: IEnumShellItems = unsafe { folder.BindToHandler(None, &BHID_EnumItems) }?;
        loop {
            if cancelled() {
                return Ok(Vec::new());
            }
            let mut chunk = [None];
            let mut fetched = 0;
            // SAFETY: the one-element output array and fetched-count local stay writable through the call.
            unsafe { enumerator.Next(&mut chunk, Some(&mut fetched)) }?;
            if fetched == 0 {
                break;
            }
            let shell = chunk[0].take().context("empty shell item")?;
            // SAFETY: this live enumerated item is queried on the same worker STA.
            let attrs = unsafe { shell.GetAttributes(SFGAO_FOLDER | SFGAO_HIDDEN | SFGAO_SYSTEM) }
                .unwrap_or_default();
            if (!hidden && attrs.contains(SFGAO_HIDDEN))
                || (!protected && attrs.contains(SFGAO_SYSTEM))
            {
                continue;
            }
            entries.push(Entry {
                path: display(&shell, SIGDN_DESKTOPABSOLUTEPARSING)?,
                name: display(&shell, SIGDN_NORMALDISPLAY)?,
                directory: attrs.contains(SFGAO_FOLDER),
                size: None,
                modified: 0,
                kind: if attrs.contains(SFGAO_FOLDER) {
                    "Folder"
                } else {
                    "Shell item"
                }
                .into(),
                hidden: attrs.contains(SFGAO_HIDDEN),
            });
            if entries.len() == 128 {
                first(&entries);
            }
        }
    }
    Ok(entries)
}

#[derive(Debug, Clone)]
pub struct Decoration {
    pub name: String,
    pub kind: String,
    pub image: Option<Arc<Image>>,
}

pub fn decorate(path: &str, size: i32, thumbnail: bool) -> Result<Decoration> {
    let shell = item(path)?;
    let name = display(&shell, SIGDN_NORMALDISPLAY)?;
    let mut info = SHFILEINFOW::default();
    // SAFETY: only name/type fields are requested, no HICON to leak.
    unsafe {
        SHGetFileInfoW(
            &HSTRING::from(path),
            FILE_FLAGS_AND_ATTRIBUTES(0),
            Some(&mut info),
            size_of::<SHFILEINFOW>() as u32,
            SHGFI_TYPENAME,
        );
    }
    let end = info
        .szTypeName
        .iter()
        .position(|c| *c == 0)
        .unwrap_or(info.szTypeName.len());
    let kind = String::from_utf16_lossy(&info.szTypeName[..end]);
    let image = (|| -> Result<Image> {
        let factory: IShellItemImageFactory = windows::core::Interface::cast(&shell)?;
        // SAFETY: the live image factory returns an owned HBITMAP for the requested bounded size.
        let bitmap = unsafe {
            factory.GetImage(
                SIZE { cx: size, cy: size },
                if thumbnail {
                    SIIGBF_RESIZETOFIT
                } else {
                    SIIGBF_ICONONLY
                },
            )
        }?;
        let result = bitmap_image(bitmap);
        // SAFETY: the bitmap returned by GetImage is owned here and deleted after its pixels are copied.
        unsafe {
            let _ = DeleteObject(bitmap.into());
        }
        result
    })()
    .ok()
    .map(Arc::new);
    Ok(Decoration { name, kind, image })
}

fn bitmap_image(bitmap: HBITMAP) -> Result<Image> {
    let mut object = BITMAP::default();
    // SAFETY: bitmap from the shell and correctly sized BITMAP output.
    unsafe {
        GetObjectW(
            bitmap.into(),
            size_of::<BITMAP>() as i32,
            Some((&raw mut object).cast()),
        );
    }
    let (width, height) = (
        object.bmWidth.unsigned_abs(),
        object.bmHeight.unsigned_abs(),
    );
    ensure!(
        width > 0 && height > 0 && width <= 1024 && height <= 1024,
        "invalid thumbnail size"
    );
    let mut pixels = vec![0; width as usize * height as usize * 4];
    let mut info = BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: width as i32,
            biHeight: -(height as i32),
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB.0,
            ..Default::default()
        },
        ..Default::default()
    };
    // SAFETY: the valid shell bitmap, matching BGRA buffer and BITMAPINFO outlive the GetDIBits call.
    unsafe {
        let dc = GetDC(None);
        let lines = GetDIBits(
            dc,
            bitmap,
            0,
            height,
            Some(pixels.as_mut_ptr().cast()),
            &mut info,
            DIB_RGB_COLORS,
        );
        ReleaseDC(None, dc);
        ensure!(lines != 0, "reading shell thumbnail");
    }
    // Opaque thumbnails sometimes carry no alpha at all; shell icons already use premultiplied BGRA.
    if pixels.as_chunks::<4>().0.iter().all(|p| p[3] == 0) {
        for p in pixels.as_chunks_mut::<4>().0 {
            p[3] = 255;
        }
    }
    Ok(Image {
        width,
        height,
        pixels,
    })
}

pub fn modified_label(ticks: u64) -> String {
    if ticks == 0 {
        return String::new();
    }
    let file = FILETIME {
        dwLowDateTime: ticks as u32,
        dwHighDateTime: (ticks >> 32) as u32,
    };
    let mut local = FILETIME::default();
    let mut time = SYSTEMTIME::default();
    // SAFETY: both FILETIME and SYSTEMTIME outputs are correctly sized writable locals.
    unsafe {
        if FileTimeToLocalFileTime(&file, &mut local).is_err()
            || FileTimeToSystemTime(&local, &mut time).is_err()
        {
            return String::new();
        }
    }
    format!(
        "{:04}-{:02}-{:02}  {:02}:{:02}",
        time.wYear, time.wMonth, time.wDay, time.wHour, time.wMinute
    )
}

pub fn known_folder(id: &windows::core::GUID) -> Option<String> {
    // SAFETY: the known-folder API returns a task-allocated string released by text().
    text(unsafe { SHGetKnownFolderPath(id, KF_FLAG_DEFAULT, None) }.ok()?).ok()
}

pub fn drives() -> Vec<String> {
    // SAFETY: GetLogicalDrives is a read-only query with no pointer parameters.
    let mask = unsafe { GetLogicalDrives() };
    (0..26)
        .filter(|n| mask & (1 << n) != 0)
        .map(|n| format!("{}:\\", char::from(b'A' + n)))
        .collect()
}

pub fn open(path: &str) -> Result<()> {
    // SAFETY: all terminated strings outlive the synchronous ShellExecuteW call.
    let result = unsafe {
        ShellExecuteW(
            None,
            windows::core::w!("open"),
            &HSTRING::from(path),
            PCWSTR::null(),
            PCWSTR::null(),
            SW_SHOWNORMAL,
        )
    };
    ensure!(
        result.0 as isize > 32,
        "Windows could not open this item ({})",
        result.0 as isize
    );
    Ok(())
}

pub fn terminal(path: &str) -> Result<()> {
    use std::os::windows::process::CommandExt;
    // Pass the directory as a distinct argument; no shell command is constructed from a path.
    std::process::Command::new("wt.exe")
        .args(["-d", path])
        .creation_flags(0x08000000)
        .spawn()?;
    Ok(())
}

#[derive(Debug, Clone)]
pub enum Operation {
    Copy {
        paths: Vec<String>,
        destination: String,
        moving: bool,
    },
    Delete {
        paths: Vec<String>,
        permanent: bool,
    },
    Rename {
        path: String,
        name: String,
    },
    NewFolder {
        destination: String,
        name: String,
    },
}

/// Windows owns progress, elevation, conflict handling, recycle semantics and the undo record.
pub fn operate(operation: Operation, owner: HWND) -> Result<()> {
    // SAFETY: this STA owns each COM interface; shell items and terminated names remain live until operations finish.
    unsafe {
        let file: IFileOperation = CoCreateInstance(&FileOperation, None, CLSCTX_ALL)?;
        file.SetOwnerWindow(owner)
            .context("setting operation owner")?;
        let permanent = matches!(
            &operation,
            Operation::Delete {
                permanent: true,
                ..
            }
        );
        let flags = if permanent {
            FILEOPERATION_FLAGS(0)
        } else {
            FOF_ALLOWUNDO | FOFX_ADDUNDORECORD | FOFX_RECYCLEONDELETE
        };
        file.SetOperationFlags(flags)
            .context("setting operation flags")?;
        match operation {
            Operation::Copy {
                paths,
                destination,
                moving,
            } => {
                let to = item(&destination)?;
                for path in paths {
                    let from = item(&path)?;
                    if moving {
                        file.MoveItem(&from, &to, PCWSTR::null(), None)?;
                    } else {
                        file.CopyItem(&from, &to, PCWSTR::null(), None)
                            .context("queuing copy")?;
                    }
                }
            }
            Operation::Delete { paths, .. } => {
                for path in paths {
                    file.DeleteItem(&item(&path)?, None)?;
                }
            }
            Operation::Rename { path, name } => {
                validate_name(&name)?;
                file.RenameItem(&item(&path)?, &HSTRING::from(name), None)?;
            }
            Operation::NewFolder { destination, name } => {
                validate_name(&name)?;
                file.NewItem(
                    &item(&destination)?,
                    FILE_ATTRIBUTE_DIRECTORY.0,
                    &HSTRING::from(name),
                    PCWSTR::null(),
                    None,
                )?;
            }
        }
        file.PerformOperations()
            .context("performing file operation")?;
        ensure!(
            !file.GetAnyOperationsAborted()?.as_bool(),
            "Operation cancelled"
        );
    }
    Ok(())
}

pub fn validate_name(name: &str) -> Result<()> {
    ensure!(
        !name.is_empty()
            && name != "."
            && name != ".."
            && !name.ends_with(['.', ' '])
            && !name.chars().any(|c| c < ' ' || "<>:\"/\\|?*".contains(c)),
        "Choose a valid file name"
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shell_locations_resolve_and_enumerate_without_filesystem_paths() {
        let _apartment = Apartment::new().unwrap();
        let location = resolve("shell:MyComputerFolder", None).unwrap();
        assert!(!location.filesystem);
        let entries = enumerate(&location, || false, |_| {}).unwrap();
        assert!(
            entries.iter().any(|entry| entry.path.starts_with("C:")),
            "{entries:?}"
        );
    }

    #[test]
    fn enumeration_names_and_shell_operations_round_trip_in_a_private_scratch_folder() {
        let _apartment = Apartment::new().unwrap();
        let root = Path::new(r"C:\mypaper-targets\files\scratch")
            .join(format!("operations-{}", std::process::id()));
        assert!(root.starts_with(r"C:\mypaper-targets\files\scratch"));
        std::fs::create_dir_all(&root).unwrap();
        let path = |name: &str| root.join(name).to_string_lossy().into_owned();
        std::fs::write(path("source.txt"), b"file operation fixture").unwrap();
        std::fs::create_dir_all(path("destination")).unwrap();
        let location = resolve(&root.to_string_lossy(), None).unwrap();
        let entries = enumerate(&location, || false, |_| {}).unwrap();
        assert_eq!(entries.len(), 2);
        let image = decorate(&path("source.txt"), 20, false).unwrap();
        assert!(!image.name.is_empty() && image.image.is_some());
        operate(
            Operation::Copy {
                paths: vec![path("source.txt")],
                destination: path("destination"),
                moving: false,
            },
            HWND::default(),
        )
        .unwrap();
        assert_eq!(
            std::fs::read(root.join("destination/source.txt")).unwrap(),
            b"file operation fixture"
        );
        operate(
            Operation::Rename {
                path: path("destination/source.txt"),
                name: "renamed.txt".into(),
            },
            HWND::default(),
        )
        .unwrap();
        assert!(root.join("destination/renamed.txt").exists());
        operate(
            Operation::NewFolder {
                destination: path("destination"),
                name: "Nested".into(),
            },
            HWND::default(),
        )
        .unwrap();
        operate(
            Operation::Copy {
                paths: vec![path("destination/renamed.txt")],
                destination: path("destination/Nested"),
                moving: true,
            },
            HWND::default(),
        )
        .unwrap();
        assert!(!root.join("destination/renamed.txt").exists());
        assert!(root.join("destination/Nested/renamed.txt").exists());
        operate(
            Operation::Delete {
                paths: vec![path("destination/Nested/renamed.txt")],
                permanent: false,
            },
            HWND::default(),
        )
        .unwrap();
        assert!(!root.join("destination/Nested/renamed.txt").exists());
        // No recursive cleanup of a computed user path: this test owns this exact fixture tree.
        std::fs::remove_dir(root.join("destination/Nested")).unwrap();
        std::fs::remove_dir(root.join("destination")).unwrap();
        std::fs::remove_file(root.join("source.txt")).unwrap();
        std::fs::remove_dir(root).unwrap();
    }

    #[test]
    fn invalid_names_cannot_turn_a_rename_into_a_path_operation() {
        for name in [
            "",
            ".",
            "..",
            "../escape",
            "a\\b",
            "a:b",
            "a\0b",
            "trailing.",
        ] {
            assert!(validate_name(name).is_err(), "{name:?}");
        }
        assert!(validate_name("資料 — notes.txt").is_ok());
    }
}
