//! App icons for the dock's pinned items: 64 px PNGs from the shell
//! (`IShellItemImageFactory`, docs/bars.md "Icons"), cached as files in
//! `%LOCALAPPDATA%\mypaper\studio-icons` so the webview can load them through
//! the asset protocol. A file's icon is re-made when its modified time changes.

use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use anyhow::{Context, Result};
use windows::Win32::Foundation::SIZE;
use windows::Win32::UI::Shell::SIIGBF_ICONONLY;

use crate::fsio;
use crate::thumbs;

const SIZE_PX: SIZE = SIZE { cx: 64, cy: 64 };

/// The cache file name for an item: changes with its path and, for real files, its modified time.
fn cache_name(source: &str, modified_nanos: u128) -> String {
    let key = format!("{}|{modified_nanos}", source.to_lowercase());
    format!("{:016x}.png", thumbs::fnv1a(key.as_bytes()))
}

fn modified_nanos(source: &str) -> u128 {
    // Store apps (`shell:AppsFolder\...`) aren't files; their key is the path alone.
    std::fs::metadata(source)
        .and_then(|meta| meta.modified())
        .ok()
        .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
        .map_or(0, |d| d.as_nanos())
}

/// The icon PNG for an exe, `.lnk`, folder or `shell:AppsFolder\<AUMID>` path,
/// made in `dir` if it isn't there yet.
pub fn icon_in(dir: &Path, source: &str) -> Result<PathBuf> {
    let target = dir.join(cache_name(source, modified_nanos(source)));
    if target.is_file() {
        return Ok(target);
    }
    let image = thumbs::shell_image(OsStr::new(source), SIZE_PX, SIIGBF_ICONONLY).with_context(|| format!("getting the icon of {source}"))?;
    fsio::write_atomic(&target, thumbs::encode_png(&image)?)?;
    Ok(target)
}

pub fn icon(source: &str) -> Result<PathBuf> {
    icon_in(&thumbs::local_dir()?.join("studio-icons"), source)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::source::tests::temp_dir;

    #[test]
    fn cache_names_follow_the_path_case_insensitively_and_the_file_version() {
        let name = cache_name(r"C:\Windows\explorer.exe", 5);
        assert_eq!(name, cache_name(r"c:\windows\EXPLORER.exe", 5));
        assert_ne!(name, cache_name(r"C:\Windows\explorer.exe", 6));
        assert_ne!(name, cache_name(r"C:\Windows\notepad.exe", 5));
    }

    #[test]
    fn the_shell_makes_a_png_icon_for_an_exe_and_reuses_it() {
        let dir = temp_dir("icons");
        let exe = r"C:\Windows\explorer.exe";
        let first = icon_in(&dir, exe).unwrap();
        let bytes = std::fs::read(&first).unwrap();
        assert_eq!(&bytes[..8], b"\x89PNG\r\n\x1a\n");
        let reader = png::Decoder::new(std::io::Cursor::new(&bytes)).read_info().unwrap();
        assert_eq!(reader.info().width, 64);

        assert_eq!(icon_in(&dir, exe).unwrap(), first);
        assert!(icon_in(&dir, r"C:\no\such\app.exe").is_err());
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
