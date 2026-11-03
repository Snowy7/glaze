//! Injection policy. The hard rule: mypaper only ever injects into **Windows
//! itself and built-in Windows apps**. Never games, never third-party apps.
//!
//! This is a strict allowlist (anything not listed is denied) and the module
//! is pure, so the rule is unit-tested rather than trusted. The injector must
//! call [`evaluate`] for every process before touching it, and must not follow
//! child processes.

pub mod signature;

use std::path::{Component, Path};

/// Built-in Windows executables, relative to the Windows directory.
/// Deliberately small. Add entries one by one, with a reason.
const SYSTEM_EXECUTABLES: &[&str] = &[
    r"explorer.exe", // desktop, taskbar, File Explorer, Control Panel
    r"regedit.exe",
    r"ImmersiveControlPanel\SystemSettings.exe", // Settings
    r"SystemApps\Microsoft.Windows.StartMenuExperienceHost_cw5n1h2txyewy\StartMenuExperienceHost.exe",
    r"SystemApps\ShellExperienceHost_cw5n1h2txyewy\ShellExperienceHost.exe", // notification/action center
    r"SystemApps\MicrosoftWindows.Client.CBS_cw5n1h2txyewy\SearchHost.exe",
    r"System32\ApplicationFrameHost.exe", // frames around UWP apps
    r"System32\Taskmgr.exe",
    r"System32\mmc.exe", // Device Manager, Disk Management, ...
    r"System32\notepad.exe",
    r"System32\charmap.exe",
];

/// Inbox Store-packaged Windows apps, by package family name.
/// Family names are not unique to Windows (Microsoft also publishes games
/// through the Store), which is exactly why this is an allowlist.
const PACKAGE_FAMILIES: &[&str] = &[
    "Microsoft.WindowsNotepad_8wekyb3d8bbwe",
    "Microsoft.WindowsTerminal_8wekyb3d8bbwe",
    "Microsoft.WindowsCalculator_8wekyb3d8bbwe",
    "Microsoft.Windows.Photos_8wekyb3d8bbwe",
    "Microsoft.Paint_8wekyb3d8bbwe",
    "Microsoft.ScreenSketch_8wekyb3d8bbwe",  // Snipping Tool
    "Microsoft.WindowsAlarms_8wekyb3d8bbwe", // Clock
];

/// What the injector knows about a candidate process.
#[derive(Debug, Clone, Copy)]
pub struct Target<'a> {
    /// Full image path, as from `QueryFullProcessImageNameW`.
    pub image: &'a Path,
    /// Package family name, if the process is packaged (`GetPackageFamilyName`).
    pub package_family: Option<&'a str>,
    /// The image carries a valid Microsoft Windows signature (embedded or catalog).
    pub microsoft_signed: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    Allow,
    Deny(DenyReason),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DenyReason {
    NotSigned,
    NotAllowlisted,
    SuspiciousPath,
}

/// Decides whether `target` may be injected. `windows_dir` is the system's
/// Windows directory (`GetWindowsDirectoryW`, usually `C:\Windows`).
pub fn evaluate(target: &Target, windows_dir: &Path) -> Verdict {
    if !is_plain_absolute(target.image) {
        return Verdict::Deny(DenyReason::SuspiciousPath);
    }
    if !target.microsoft_signed {
        return Verdict::Deny(DenyReason::NotSigned);
    }

    let allowlisted = match target.package_family {
        Some(family) => PACKAGE_FAMILIES
            .iter()
            .any(|f| f.eq_ignore_ascii_case(family)),
        None => windows_relative(target.image, windows_dir).is_some_and(|rel| {
            SYSTEM_EXECUTABLES
                .iter()
                .any(|e| e.eq_ignore_ascii_case(&rel))
        }),
    };

    if allowlisted {
        Verdict::Allow
    } else {
        Verdict::Deny(DenyReason::NotAllowlisted)
    }
}

/// Absolute, and no `.`/`..` segments that could make a prefix check lie.
fn is_plain_absolute(path: &Path) -> bool {
    path.is_absolute()
        && path
            .components()
            .all(|c| !matches!(c, Component::ParentDir | Component::CurDir))
}

/// `C:\Windows\System32\x.exe` -> `System32\x.exe`, case-insensitively.
fn windows_relative(image: &Path, windows_dir: &Path) -> Option<String> {
    let image = image.to_str()?.replace('/', "\\");
    let dir = windows_dir.to_str()?.replace('/', "\\");
    let dir = dir.trim_end_matches('\\');
    let head = image.get(..dir.len())?;
    let rest = image.get(dir.len()..)?.strip_prefix('\\')?;
    head.eq_ignore_ascii_case(dir).then(|| rest.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    const WIN: &str = r"C:\Windows";

    fn check(image: &str, family: Option<&str>, signed: bool) -> Verdict {
        let target = Target {
            image: Path::new(image),
            package_family: family,
            microsoft_signed: signed,
        };
        evaluate(&target, Path::new(WIN))
    }

    #[test]
    fn allows_windows_shell_and_settings() {
        assert_eq!(
            check(r"C:\Windows\explorer.exe", None, true),
            Verdict::Allow
        );
        assert_eq!(
            check(
                r"c:\windows\immersivecontrolpanel\systemsettings.exe",
                None,
                true
            ),
            Verdict::Allow
        );
    }

    #[test]
    fn allows_inbox_packaged_apps() {
        let notepad = r"C:\Program Files\WindowsApps\Microsoft.WindowsNotepad_11.2410.21.0_x64__8wekyb3d8bbwe\Notepad\Notepad.exe";
        assert_eq!(
            check(
                notepad,
                Some("Microsoft.WindowsNotepad_8wekyb3d8bbwe"),
                true
            ),
            Verdict::Allow
        );
    }

    #[test]
    fn denies_games_even_microsoft_published_ones() {
        assert_eq!(
            check(r"D:\Games\Valorant\VALORANT.exe", None, false),
            Verdict::Deny(DenyReason::NotSigned)
        );
        let minecraft = r"C:\Program Files\WindowsApps\Microsoft.MinecraftUWP_1.21.0.0_x64__8wekyb3d8bbwe\Minecraft.Windows.exe";
        assert_eq!(
            check(
                minecraft,
                Some("Microsoft.MinecraftUWP_8wekyb3d8bbwe"),
                true
            ),
            Verdict::Deny(DenyReason::NotAllowlisted)
        );
    }

    #[test]
    fn denies_lookalikes() {
        // Right name, wrong place.
        assert_eq!(
            check(r"D:\Games\explorer.exe", None, true),
            Verdict::Deny(DenyReason::NotAllowlisted)
        );
        // Right place, not signed (e.g. a dropped binary).
        assert_eq!(
            check(r"C:\Windows\explorer.exe", None, false),
            Verdict::Deny(DenyReason::NotSigned)
        );
        // Path tricks.
        assert_eq!(
            check(r"C:\Windows\..\Games\explorer.exe", None, true),
            Verdict::Deny(DenyReason::SuspiciousPath)
        );
        assert_eq!(
            check(r"C:\WindowsX\explorer.exe", None, true),
            Verdict::Deny(DenyReason::NotAllowlisted)
        );
    }

    #[test]
    fn never_allows_critical_or_non_ui_system_processes() {
        for exe in [
            "lsass.exe",
            "csrss.exe",
            "winlogon.exe",
            "dwm.exe",
            "svchost.exe",
            "consent.exe",
        ] {
            let path = format!(r"C:\Windows\System32\{exe}");
            assert_eq!(
                check(&path, None, true),
                Verdict::Deny(DenyReason::NotAllowlisted),
                "{exe}"
            );
        }
    }
}
