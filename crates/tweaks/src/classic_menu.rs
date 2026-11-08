//! The Windows 11 "classic context menu" toggle: an empty default value at
//! `HKCU\Software\Classes\CLSID\{86ca1aa0-...}\InprocServer32` makes Explorer
//! fall back to the full (Windows 10 style) right-click menu everywhere.
//!
//! **Undocumented** (the CLSID is the one the Win11 menu handler registers;
//! the trick is widely used, Microsoft does not document it), and it is a
//! user-visible global change, so it is strictly opt-in
//! (`classic_context_menu` in `config.json`). It is journaled in its own file,
//! `%LOCALAPPDATA%\glaze\undo-shell.json`, which, unlike the theme's styling
//! journal, outlives the run: the menu stays classic until the option is turned
//! off, and turning it off removes exactly what was added (including the keys
//! we created). Explorer reads the value when it starts, so a change needs an
//! Explorer restart (or signing out) to show; **we never restart Explorer**.

use std::path::PathBuf;
use std::time::Duration;

use anyhow::Result;

use crate::palette::local_dir;
use crate::registry::Registry;
use crate::undo::{Store, Undo, Value, Write};

/// The Win11 menu handler's registration, overridden for the current user.
pub const KEY: &str =
    r"Software\Classes\CLSID\{86ca1aa0-34aa-4e8b-a509-50c905bae2a2}\InprocServer32";

const JOURNAL: &str = "undo-shell.json";

/// The registry change that selects the classic menu: an empty default value.
pub fn writes() -> Vec<Write> {
    vec![Write {
        key: KEY,
        name: "",
        value: Value::String(String::new()),
    }]
}

/// What a call to [`ClassicMenu::set`] did to the registry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Change {
    /// The menu setting changed: Explorer needs a restart to show it.
    Applied,
    /// Already as asked (including a key the user had added by hand).
    Unchanged,
}

pub struct ClassicMenu<S = Registry> {
    undo: Undo<S>,
}

impl ClassicMenu {
    pub fn new() -> Result<Self> {
        Ok(Self::with(Registry, local_dir()?.join(JOURNAL)))
    }
}

impl<S: Store> ClassicMenu<S> {
    fn with(store: S, journal: PathBuf) -> Self {
        Self {
            undo: Undo::new(store, journal, Duration::ZERO),
        }
    }

    /// Did Glaze turn the classic menu on (and not yet off)?
    pub fn is_on(&self) -> bool {
        self.undo.pending()
    }

    /// Turns the classic menu on or off. Off restores what was there before
    /// we first turned it on. Safe to call repeatedly, e.g. at every start.
    pub fn set(&mut self, on: bool) -> Result<Change> {
        let changed = if on {
            self.undo.apply(&writes())?
        } else {
            self.undo.restore(|| {})?
        };
        Ok(if changed {
            Change::Applied
        } else {
            Change::Unchanged
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::undo::tests::{Memory, Scratch};

    const CLSID_PARENT: &str = r"Software\Classes\CLSID";

    /// A registry that has other classes, and a menu toggle over it. `test` keeps
    /// the scratch folders of parallel tests apart.
    fn shell(test: &str) -> (Memory, ClassicMenu<Memory>, Scratch) {
        let scratch = Scratch::new(&format!("classic-menu-{test}"));
        let registry = Memory::default();
        registry.create_key(CLSID_PARENT); // other classes are registered here
        let menu = ClassicMenu::with(registry.clone(), scratch.0.join(JOURNAL));
        (registry, menu, scratch)
    }

    #[test]
    fn the_plan_is_one_empty_default_value_on_the_win11_handler() {
        let plan = writes();
        assert_eq!(plan.len(), 1);
        let write = &plan[0];
        assert_eq!(write.name, "", "the default value");
        assert_eq!(write.value, Value::String(String::new()));
        assert!(
            write
                .key
                .starts_with(r"Software\Classes\CLSID\{86ca1aa0-34aa-4e8b-a509-50c905bae2a2}")
        );
        assert!(write.key.ends_with(r"\InprocServer32"));
    }

    #[test]
    fn turning_it_on_then_off_leaves_the_registry_as_it_was() {
        let (registry, mut menu, _scratch) = shell("roundtrip");
        assert!(!menu.is_on());

        assert_eq!(menu.set(true).unwrap(), Change::Applied);
        assert!(menu.is_on());
        assert_eq!(
            registry.read(KEY, "").unwrap(),
            Some(Value::String(String::new()))
        );
        assert_eq!(
            menu.set(true).unwrap(),
            Change::Unchanged,
            "asking twice changes nothing"
        );

        assert_eq!(menu.set(false).unwrap(), Change::Applied);
        assert!(!menu.is_on());
        assert_eq!(registry.read(KEY, "").unwrap(), None);
        // The keys we created are gone, the ones that were there stay.
        assert!(
            !registry.has_key(r"Software\Classes\CLSID\{86ca1aa0-34aa-4e8b-a509-50c905bae2a2}")
        );
        assert!(registry.has_key(CLSID_PARENT));
        assert_eq!(menu.set(false).unwrap(), Change::Unchanged);
    }

    #[test]
    fn the_journal_outlives_the_process_so_a_later_run_can_turn_it_off() {
        let (registry, mut first, scratch) = shell("restart");
        first.set(true).unwrap();
        drop(first);

        let mut next_run = ClassicMenu::with(registry.clone(), scratch.0.join(JOURNAL));
        assert!(next_run.is_on());
        next_run.set(false).unwrap();
        assert_eq!(registry.read(KEY, "").unwrap(), None);
    }

    #[test]
    fn a_value_the_user_added_by_hand_is_not_touched_when_we_turn_it_off() {
        let (mut registry, mut menu, _scratch) = shell("by-hand");
        registry
            .write(KEY, "", &Value::String(String::new()))
            .unwrap();

        assert_eq!(
            menu.set(true).unwrap(),
            Change::Unchanged,
            "nothing to change, no restart needed"
        );
        assert_eq!(menu.set(false).unwrap(), Change::Unchanged);
        assert_eq!(
            registry.read(KEY, "").unwrap(),
            Some(Value::String(String::new()))
        );
    }
}
