//! Serializable layout preferences and configurable Win-key shortcuts; no platform dependencies.
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Center {
    #[default]
    Never,
    Always,
    OnOverflow,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Direction {
    Left,
    Right,
    Up,
    Down,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", tag = "action", content = "direction")]
pub enum Command {
    Focus(Direction),
    Move(Direction),
    FocusMonitor(Direction),
    MoveMonitor(Direction),
    Workspace(Direction),
    MoveWorkspace(Direction),
    CycleWidth,
    CycleHeight,
    Maximize,
    Fullscreen,
    Center,
    Floating,
    Tabbed,
    Consume,
    Expel,
    Overview,
}

/// The Windows key is implicit. Other modifiers must match exactly.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Binding {
    pub key: u16,
    #[serde(default)]
    pub ctrl: bool,
    #[serde(default)]
    pub shift: bool,
    #[serde(default)]
    pub alt: bool,
    pub command: Command,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Settings {
    pub preset_widths: Vec<f64>,
    pub default_width: f64,
    pub gaps: f64,
    pub center_focused_column: Center,
    pub keybindings: Vec<Binding>,
    pub animations: bool,
}

impl Default for Settings {
    fn default() -> Self {
        let mut keybindings = Vec::new();
        // Arrows avoid Win+L (lock) and Win+G / Win+Alt+R (Game Bar).
        for (key, direction) in [
            (0x25, Direction::Left),
            (0x27, Direction::Right),
            (0x26, Direction::Up),
            (0x28, Direction::Down),
        ] {
            for (ctrl, shift) in [(false, false), (true, false), (false, true), (true, true)] {
                let command = match (ctrl, shift) {
                    (false, false) => Command::Focus(direction),
                    (true, false) => Command::Move(direction),
                    (false, true) => Command::FocusMonitor(direction),
                    (true, true) => Command::MoveMonitor(direction),
                };
                keybindings.push(Binding {
                    key,
                    ctrl,
                    shift,
                    alt: false,
                    command,
                });
            }
        }
        for (key, command) in [
            (0x21, Command::Workspace(Direction::Up)),
            (0x22, Command::Workspace(Direction::Down)),
            (0x52, Command::CycleWidth),
            (0x46, Command::Maximize),
            (0x43, Command::Center),
            (0x56, Command::Floating),
            (0x57, Command::Tabbed),
            (0xBC, Command::Consume),
            (0xBE, Command::Expel),
            (0x09, Command::Overview),
        ] {
            keybindings.push(Binding {
                key,
                ctrl: false,
                shift: false,
                alt: false,
                command,
            });
        }
        for (key, ctrl, command) in [
            (0x52, false, Command::CycleHeight),
            (0x46, false, Command::Fullscreen),
            (0x21, true, Command::MoveWorkspace(Direction::Up)),
            (0x22, true, Command::MoveWorkspace(Direction::Down)),
        ] {
            keybindings.push(Binding {
                key,
                ctrl,
                shift: !ctrl,
                alt: false,
                command,
            });
        }
        Self {
            preset_widths: vec![1.0 / 3.0, 0.5, 2.0 / 3.0],
            default_width: 0.5,
            gaps: 16.0,
            center_focused_column: Center::Never,
            keybindings,
            animations: true,
        }
    }
}

impl Settings {
    pub fn is_default(&self) -> bool {
        self == &Self::default()
    }

    /// Reject invalid geometry and shortcuts the OS reserves or our launcher owns.
    pub fn validate(&self) -> Result<(), &'static str> {
        if !self.gaps.is_finite() || !(0.0..=128.0).contains(&self.gaps) {
            return Err("tiling.gaps must be 0..128");
        }
        if self.preset_widths.is_empty()
            || self.preset_widths.len() > 16
            || self
                .preset_widths
                .iter()
                .chain([&self.default_width])
                .any(|v| !v.is_finite() || !(0.05..=1.0).contains(v))
        {
            return Err("tiling widths must be finite proportions in 0.05..1");
        }
        for (i, b) in self.keybindings.iter().enumerate() {
            if b.key == 0
                || b.key > 255
                || matches!(b.key, 0x4C | 0x47 | 0x5B | 0x5C)
                || (!b.ctrl && !b.shift && !b.alt && matches!(b.key, 0x41 | 0x4E | 0x45))
                || (b.alt && matches!(b.key, 0x42 | 0x44 | 0x47 | 0x4D | 0x50 | 0x52 | 0x57))
            {
                return Err("tiling shortcut conflicts with Windows, Game Bar, or mypaper");
            }
            if self.keybindings[..i]
                .iter()
                .any(|a| (a.key, a.ctrl, a.shift, a.alt) == (b.key, b.ctrl, b.shift, b.alt))
            {
                return Err("duplicate tiling shortcut");
            }
        }
        Ok(())
    }
}
