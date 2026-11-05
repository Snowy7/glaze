//! On-demand public Shell Recent/Frequent categories and URL encoding.
use anyhow::{Result, ensure};
use windows::{
    Win32::{
        System::Com::{CLSCTX_INPROC_SERVER, CoCreateInstance, CoTaskMemFree},
        UI::Shell::{
            self, ADLT_FREQUENT, ADLT_RECENT, ApplicationDocumentLists, Common::IObjectArray,
            IApplicationDocumentLists, IShellItem, IShellLinkW, SIGDN_DESKTOPABSOLUTEPARSING,
            SIGDN_NORMALDISPLAY,
        },
    },
    core::{Interface, PCWSTR, w},
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Recent {
    pub title: String,
    pub target: String,
    pub arguments: String,
}
fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain([0]).collect()
}
fn string(value: &[u16]) -> String {
    String::from_utf16_lossy(&value[..value.iter().position(|v| *v == 0).unwrap_or(value.len())])
}

/// Runs on a COM worker only when the user's app context panel opens. Pinned/custom/tasks are excluded.
pub fn recent(app_id: &str, frequent: bool) -> Result<Vec<Recent>> {
    let _com = crate::media::Com::new()?;
    let id = wide(app_id);
    // SAFETY: COM is initialized and Shell outputs are owned interfaces/buffers.
    unsafe {
        let lists: IApplicationDocumentLists =
            CoCreateInstance(&ApplicationDocumentLists, None, CLSCTX_INPROC_SERVER)?;
        lists.SetAppID(PCWSTR(id.as_ptr()))?;
        let list: IObjectArray =
            lists.GetList(if frequent { ADLT_FREQUENT } else { ADLT_RECENT }, 12)?;
        let mut result = Vec::new();
        for index in 0..list.GetCount()?.min(12) {
            let object: windows::core::IUnknown = list.GetAt(index)?;
            if let Ok(item) = object.cast::<IShellItem>() {
                let title = item.GetDisplayName(SIGDN_NORMALDISPLAY)?;
                let name = title.to_string();
                CoTaskMemFree(Some(title.0.cast()));
                let target = item.GetDisplayName(SIGDN_DESKTOPABSOLUTEPARSING)?;
                let path = target.to_string();
                CoTaskMemFree(Some(target.0.cast()));
                result.push(Recent {
                    title: name?,
                    target: path?,
                    arguments: String::new(),
                });
            } else if let Ok(link) = object.cast::<IShellLinkW>() {
                let (mut path, mut args, mut description) = ([0; 32768], [0; 4096], [0; 1024]);
                link.GetPath(&mut path, std::ptr::null_mut(), 0)?;
                link.GetArguments(&mut args)?;
                link.GetDescription(&mut description)?;
                let path = string(&path);
                let title = string(&description);
                result.push(Recent {
                    title: if title.is_empty() {
                        path.clone()
                    } else {
                        title
                    },
                    target: path,
                    arguments: string(&args),
                });
            }
        }
        Ok(result)
    }
}

/// ShellExecute preserves Shell link arguments; no command shell is involved.
pub fn open(item: &Recent) -> Result<()> {
    let (target, args) = (wide(&item.target), wide(&item.arguments));
    // SAFETY: both null-terminated strings live through ShellExecuteW.
    let result = unsafe {
        Shell::ShellExecuteW(
            None,
            w!("open"),
            PCWSTR(target.as_ptr()),
            PCWSTR(args.as_ptr()),
            None,
            windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL,
        )
    };
    ensure!(result.0 as isize > 32, "Windows could not open the item");
    Ok(())
}

pub fn encode(value: &str) -> String {
    value
        .bytes()
        .map(|b| {
            if b.is_ascii_alphanumeric() || b"-._~".contains(&b) {
                (b as char).to_string()
            } else {
                format!("%{b:02X}")
            }
        })
        .collect()
}
