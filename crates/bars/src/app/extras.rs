//! The host's side of phase 2's behavior (docs/bars.md): windows asking for attention (the shell
//! hook's `HSHELL_FLASH`), custom modules, the dock's Recycle Bin, window previews, and the
//! `exec`/`open` actions `bars.json` binds to module inputs.

use std::collections::{BTreeMap, HashSet};
use std::time::Instant;

use mypaper_bars::ModuleRef;
use mypaper_bars::config::{Action, Trigger};
use serde_json::{Value, json};

use super::App;
use crate::bridge::{BarError, PreviewWindow, Topic, bar_modules};
use crate::custom::{self, Custom, Spec};
use crate::system;
use crate::trash::{self, TrashWatch};
use crate::wake::Waker;

/// `HSHELL_*` codes (WinUser.h).
const HSHELL_WINDOWDESTROYED: usize = 2;
const HSHELL_WINDOWACTIVATED: usize = 4;
const HSHELL_RUDEAPPACTIVATED: usize = 0x8004;
const HSHELL_FLASH: usize = 0x8006;

pub(super) struct Extras {
    pub custom: Custom,
    /// Windows flashing for attention since they were last in front.
    pub attention: HashSet<isize>,
    trash: Option<TrashWatch>,
    pub trash_full: bool,
}

impl Extras {
    pub fn new(waker: Waker) -> Self {
        Self {
            custom: Custom::new(waker),
            attention: HashSet::new(),
            trash: None,
            trash_full: false,
        }
    }
}

impl App {
    /// Starts and stops what the extra topics need, as `sync_topics` changes them.
    pub(super) fn sync_extras(&mut self) {
        let wanted: BTreeMap<String, Spec> = if self.active.contains(&Topic::Custom) {
            self.bars
                .iter()
                .filter(|bar| !bar.hidden)
                .flat_map(|bar| {
                    bar_modules(bar.kind, &bar.config).filter_map(|module| {
                        let ModuleRef::Custom(id) = module else {
                            return None;
                        };
                        let options = bar.config.modules.get(module)?;
                        let command = options.command.as_deref()?;
                        Some((
                            id.clone(),
                            Spec::new(command, &options.args, options.interval),
                        ))
                    })
                })
                .collect()
        } else {
            BTreeMap::new()
        };
        self.extras.custom.sync(wanted);
        self.timers.custom = self.extras.custom.tick(Instant::now());

        match (
            self.active.contains(&Topic::Trash),
            self.extras.trash.is_some(),
        ) {
            (true, false) => {
                self.extras.trash = TrashWatch::new(self.broadcast);
                self.extras.trash_full = trash::full();
            }
            (false, true) => self.extras.trash = None,
            _ => {}
        }
    }

    pub(super) fn custom_value(&self) -> Value {
        json!(self.extras.custom.outputs)
    }

    pub(super) fn trash_value(&self) -> Value {
        json!({ "full": self.extras.trash_full })
    }

    /// The Recycle Bin changed.
    pub(super) fn trash_changed(&mut self) {
        let full = trash::full();
        if full != self.extras.trash_full {
            self.extras.trash_full = full;
            self.emit(Topic::Trash);
        }
    }

    /// A shell hook event: windows flashing for attention, and the end of it.
    pub(super) fn on_shell(&mut self, code: usize, hwnd: isize) {
        let changed = match code {
            HSHELL_FLASH => self.extras.attention.insert(hwnd),
            HSHELL_WINDOWACTIVATED | HSHELL_RUDEAPPACTIVATED | HSHELL_WINDOWDESTROYED => {
                self.extras.attention.remove(&hwnd)
            }
            _ => false,
        };
        if changed {
            self.emit(Topic::Apps);
        }
    }

    /// A window came to the front: it no longer needs attention.
    pub(super) fn attended(&mut self, hwnd: isize) {
        if self.extras.attention.remove(&hwnd) {
            self.emit(Topic::Apps);
        }
    }

    /// Runs the `exec` or `open` action the bar's config binds to `module`'s `trigger`. Built-in
    /// actions are the page's own: they aren't run here.
    pub(super) fn run_action(
        &mut self,
        index: usize,
        module: &ModuleRef,
        trigger: Trigger,
    ) -> Result<Value, BarError> {
        let action = self.bars[index]
            .config
            .modules
            .get(module)
            .and_then(|options| options.click.get(trigger))
            .ok_or_else(|| BarError::not_found("no action is bound to that"))?;
        match action {
            Action::Exec { exec, args } => custom::exec(exec, args),
            Action::Open { open } => system::open(open.clone()),
            Action::Builtin(_) => {
                return Err(BarError::invalid("built-in actions run in the page"));
            }
        }
        Ok(Value::Null)
    }

    pub(super) fn preview_open(&mut self, index: usize, hwnds: &[isize]) -> Value {
        let bar = &mut self.bars[index];
        let hwnd = bar.hwnd();
        json!(bar.previews.open(hwnd, hwnds))
    }

    pub(super) fn preview_place(&self, index: usize, windows: &[PreviewWindow]) {
        let bar = &self.bars[index];
        let placed: Vec<(isize, [f64; 4])> = windows.iter().map(|w| (w.hwnd, w.rect)).collect();
        bar.previews.place(&placed, bar.scale());
    }
}
