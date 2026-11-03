//! Why every screen is paused at once ("held"), and how the reasons combine.
//!
//! Pure data, no Win32: the OS reports [`Fact`]s, the user's [`Config`]
//! decides which facts matter, and [`Holds::reasons`] combines both. Any one
//! reason holds the wallpaper at zero CPU/GPU.

use std::fmt;

use mypaper_config::Config;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reason {
    /// Paused from the tray or studio.
    User,
    Battery,
    DisplayOff,
    Locked,
    /// A fullscreen app or game is in front; it gets the whole GPU.
    Fullscreen,
}

impl Reason {
    const ALL: [Self; 5] = [
        Self::User,
        Self::Battery,
        Self::DisplayOff,
        Self::Locked,
        Self::Fullscreen,
    ];

    fn label(self) -> &'static str {
        match self {
            Self::User => "paused by user",
            Self::Battery => "on battery",
            Self::DisplayOff => "display off",
            Self::Locked => "session locked",
            Self::Fullscreen => "fullscreen app in front",
        }
    }
}

/// A set of [`Reason`]s.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct HoldReasons(u8);

impl HoldReasons {
    pub fn is_empty(self) -> bool {
        self.0 == 0
    }

    pub fn contains(self, reason: Reason) -> bool {
        self.0 & (1 << reason as u8) != 0
    }

    fn with(mut self, reason: Reason, on: bool) -> Self {
        if on {
            self.0 |= 1 << reason as u8;
        } else {
            self.0 &= !(1 << reason as u8);
        }
        self
    }
}

impl fmt::Display for HoldReasons {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut labels = Reason::ALL
            .into_iter()
            .filter(|r| self.contains(*r))
            .map(Reason::label);
        match labels.next() {
            None => f.write_str("nothing"),
            Some(first) => {
                f.write_str(first)?;
                labels.try_for_each(|label| write!(f, ", {label}"))
            }
        }
    }
}

/// A system condition the OS notifies us about. Travels from the window
/// procedure to the main loop as a thread message (see `window::WM_FACT`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Fact {
    OnBattery,
    DisplayOff,
    SessionLocked,
}

impl Fact {
    const ALL: [Self; 3] = [Self::OnBattery, Self::DisplayOff, Self::SessionLocked];

    pub fn to_wparam(self) -> usize {
        self as usize
    }

    pub fn from_wparam(wparam: usize) -> Option<Self> {
        Self::ALL.get(wparam).copied()
    }
}

/// The user's settings plus everything observed about the system.
#[derive(Debug, Default)]
pub struct Holds {
    paused: bool,
    pause_on_battery: bool,
    pause_on_fullscreen: bool,
    on_battery: bool,
    display_off: bool,
    locked: bool,
    fullscreen: bool,
}

impl Holds {
    /// Takes the settings from a (new) config.
    pub fn apply(&mut self, config: &Config) {
        self.paused = config.paused;
        self.pause_on_battery = config.pause_on_battery;
        self.pause_on_fullscreen = config.pause_on_fullscreen;
    }

    pub fn set(&mut self, fact: Fact, value: bool) {
        match fact {
            Fact::OnBattery => self.on_battery = value,
            Fact::DisplayOff => self.display_off = value,
            Fact::SessionLocked => self.locked = value,
        }
    }

    pub fn set_fullscreen(&mut self, value: bool) {
        self.fullscreen = value;
    }

    /// Whether fullscreen detection is worth running at all.
    pub fn wants_fullscreen(&self) -> bool {
        self.pause_on_fullscreen
    }

    pub fn reasons(&self) -> HoldReasons {
        HoldReasons::default()
            .with(Reason::User, self.paused)
            .with(Reason::Battery, self.pause_on_battery && self.on_battery)
            .with(Reason::DisplayOff, self.display_off)
            .with(Reason::Locked, self.locked)
            .with(
                Reason::Fullscreen,
                self.pause_on_fullscreen && self.fullscreen,
            )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn holds(config: Config) -> Holds {
        let mut holds = Holds::default();
        holds.apply(&config);
        holds
    }

    #[test]
    fn nothing_held_by_default() {
        assert!(holds(Config::default()).reasons().is_empty());
    }

    #[test]
    fn reasons_combine() {
        let mut holds = holds(Config::default());
        holds.set(Fact::OnBattery, true);
        holds.set_fullscreen(true);
        let reasons = holds.reasons();
        assert!(reasons.contains(Reason::Battery) && reasons.contains(Reason::Fullscreen));
        assert!(!reasons.contains(Reason::User));
        assert_eq!(reasons.to_string(), "on battery, fullscreen app in front");

        // Dropping one reason keeps the other.
        holds.set(Fact::OnBattery, false);
        assert_eq!(holds.reasons().to_string(), "fullscreen app in front");
    }

    #[test]
    fn settings_gate_battery_and_fullscreen_but_not_the_rest() {
        let mut holds = holds(Config {
            pause_on_battery: false,
            pause_on_fullscreen: false,
            ..Config::default()
        });
        holds.set(Fact::OnBattery, true);
        holds.set_fullscreen(true);
        assert!(holds.reasons().is_empty());

        holds.set(Fact::DisplayOff, true);
        holds.set(Fact::SessionLocked, true);
        assert!(holds.reasons().contains(Reason::DisplayOff));
        assert!(holds.reasons().contains(Reason::Locked));
    }

    #[test]
    fn config_changes_take_effect_on_apply() {
        let mut holds = holds(Config::default());
        holds.set_fullscreen(true);
        assert!(holds.reasons().contains(Reason::Fullscreen));
        holds.apply(&Config {
            paused: true,
            pause_on_fullscreen: false,
            ..Config::default()
        });
        assert_eq!(holds.reasons().to_string(), "paused by user");
    }

    #[test]
    fn facts_round_trip_through_wparam() {
        for fact in Fact::ALL {
            assert_eq!(Fact::from_wparam(fact.to_wparam()), Some(fact));
        }
        assert_eq!(Fact::from_wparam(99), None);
    }
}
