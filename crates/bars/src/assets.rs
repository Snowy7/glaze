//! Images `bars.json` names by path (a background picture, a module's icon file). The page can't
//! read files, so each is copied into `%LOCALAPPDATA%\mypaper\bars-assets\`, named after its
//! path and modification time, and served from `https://assets.bars.mypaper/`. The page gets a
//! map from the path as written to its URL.

use std::collections::BTreeMap;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::path::{Path, PathBuf};

/// The virtual host the copies are served from.
pub const ASSETS_HOST: &str = "assets.bars.mypaper";

const IMAGE_TYPES: &[&str] = &["svg", "png", "ico", "webp", "jpg", "jpeg", "gif", "bmp"];

/// Whether `text` names an image file (rather than an icon-pack key).
pub fn is_image_path(text: &str) -> bool {
    Path::new(text)
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| IMAGE_TYPES.contains(&e.to_ascii_lowercase().as_str()))
}

pub struct Assets {
    dir: PathBuf,
    urls: BTreeMap<String, String>,
}

impl Assets {
    pub fn new(dir: PathBuf) -> Self {
        Self {
            dir,
            urls: BTreeMap::new(),
        }
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// The URL of every path as written.
    pub fn urls(&self) -> &BTreeMap<String, String> {
        &self.urls
    }

    /// Serves exactly `paths` (relative ones from `theme`): copies what's new or changed and
    /// removes copies nobody names any more. Returns whether the map changed.
    pub fn sync<'a>(&mut self, paths: impl Iterator<Item = &'a str>, theme: Option<&Path>) -> bool {
        let _ = std::fs::create_dir_all(&self.dir);
        let mut urls = BTreeMap::new();
        for written in paths.filter(|p| is_image_path(p)) {
            let path = match theme {
                Some(theme) if Path::new(written).is_relative() => theme.join(written),
                _ => PathBuf::from(written),
            };
            match self.publish(&path) {
                Some(name) => {
                    urls.insert(written.to_owned(), format!("https://{ASSETS_HOST}/{name}"));
                }
                None => log::warn!("image {} isn't readable", path.display()),
            }
        }
        let keep: Vec<&str> = urls
            .values()
            .filter_map(|url| url.rsplit('/').next())
            .collect();
        for entry in std::fs::read_dir(&self.dir).into_iter().flatten().flatten() {
            let name = entry.file_name();
            if !keep.iter().any(|k| name.to_str() == Some(k)) {
                let _ = std::fs::remove_file(entry.path());
            }
        }
        let changed = urls != self.urls;
        self.urls = urls;
        changed
    }

    /// Copies `path` in (unless the same version is there) and returns the copy's file name.
    fn publish(&self, path: &Path) -> Option<String> {
        let modified = std::fs::metadata(path).ok()?.modified().ok()?;
        let extension = path.extension()?.to_str()?.to_ascii_lowercase();
        let mut hasher = DefaultHasher::new();
        path.hash(&mut hasher);
        modified.hash(&mut hasher);
        let name = format!("{:016x}.{extension}", hasher.finish());
        let copy = self.dir.join(&name);
        if !copy.is_file() {
            std::fs::copy(path, &copy).ok()?;
        }
        Some(name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn images_are_copied_served_and_cleaned_up() {
        let root = std::env::temp_dir().join(format!("mypaper-assets-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let theme = root.join("theme");
        std::fs::create_dir_all(&theme).unwrap();
        std::fs::write(theme.join("bg.png"), b"png").unwrap();
        let mut assets = Assets::new(root.join("served"));

        assert!(assets.sync(["bg.png", "start", "missing.svg"].into_iter(), Some(&theme)));
        let url = &assets.urls()["bg.png"];
        assert!(url.starts_with("https://assets.bars.mypaper/") && url.ends_with(".png"));
        assert_eq!(
            assets.urls().len(),
            1,
            "keys and missing files are left out"
        );
        assert!(
            !assets.sync(["bg.png"].into_iter(), Some(&theme)),
            "unchanged"
        );

        assert!(assets.sync(std::iter::empty(), Some(&theme)));
        assert_eq!(std::fs::read_dir(assets.dir()).unwrap().count(), 0);
        std::fs::remove_dir_all(&root).unwrap();
    }
}
