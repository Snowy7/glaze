//! The app styler (experimental; docs/windows-styling.md, "App styler"): restyles the XAML
//! inside File Explorer and Settings through the documented XAML Diagnostics API.
//!
//! - The **helper** (`mypaper-styler`, `src/main.rs`) runs only while `experimental.app_styler`
//!   is on. It resolves the rules (preset < theme < `look.apps`), writes one rules file per app,
//!   and attaches to allowlisted processes with `InitializeXamlDiagnosticsEx`.
//! - The **TAP** (this crate as a DLL, [`tap`]) is what XAML Diagnostics loads into the app. It
//!   styles elements as they appear, follows the rules file live, and puts everything back when
//!   the helper exits.
//!
//! [`Attach`] is the contract between the two: the helper encodes it as the initialization
//! data, the TAP decodes it.

pub mod presets;
pub mod selector;
mod tap;
mod xamlom;

use std::fmt;
use std::path::PathBuf;

use windows_core::GUID;

/// The TAP's COM class, which XAML Diagnostics instantiates from our DLL.
pub const CLSID_TAP: GUID = GUID::from_u128(0xba403a5f_560e_4d76_bf96_81a98add3411);

/// The two XAML frameworks the styled apps use.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Framework {
    /// `Windows.UI.Xaml` (Settings; Explorer's tabs live in WinUI 3 on current builds).
    System,
    /// `Microsoft.UI.Xaml` from the Windows App SDK (Explorer's header and Home).
    WinUi,
}

impl fmt::Display for Framework {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Framework::System => "system XAML",
            Framework::WinUi => "WinUI 3",
        })
    }
}

/// What the helper tells the TAP when it attaches. XAML Diagnostics silently drops initialization
/// data much longer than 256 characters, so this names one folder and short file names in it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Attach {
    /// The helper's pid: when it exits, the TAP restores everything and stops.
    pub helper: u32,
    pub framework: Framework,
    /// The styler's folder (`%LOCALAPPDATA%\mypaper\app-styler`): rules files, the log, dumps.
    pub dir: PathBuf,
    /// The app's rules file (the settled `AppRules` as JSON) in `dir`, or a full path; watched
    /// for changes.
    pub rules: String,
    /// Write the visual tree to `dir\tree-<pid>.txt` once it has settled (`mypaper-styler
    /// --dump`), with the properties of the elements the selector in `dir\props.txt` picks.
    pub dump: bool,
}

impl Attach {
    pub fn rules_path(&self) -> PathBuf {
        self.dir.join(&self.rules)
    }

    /// Where the TAP appends its log lines.
    pub fn log_path(&self) -> PathBuf {
        self.dir.join("styler.log")
    }

    /// One `key=value` per line.
    pub fn encode(&self) -> String {
        format!(
            "helper={}\nframework={}\ndir={}\nrules={}\ndump={}\n",
            self.helper,
            match self.framework {
                Framework::System => "system",
                Framework::WinUi => "winui",
            },
            self.dir.display(),
            self.rules,
            u8::from(self.dump),
        )
    }

    pub fn decode(text: &str) -> Option<Self> {
        let mut helper = None;
        let mut framework = None;
        let mut dir = None;
        let mut rules = None;
        let mut dump = false;
        for line in text.lines() {
            let (key, value) = line.split_once('=')?;
            match key {
                "helper" => helper = value.parse().ok(),
                "framework" => {
                    framework = match value {
                        "system" => Some(Framework::System),
                        "winui" => Some(Framework::WinUi),
                        _ => None,
                    }
                }
                "dir" => dir = Some(PathBuf::from(value)),
                "rules" => rules = Some(value.to_owned()),
                "dump" => dump = value == "1",
                _ => {}
            }
        }
        Some(Self {
            helper: helper?,
            framework: framework?,
            dir: dir?,
            rules: rules?,
            dump,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn attach_data_round_trips_short_and_needs_the_essentials() {
        let attach = Attach {
            helper: 42,
            framework: Framework::WinUi,
            dir: r"C:\Users\someone-with-a-long-name\AppData\Local\mypaper\app-styler".into(),
            rules: "explorer.json".into(),
            dump: true,
        };
        let data = attach.encode();
        assert!(data.len() < 160, "{} characters", data.len());
        assert_eq!(Attach::decode(&data), Some(attach));
        assert_eq!(Attach::decode("helper=1\nrules=x"), None, "no framework");
        assert_eq!(Attach::decode("garbage"), None);
    }
}
