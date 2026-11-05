//! Grok Bot's public desktop-status marker. No private database, cookies or account tokens.
use serde::Deserialize;
use std::path::PathBuf;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Discovery {
    pub installed: bool,
    pub running: bool,
    pub signed_in: bool,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Marker {
    version: u32,
    pid: u32,
    signed_in: bool,
}

pub fn status_path() -> PathBuf {
    PathBuf::from(std::env::var_os("APPDATA").unwrap_or_default())
        .join("Grok Bot/desktop-status.json")
}

pub fn discover() -> Discovery {
    let installed = PathBuf::from(std::env::var_os("LOCALAPPDATA").unwrap_or_default())
        .join("Programs/Grok Bot/Grok Bot.exe")
        .is_file();
    let marker = std::fs::read(status_path())
        .ok()
        .filter(|b| b.len() <= 4096)
        .and_then(|b| serde_json::from_slice::<Marker>(&b).ok())
        .filter(|m| m.version == 1);
    let mut state = Discovery {
        installed,
        ..Default::default()
    };
    if let Some(marker) = marker {
        use windows::Win32::System::Threading::{
            OpenProcess, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
            QueryFullProcessImageNameW,
        };
        // SAFETY: read-only query of the marker's pid; the owned handle is closed immediately.
        unsafe {
            if let Ok(process) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, marker.pid) {
                let mut name = [0_u16; 32768];
                let mut size = name.len() as u32;
                if QueryFullProcessImageNameW(
                    process,
                    PROCESS_NAME_WIN32,
                    windows::core::PWSTR(name.as_mut_ptr()),
                    &mut size,
                )
                .is_ok()
                {
                    state.running = String::from_utf16_lossy(&name[..size as usize])
                        .to_ascii_lowercase()
                        .ends_with("\\grok bot.exe");
                }
                let _ = windows::Win32::Foundation::CloseHandle(process);
            }
        }
        state.signed_in = state.running && marker.signed_in;
    }
    state.installed |= state.running;
    state
}
