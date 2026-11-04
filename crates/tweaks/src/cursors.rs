//! Cursor schemes: a theme's `windows.cursors` becomes the values of
//! `HKCU\Control Panel\Cursors` (what the Mouse control panel writes),
//! journaled like the accent, and applied with `SystemParametersInfoW(SPI_SETCURSORS)`.
//!
//! A *set* is a folder of `.cur`/`.ani` files named after the cursor they
//! replace ([`Role::stems`]). `mypaper-dark` and `mypaper-light` are drawn in
//! code ([`cursor_art`](crate::cursor_art)) and written to
//! `%LOCALAPPDATA%\mypaper\cursors\<name>` when first used.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use windows::Win32::UI::WindowsAndMessaging::{SPI_SETCURSORS, SPIF_SENDCHANGE, SystemParametersInfoW};

use crate::cursor_art::{self, Look};
use crate::undo::{Value, Write};

pub(crate) const CURSORS_KEY: &str = r"Control Panel\Cursors";

/// Bumped when the built-in art changes, so existing installs regenerate.
const ART_VERSION: &str = "1";
const VERSION_FILE: &str = ".version";

/// One cursor of the scheme.
pub struct Role {
    /// The registry value that holds its path.
    pub value: &'static str,
    /// File names (without extension) that mean this cursor in a set folder,
    /// the first being the one we generate. Matching ignores case.
    pub stems: &'static [&'static str],
    /// Generated as an `.ani` (and an `.ani` is preferred over a `.cur` of the same name).
    pub animated: bool,
}

const fn role(value: &'static str, stems: &'static [&'static str], animated: bool) -> Role {
    Role {
        value,
        stems,
        animated,
    }
}

/// Every value of the cursor scheme. Stems follow the Windows `.inf` names
/// (`vert`, `horz`, `dgn1`, ...) and the registry names.
pub const ROLES: [Role; 17] = [
    role("Arrow", &["arrow", "pointer", "normal"], false),
    role("Help", &["help"], false),
    role("AppStarting", &["appstarting", "working"], true),
    role("Wait", &["wait", "busy"], true),
    role("Crosshair", &["crosshair", "cross", "precision"], false),
    role("IBeam", &["ibeam", "text"], false),
    role("NWPen", &["nwpen", "pen", "handwriting"], false),
    role("No", &["no", "unavailable"], false),
    role("SizeNS", &["sizens", "vert", "vertical"], false),
    role("SizeWE", &["sizewe", "horz", "horizontal"], false),
    role("SizeNWSE", &["sizenwse", "dgn1", "diagonal1"], false),
    role("SizeNESW", &["sizenesw", "dgn2", "diagonal2"], false),
    role("SizeAll", &["sizeall", "move"], false),
    role("UpArrow", &["uparrow", "alternate", "alt"], false),
    role("Hand", &["hand", "link"], false),
    role("Person", &["person"], false),
    role("Pin", &["pin", "location"], false),
];

/// A named set of cursor files: one path per cursor it covers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Scheme {
    pub name: String,
    pub files: Vec<(&'static str, PathBuf)>,
}

/// The registry values that select `scheme`. Cursors the set does not cover
/// keep the user's own. The default value is the scheme name (what the Mouse
/// control panel shows); "Scheme Source" 1 marks a user scheme.
pub fn writes(scheme: &Scheme) -> Vec<Write> {
    let mut writes: Vec<Write> = scheme
        .files
        .iter()
        .map(|(value, path)| Write {
            key: CURSORS_KEY,
            name: value,
            value: Value::ExpandString(path.to_string_lossy().into_owned()),
        })
        .collect();
    writes.push(Write {
        key: CURSORS_KEY,
        name: "",
        value: Value::String(scheme.name.clone()),
    });
    writes.push(Write {
        key: CURSORS_KEY,
        name: "Scheme Source",
        value: Value::Dword(1),
    });
    writes
}

/// Which file in a folder stands for which cursor. `names` are the folder's
/// file names; for each role, the first stem (in [`Role::stems`] order) with
/// a `.cur` or `.ani` wins, and animated roles prefer `.ani`.
fn match_files(names: &[String]) -> Vec<(&'static str, String)> {
    let find = |stem: &str, ext: &str| {
        names.iter().find(|name| {
            Path::new(name.as_str()).extension().is_some_and(|e| e.eq_ignore_ascii_case(ext))
                && Path::new(name.as_str())
                    .file_stem()
                    .is_some_and(|s| s.eq_ignore_ascii_case(stem))
        })
    };
    ROLES
        .iter()
        .filter_map(|role| {
            let order = if role.animated { ["ani", "cur"] } else { ["cur", "ani"] };
            role.stems
                .iter()
                .flat_map(|stem| order.map(|ext| (stem, ext)))
                .find_map(|(stem, ext)| find(stem, ext))
                .map(|file| (role.value, file.clone()))
        })
        .collect()
}

/// Reads a set folder. Fails when it holds no cursor files at all.
fn scan(name: &str, dir: &Path) -> Result<Scheme> {
    let names: Vec<String> = std::fs::read_dir(dir)
        .with_context(|| format!("reading cursor set {}", dir.display()))?
        .flatten()
        .filter_map(|entry| entry.file_name().into_string().ok())
        .collect();
    let files: Vec<_> = match_files(&names)
        .into_iter()
        .map(|(value, file)| (value, dir.join(file)))
        .collect();
    if files.is_empty() {
        bail!(
            "no cursor files (.cur / .ani named arrow, hand, ibeam, ...) in {}",
            dir.display()
        );
    }
    Ok(Scheme {
        name: name.to_owned(),
        files,
    })
}

/// Where a `windows.cursors` value may point, in order: an absolute folder; a
/// path with separators, relative to the theme; a bare name, in the user's
/// `cursors` folder and then in the theme folder.
fn candidates(spec: &str, theme_dir: &Path, user_dir: &Path) -> Vec<PathBuf> {
    let path = Path::new(spec);
    if path.is_absolute() {
        vec![path.to_owned()]
    } else if spec.contains(['\\', '/']) {
        vec![theme_dir.join(path)]
    } else {
        vec![user_dir.join(spec), theme_dir.join(spec)]
    }
}

/// Finds the set `spec` names (see [`candidates`]), or draws a built-in one
/// into `local_dir\cursors`. `user_dir` is `%APPDATA%\mypaper\cursors`.
pub fn resolve(spec: &str, theme_dir: &Path, user_dir: &Path, local_dir: &Path) -> Result<Scheme> {
    if let Some(look) = Look::from_name(spec) {
        let dir = install_builtin(look, &local_dir.join("cursors"))?;
        return scan(look.name(), &dir);
    }
    let dir = candidates(spec, theme_dir, user_dir)
        .into_iter()
        .find(|dir| dir.is_dir())
        .with_context(|| format!("cursor set {spec:?} not found"))?;
    let name = dir.file_name().map_or_else(|| spec.to_owned(), |n| n.to_string_lossy().into_owned());
    scan(&name, &dir)
}

/// [`resolve`] with the real folders: `%APPDATA%\mypaper\cursors` for the
/// user's sets, `%LOCALAPPDATA%\mypaper` for the drawn ones.
pub fn resolve_default(spec: &str, theme_dir: &Path) -> Result<Scheme> {
    let roaming = std::env::var_os("APPDATA").context("APPDATA is not set")?;
    let user_dir = PathBuf::from(roaming).join("mypaper").join("cursors");
    resolve(spec, theme_dir, &user_dir, &crate::palette::local_dir()?)
}

/// Writes a built-in set to `root\<name>` unless a current copy is there.
fn install_builtin(look: Look, root: &Path) -> Result<PathBuf> {
    let dir = root.join(look.name());
    let complete = std::fs::read_to_string(dir.join(VERSION_FILE)).is_ok_and(|v| v.trim() == ART_VERSION)
        && ROLES.iter().all(|role| dir.join(file_name(role)).is_file());
    if complete {
        return Ok(dir);
    }
    std::fs::create_dir_all(&dir).with_context(|| format!("creating {}", dir.display()))?;
    for role in &ROLES {
        let path = dir.join(file_name(role));
        std::fs::write(&path, cursor_art::render(look, role))
            .with_context(|| format!("writing {}", path.display()))?;
    }
    std::fs::write(dir.join(VERSION_FILE), ART_VERSION)?;
    Ok(dir)
}

/// The generated file for a role: its first stem, `.ani` or `.cur`.
fn file_name(role: &Role) -> String {
    format!("{}.{}", role.stems[0], if role.animated { "ani" } else { "cur" })
}

/// Writes every built-in set into `root` (`cargo run -p mypaper-tweaks --example export_cursors`).
pub fn export_builtin(root: &Path) -> Result<Vec<PathBuf>> {
    [Look::Dark, Look::Light]
        .into_iter()
        .map(|look| install_builtin(look, root))
        .collect()
}

/// Makes Windows reload the cursors from the registry.
pub fn reload() {
    // SAFETY: SPI_SETCURSORS takes no parameters; the call reads the registry.
    if let Err(e) = unsafe { SystemParametersInfoW(SPI_SETCURSORS, 0, None, SPIF_SENDCHANGE) } {
        log::warn!("cursors not reloaded: {e}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::undo::Undo;
    use crate::undo::tests::{Memory, Scratch};
    use crate::undo::Store;
    use std::time::Duration;

    fn names(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| (*s).to_owned()).collect()
    }

    #[test]
    fn files_match_roles_by_stem_ignoring_case_and_prefer_ani_for_busy_cursors() {
        let found = match_files(&names(&[
            "Arrow.CUR", "hand.cur", "vert.cur", "dgn1.cur", "wait.cur", "wait.ani", "readme.txt", "ibeam.png",
        ]));
        let get = |value: &str| found.iter().find(|(v, _)| *v == value).map(|(_, f)| f.as_str());
        assert_eq!(get("Arrow"), Some("Arrow.CUR"));
        assert_eq!(get("Hand"), Some("hand.cur"));
        assert_eq!(get("SizeNS"), Some("vert.cur"), "inf-style names count");
        assert_eq!(get("SizeNWSE"), Some("dgn1.cur"));
        assert_eq!(get("Wait"), Some("wait.ani"), "animated beats static");
        assert_eq!(get("IBeam"), None, "only .cur and .ani are cursors");
        assert_eq!(found.len(), 5);
        assert!(match_files(&names(&["notes.txt"])).is_empty());
    }

    #[test]
    fn roles_cover_the_scheme_once_with_unique_generated_names() {
        let mut files: Vec<String> = ROLES.iter().map(file_name).collect();
        files.sort();
        files.dedup();
        assert_eq!(files.len(), ROLES.len());
        // Generated file names are found again by the matcher.
        let found = match_files(&ROLES.iter().map(file_name).collect::<Vec<_>>());
        assert_eq!(found.len(), ROLES.len());
    }

    #[test]
    fn a_spec_resolves_absolute_then_theme_relative_then_by_name() {
        let (theme, user) = (Path::new(r"C:\themes\t"), Path::new(r"C:\Users\me\mypaper\cursors"));
        assert_eq!(candidates(r"D:\sets\mine", theme, user), [PathBuf::from(r"D:\sets\mine")]);
        assert_eq!(candidates(r"cursors\mine", theme, user), [theme.join(r"cursors\mine")]);
        assert_eq!(candidates("mine", theme, user), [user.join("mine"), theme.join("mine")]);
    }

    #[test]
    fn writes_select_the_scheme_and_keep_registry_types() {
        let scheme = Scheme {
            name: "mypaper-dark".into(),
            files: vec![("Arrow", PathBuf::from(r"C:\c\arrow.cur")), ("Wait", PathBuf::from(r"C:\c\wait.ani"))],
        };
        let plan = writes(&scheme);
        let by_name = |name: &str| plan.iter().find(|w| w.name == name).map(|w| &w.value);
        assert_eq!(by_name("Arrow"), Some(&Value::ExpandString(r"C:\c\arrow.cur".into())));
        assert_eq!(by_name(""), Some(&Value::String("mypaper-dark".into())));
        assert_eq!(by_name("Scheme Source"), Some(&Value::Dword(1)));
        assert_eq!(by_name("Hand"), None, "cursors the set lacks stay as they are");
        assert!(plan.iter().all(|w| w.key == CURSORS_KEY));
    }

    #[test]
    fn applying_and_restoring_a_scheme_brings_back_the_exact_original_values() {
        let scratch = Scratch::new("cursor-journal");
        let mut registry = Memory::default();
        let originals = [
            ("Arrow", Value::ExpandString(r"C:\WINDOWS\Cursors\old\arrow.cur".into())),
            ("", Value::String("Old scheme".into())),
            ("Scheme Source", Value::Dword(1)),
            ("CursorBaseSize", Value::Dword(32)), // not ours: must never change
        ];
        for (name, value) in &originals {
            registry.write(CURSORS_KEY, name, value).unwrap();
        }
        let scheme = Scheme {
            name: "mypaper-light".into(),
            files: vec![
                ("Arrow", PathBuf::from(r"C:\c\arrow.cur")),
                ("Hand", PathBuf::from(r"C:\c\hand.cur")), // absent before
            ],
        };
        let mut undo = Undo::new(registry.clone(), scratch.journal(), Duration::ZERO);
        assert!(undo.apply(&writes(&scheme)).unwrap());
        assert_eq!(registry.read(CURSORS_KEY, "").unwrap(), Some(Value::String("mypaper-light".into())));
        assert!(undo.journaled_key(CURSORS_KEY));

        assert!(undo.restore(|| {}).unwrap());
        for (name, value) in &originals {
            assert_eq!(registry.read(CURSORS_KEY, name).unwrap().as_ref(), Some(value), "{name}");
        }
        assert_eq!(registry.read(CURSORS_KEY, "Hand").unwrap(), None);
        assert!(registry.has_key(r"Control Panel"), "the scheme key itself is never removed");
    }

    #[test]
    fn built_in_sets_are_drawn_once_and_found_again() {
        let root = Scratch::new("cursor-builtin");
        let local = root.0.join("local");
        let scheme = resolve("mypaper-dark", Path::new("."), &root.0.join("user"), &local).unwrap();
        assert_eq!(scheme.name, "mypaper-dark");
        assert_eq!(scheme.files.len(), ROLES.len());
        assert!(scheme.files.iter().all(|(_, path)| path.is_file()));
        let wait = scheme.files.iter().find(|(v, _)| *v == "Wait").unwrap();
        assert_eq!(wait.1.extension().unwrap(), "ani");

        // A second resolve reuses the files instead of drawing again.
        let arrow = &scheme.files[0].1;
        let stamp = std::fs::metadata(arrow).unwrap().modified().unwrap();
        resolve("MyPaper-Dark", Path::new("."), &root.0.join("user"), &local).unwrap();
        assert_eq!(std::fs::metadata(arrow).unwrap().modified().unwrap(), stamp);

        // A user folder works too, and an empty or missing one is an error.
        let mine = root.0.join("user").join("mine");
        std::fs::create_dir_all(&mine).unwrap();
        assert!(resolve("mine", Path::new("."), &root.0.join("user"), &local).is_err());
        std::fs::write(mine.join("arrow.cur"), b"x").unwrap();
        let user = resolve("mine", Path::new("."), &root.0.join("user"), &local).unwrap();
        assert_eq!((user.name.as_str(), user.files.len()), ("mine", 1));
        assert!(resolve("nope", Path::new("."), &root.0.join("user"), &local).is_err());
    }
}
