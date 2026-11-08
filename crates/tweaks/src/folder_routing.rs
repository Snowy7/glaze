//! Experimental per-user Folder/Win+E routing. Originals and our last writes are journaled
//! before touching HKCU; disabling preserves later changes made by another app.

use std::{path::{Path, PathBuf}, time::Duration};
use anyhow::{Result, ensure};
use crate::{palette::local_dir, registry::Registry, undo::{Store, Undo, Value, Write}};

pub const KEYS: [&str; 3] = [
    r"Software\Classes\Folder\shell\open\command",
    r"Software\Classes\Folder\shell\explore\command",
    r"Software\Classes\CLSID\{52205fd8-5dfb-447d-801a-d0b52f2e83e1}\shell\opennewwindow\command",
];

pub fn writes(executable: &Path) -> Result<Vec<Write>> {
    let executable = executable.to_str().ok_or_else(|| anyhow::anyhow!("Files path is not Unicode"))?;
    ensure!(!executable.contains(['"', '\0']), "invalid Files executable path");
    let mut writes = Vec::new();
    for (index, key) in KEYS.into_iter().enumerate() {
        writes.push(Write { key, name: "", value: Value::String(if index < 2 {
            format!("\"{executable}\" \"%1\"")
        } else { format!("\"{executable}\" \"shell:profile\"") }) });
        writes.push(Write { key, name: "DelegateExecute", value: Value::String(String::new()) });
    }
    Ok(writes)
}

pub struct FolderRouting<S = Registry> { undo: Undo<S> }

impl FolderRouting {
    pub fn new() -> Result<Self> {
        Ok(Self::with(Registry, local_dir()?.join("undo-folders.json")))
    }
}

impl<S: Store> FolderRouting<S> {
    fn with(store: S, path: PathBuf) -> Self {
        Self { undo: Undo::new(store, path, Duration::ZERO) }
    }

    /// Enabling requires an installed executable; restoring works even after it is removed.
    pub fn set(&mut self, enabled: bool, executable: &Path) -> Result<bool> {
        let changed = if enabled {
            ensure!(executable.is_file(), "Files executable is missing: {}", executable.display());
            self.undo.apply(&writes(executable)?)?
        } else { self.undo.restore_if_unchanged()? };
        if changed { announce(); }
        Ok(changed)
    }
}

fn announce() {
    use windows::Win32::UI::Shell::{SHChangeNotify, SHCNE_ASSOCCHANGED, SHCNF_IDLIST};
    // SAFETY: association-change notification takes no item pointers.
    unsafe { SHChangeNotify(SHCNE_ASSOCCHANGED, SHCNF_IDLIST, None, None) };
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::undo::tests::{Memory, Scratch};

    #[test]
    fn restore_preserves_type_absence_and_foreign_changes_across_a_restart() {
        let scratch = Scratch::new("folder-routing");
        std::fs::create_dir_all(&scratch.0).unwrap();
        let executable = scratch.0.join("glaze-files.exe");
        std::fs::write(&executable, []).unwrap();
        let mut registry = Memory::default();
        registry.write(KEYS[0], "", &Value::ExpandString("%OLD%\\open.exe".into())).unwrap();
        registry.write(KEYS[0], "DelegateExecute", &Value::String("{original}".into())).unwrap();
        let journal = scratch.0.join("undo-folders.json");
        let mut first = FolderRouting::with(registry.clone(), journal.clone());
        assert!(first.set(true, &executable).unwrap());
        assert!(!first.set(true, &executable).unwrap());
        // Another app takes one command after us: disabling must leave it alone.
        registry.write(KEYS[1], "", &Value::String("foreign.exe".into())).unwrap();
        drop(first);
        let mut next = FolderRouting::with(registry.clone(), journal.clone());
        assert!(next.set(false, Path::new("missing.exe")).unwrap());
        assert_eq!(registry.read(KEYS[0], "").unwrap(), Some(Value::ExpandString("%OLD%\\open.exe".into())));
        assert_eq!(registry.read(KEYS[0], "DelegateExecute").unwrap(), Some(Value::String("{original}".into())));
        assert_eq!(registry.read(KEYS[1], "").unwrap(), Some(Value::String("foreign.exe".into())));
        assert_eq!(registry.read(KEYS[2], "").unwrap(), None);
        assert!(!registry.has_key(KEYS[2]));
        assert!(!journal.exists());
        assert!(!next.set(false, &executable).unwrap());
    }

    #[test]
    fn command_quotes_paths_and_placeholders_and_never_journals_a_missing_executable() {
        let plan = writes(Path::new(r"C:\Some App\glaze-files.exe")).unwrap();
        assert_eq!(plan.len(), 6);
        assert_eq!(plan[0].value, Value::String(r#""C:\Some App\glaze-files.exe" "%1""#.into()));
        let scratch = Scratch::new("folder-routing-missing");
        let journal = scratch.journal();
        let mut routing = FolderRouting::with(Memory::default(), journal.clone());
        assert!(routing.set(true, Path::new("does-not-exist.exe")).is_err());
        assert!(!journal.exists());
    }
}
