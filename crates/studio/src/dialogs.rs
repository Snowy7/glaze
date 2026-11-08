//! Windows' own open dialog for picking a file or a folder (wallpaper sources, programs, widget
//! pictures). Modal on Studio's window; `None` when cancelled.

use std::path::PathBuf;

use windows::Win32::Foundation::HWND;
use windows::Win32::System::Com::{CLSCTX_INPROC_SERVER, CoCreateInstance};
use windows::Win32::UI::Shell::Common::COMDLG_FILTERSPEC;
use windows::Win32::UI::Shell::{
    FOS_FORCEFILESYSTEM, FOS_PICKFOLDERS, FileOpenDialog, IFileOpenDialog, SIGDN_FILESYSPATH,
};
use windows::core::{HSTRING, PCWSTR};

/// What to pick: files matching `filters` (name, pattern like `*.png;*.jpg`), or a folder.
pub fn pick(hwnd: Option<HWND>, title: &str, folder: bool, filters: &[(&str, &str)]) -> Option<PathBuf> {
    // SAFETY: a COM dialog created, shown modally and released here; the filter strings live
    // until `Show` returns.
    unsafe {
        let dialog: IFileOpenDialog = CoCreateInstance(&FileOpenDialog, None, CLSCTX_INPROC_SERVER).ok()?;
        let mut options = dialog.GetOptions().ok()? | FOS_FORCEFILESYSTEM;
        if folder {
            options |= FOS_PICKFOLDERS;
        }
        dialog.SetOptions(options).ok()?;
        let _ = dialog.SetTitle(&HSTRING::from(title));
        let names: Vec<HSTRING> = filters.iter().map(|(n, _)| HSTRING::from(*n)).collect();
        let specs: Vec<HSTRING> = filters.iter().map(|(_, s)| HSTRING::from(*s)).collect();
        let spec: Vec<COMDLG_FILTERSPEC> = names
            .iter()
            .zip(&specs)
            .map(|(n, s)| COMDLG_FILTERSPEC {
                pszName: PCWSTR(n.as_ptr()),
                pszSpec: PCWSTR(s.as_ptr()),
            })
            .collect();
        if !folder && !spec.is_empty() {
            let _ = dialog.SetFileTypes(&spec);
        }
        dialog.Show(hwnd).ok()?;
        let item = dialog.GetResult().ok()?;
        let path = item.GetDisplayName(SIGDN_FILESYSPATH).ok()?;
        let text = path.to_string().ok();
        windows::Win32::System::Com::CoTaskMemFree(Some(path.0 as *const _));
        text.map(PathBuf::from)
    }
}

/// The wallpaper sources a file can be.
pub const WALLPAPER_FILES: &[(&str, &str)] = &[
    ("Wallpapers", "*.png;*.jpg;*.jpeg;*.bmp;*.gif;*.webp;*.heic;*.mp4;*.mkv;*.webm;*.mov;*.m4v;*.wgsl;*.glb;*.gltf"),
    ("Pictures", "*.png;*.jpg;*.jpeg;*.bmp;*.gif;*.webp;*.heic"),
    ("Videos", "*.mp4;*.mkv;*.webm;*.mov;*.m4v"),
    ("Shaders and 3D scenes", "*.wgsl;*.glb;*.gltf"),
];

pub const PROGRAMS: &[(&str, &str)] = &[("Programs", "*.exe")];
pub const PAGES: &[(&str, &str)] = &[("Web pages", "*.html;*.htm")];
/// What an icon pack can hold (`glaze_bars::icon_pack`).
pub const ICONS: &[(&str, &str)] = &[("Icons and pictures", "*.svg;*.png;*.ico;*.webp;*.jpg;*.jpeg")];
pub const PICTURES: &[(&str, &str)] = &[("Pictures", "*.png;*.jpg;*.jpeg;*.bmp;*.gif;*.webp")];
