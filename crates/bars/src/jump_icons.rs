//! Session-only, DPI-sized shell icon cache. All calls run on the jump-list worker STA.
use super::{Launch, Section};
use std::{
    collections::HashMap,
    sync::{Arc, Mutex, OnceLock},
};
use windows::{
    Win32::{
        Graphics::Gdi::*,
        System::Com::{CLSCTX_INPROC_SERVER, CoCreateInstance, CoTaskMemFree, IPersistStream},
        UI::{Shell::*, WindowsAndMessaging::*},
    },
    core::{HSTRING, Interface},
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Icon {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u8>,
}
type Cache = HashMap<(String, u32), Option<Arc<Icon>>>;
static CACHE: OnceLock<Mutex<Cache>> = OnceLock::new();

/// Gets the shell icon at a physical pixel size; callers must initialize COM on a worker.
pub fn icon(launch: &Launch, size: u32) -> Option<Arc<Icon>> {
    let size = size.clamp(16, 64);
    let cache = CACHE.get_or_init(Mutex::default);
    let key = (cache_path(launch)?, size);
    if let Some(found) = cache.lock().ok()?.get(&key).cloned() {
        return found;
    }
    let found = resolve(launch, size).map(Arc::new);
    if let Ok(mut cache) = cache.lock() {
        if cache.len() >= 256 {
            cache.clear();
        }
        cache.insert(key, found.clone());
    }
    found
}

fn cache_path(launch: &Launch) -> Option<String> {
    match launch {
        Launch::Item(path) => Some(format!("item:{}",path.to_lowercase())),
        Launch::Link(bytes) => {
            // SAFETY: decode only our ShellLink object, never a class selected by the file.
            unsafe {
                let stream=SHCreateMemStream(Some(bytes))?;
                let link:IShellLinkW=CoCreateInstance(&ShellLink,None,CLSCTX_INPROC_SERVER).ok()?;
                link.cast::<IPersistStream>().ok()?.Load(&stream).ok()?;
                let mut path=[0u16;32768];let mut index=0;
                if link.GetIconLocation(&mut path,&mut index).is_ok() {
                    let n=path.iter().position(|c|*c==0).unwrap_or(path.len());
                    if n>0 {return Some(format!("resource:{}:{index}",String::from_utf16_lossy(&path[..n]).to_lowercase()));}
                }
                if link.GetPath(&mut path,std::ptr::null_mut(),0).is_ok() {
                    let n=path.iter().position(|c|*c==0).unwrap_or(path.len());
                    if n>0 {return Some(format!("item:{}",String::from_utf16_lossy(&path[..n]).to_lowercase()));}
                }
                let pidl=link.GetIDList().ok()?;
                let path=SHGetNameFromIDList(pidl,SIGDN_DESKTOPABSOLUTEPARSING).ok().and_then(super::owned_string);
                CoTaskMemFree(Some(pidl.cast()));
                path.map(|p|format!("item:{}",p.to_lowercase()))
            }
        }
    }
}

/// Enrich an already-delivered list, then send a second in-place menu update.
pub fn load_icons(sections: &mut [Section], app: &str, size: u32) -> Option<Arc<Icon>> {
    let _com = glaze_desktop::desktop_view::Com::init();
    let app_icon = icon(&Launch::Item(app.into()), size);
    for item in sections
        .iter_mut()
        .flat_map(|s| s.items.iter_mut())
        .flatten()
    {
        item.icon = icon(&item.launch, size).or_else(|| app_icon.clone());
    }
    app_icon
}

fn resolve(launch: &Launch, size: u32) -> Option<Icon> {
    // SAFETY: only ShellLink is instantiated; pointers are owned and freed after copying.
    unsafe {
        let item: IShellItem = match launch {
            Launch::Item(path) => SHCreateItemFromParsingName(&HSTRING::from(path), None).ok()?,
            Launch::Link(bytes) => {
                let stream = SHCreateMemStream(Some(bytes))?;
                let link: IShellLinkW =
                    CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER).ok()?;
                link.cast::<IPersistStream>().ok()?.Load(&stream).ok()?;
                let mut path = [0u16; 32768];
                let mut index = 0;
                if link.GetIconLocation(&mut path, &mut index).is_ok() {
                    let n = path.iter().position(|c| *c == 0).unwrap_or(path.len());
                    if n > 0 {
                        let file = HSTRING::from(String::from_utf16_lossy(&path[..n]));
                        let mut handle = HICON::default();
                        if SHDefExtractIconW(&file, index, 0, Some(&mut handle), None, size).is_ok()
                            && !handle.is_invalid()
                        {
                            let result = draw_icon(handle, size);
                            let _ = DestroyIcon(handle);
                            if result.is_some() {
                                return result;
                            }
                        }
                    }
                }
                let pidl = link.GetIDList().ok()?;
                let item = SHCreateItemFromIDList(pidl).ok();
                CoTaskMemFree(Some(pidl.cast()));
                item?
            }
        };
        let (width, height, mut pixels) = crate::icons::image_at(&item, size as i32)?;
        opaque_if_missing(&mut pixels);
        Some(Icon {
            width,
            height,
            pixels,
        })
    }
}

fn opaque_if_missing(pixels: &mut [u8]) {
    if pixels.chunks_exact(4).all(|p| p[3] == 0) {
        for p in pixels.chunks_exact_mut(4) {
            p[3] = 255;
        }
    }
}

fn draw_icon(icon: HICON, size: u32) -> Option<Icon> {
    // SAFETY: private zeroed DIB, selected only while drawing; all GDI objects released here.
    unsafe {
        let dc = CreateCompatibleDC(None);
        let info = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: size as i32,
                biHeight: -(size as i32),
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut bits = std::ptr::null_mut();
        let Ok(bitmap) = CreateDIBSection(Some(dc), &info, DIB_RGB_COLORS, &mut bits, None, 0)
        else {
            let _ = DeleteDC(dc);
            return None;
        };
        let old = SelectObject(dc, bitmap.into());
        let drawn =
            DrawIconEx(dc, 0, 0, icon, size as i32, size as i32, 0, None, DI_NORMAL).is_ok();
        let result = drawn.then(|| {
            let mut pixels =
                std::slice::from_raw_parts(bits.cast::<u8>(), (size * size * 4) as usize).to_vec();
            opaque_if_missing(&mut pixels);
            Icon {
                width: size,
                height: size,
                pixels,
            }
        });
        SelectObject(dc, old);
        let _ = DeleteObject(bitmap.into());
        let _ = DeleteDC(dc);
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn link_icon_location_is_dpi_sized_and_cached_without_execution() {
        let _com = glaze_desktop::desktop_view::Com::init();
        // SAFETY: synthetic link in memory; never executed or written to the user's lists.
        unsafe {
            let link: IShellLinkW =
                CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER).unwrap();
            link.SetPath(&HSTRING::from(r"C:\Example\not-an-installed-app.exe"))
                .unwrap();
            link.SetIconLocation(&HSTRING::from(r"C:\Windows\System32\shell32.dll"), 3)
                .unwrap();
            let stream = SHCreateMemStream(None).unwrap();
            link.cast::<IPersistStream>()
                .unwrap()
                .Save(&stream, false)
                .unwrap();
            stream
                .Seek(0, windows::Win32::System::Com::STREAM_SEEK_SET, None)
                .unwrap();
            let launch = Launch::Link(super::super::stream_bytes(&stream).unwrap());
            let small = icon(&launch, 16).unwrap();
            let again = icon(&launch, 16).unwrap();
            let large = icon(&launch, 32).unwrap();
            assert_eq!((small.width, small.height), (16, 16));
            assert_eq!((large.width, large.height), (32, 32));
            assert!(Arc::ptr_eq(&small, &again));
            assert!(small.pixels.chunks_exact(4).any(|p| p[3] > 0));
        }
    }
}
