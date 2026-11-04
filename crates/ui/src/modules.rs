//! The DLLs loaded into this process, and the GPU driver check of plan Rule 1: no vendor
//! driver may ever load into the shell (only the engine has a hardware device).

use windows::Win32::Foundation::HMODULE;
use windows::Win32::System::ProcessStatus::{EnumProcessModules, GetModuleBaseNameW};
use windows::Win32::System::Threading::GetCurrentProcess;

/// The base names of every module loaded into this process, lowercase, sorted.
pub fn loaded() -> Vec<String> {
    let mut modules = vec![HMODULE::default(); 1024];
    let mut needed = 0u32;
    // SAFETY: the buffer and its size in bytes match; our own pseudo process handle.
    let ok = unsafe {
        EnumProcessModules(
            GetCurrentProcess(),
            modules.as_mut_ptr(),
            (modules.len() * size_of::<HMODULE>()) as u32,
            &mut needed,
        )
    };
    if ok.is_err() {
        return Vec::new();
    }
    modules.truncate(needed as usize / size_of::<HMODULE>());
    let mut names: Vec<String> = modules
        .into_iter()
        .filter_map(|module| {
            let mut name = [0u16; 260];
            // SAFETY: a module of our own process and a buffer of its stated length.
            let len = unsafe { GetModuleBaseNameW(GetCurrentProcess(), Some(module), &mut name) };
            (len > 0).then(|| String::from_utf16_lossy(&name[..len as usize]).to_lowercase())
        })
        .collect();
    names.sort();
    names
}

/// Is `name` (a DLL's base name) a GPU vendor's user-mode driver: `nv*umx`, `nvgpucomp64`,
/// `amdxc64` or `atidx*`?
pub fn is_gpu_driver(name: &str) -> bool {
    let name = name.to_lowercase();
    let stem = name.strip_suffix(".dll").unwrap_or(&name);
    (stem.starts_with("nv") && stem.ends_with("umx"))
        || stem == "nvgpucomp64"
        || stem == "amdxc64"
        || stem.starts_with("atidx")
}

/// The GPU drivers loaded into this process (empty, always, for the shell).
pub fn gpu_drivers() -> Vec<String> {
    loaded().into_iter().filter(|m| is_gpu_driver(m)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vendor_drivers_match_and_system_dlls_do_not() {
        for driver in [
            "nvwgf2umx.dll",
            "nvd3dumx.dll",
            "NVGPUCOMP64.DLL",
            "amdxc64.dll",
            "atidxx64.dll",
        ] {
            assert!(is_gpu_driver(driver), "{driver}");
        }
        for system in ["d3d11.dll", "dxgi.dll", "d3d10warp.dll", "d2d1.dll", "nvml.dll"] {
            assert!(!is_gpu_driver(system), "{system}");
        }
        assert!(loaded().iter().any(|m| m == "kernel32.dll"));
    }
}
