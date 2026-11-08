//! Package staging, consent, cleanup, and Studio's plugin control messages.
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Component, Path, PathBuf};
use std::sync::OnceLock;

use anyhow::{Context, Result, bail, ensure};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{
    files,
    manifest::{PluginManifest, valid_id},
    wake::{Msg, Waker},
};

const MAX_BYTES: u64 = 128 << 20;
pub const STUDIO: &str = "@studio-plugins";
pub const COPY_TAG: usize = 0x4d50504c;
static WAKER: OnceLock<Waker> = OnceLock::new();

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Preferences {
    #[serde(default)]
    pub disabled: BTreeSet<String>,
    #[serde(default)]
    pub uninstalled: BTreeSet<String>,
    #[serde(default)]
    pub settings: BTreeMap<String, Value>,
}
impl Preferences {
    pub fn load() -> Self {
        files::user("plugin-manager.json")
            .ok()
            .and_then(|p| std::fs::read(p).ok())
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default()
    }
    pub fn save(&self) -> Result<()> {
        files::write_atomic(
            &files::user("plugin-manager.json")?,
            &serde_json::to_vec_pretty(self)?,
        )
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct Metrics {
    #[serde(default)]
    pub measured: bool,
    pub memory: u64,
    pub cpu_us: u64,
    pub events: u64,
    pub status: String,
}
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Snapshot {
    pub plugins: BTreeMap<String, Metrics>,
    pub error: Option<String>,
}
impl Snapshot {
    pub fn load() -> Self {
        files::local("plugins/studio.json")
            .ok()
            .and_then(|p| std::fs::read(p).ok())
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default()
    }
    pub fn publish(&self) {
        if let Ok(path) = files::local("plugins/studio.json")
            && let Ok(bytes) = serde_json::to_vec(self)
            && let Err(error) = files::write_atomic(&path, &bytes)
        {
            log::warn!("Studio plugin stats: {error:#}");
        }
    }
    pub fn route(&mut self, message: &glaze_plugin_api::wire::HelperMsg) -> bool {
        use glaze_plugin_api::wire::{HelperMsg, Status};
        match message {
            HelperMsg::Stats {
                plugin,
                memory,
                cpu_us,
                events,
            } => {
                let m = self.plugins.entry(plugin.clone()).or_default();
                m.measured = true;
                m.memory = *memory;
                m.cpu_us = *cpu_us;
                m.events = *events;
            }
            HelperMsg::Status { plugin, status } => {
                let m = self.plugins.entry(plugin.clone()).or_default();
                m.status = match status {
                    Status::Live => "Live".into(),
                    Status::Unloaded => "Unloaded".into(),
                    Status::Restarting(why) => format!("Restarting: {why}"),
                    Status::Stopped(why) => format!("Stopped: {why}"),
                };
                if !matches!(status, Status::Live) {
                    m.memory = 0;
                }
            }
            _ => return false,
        }
        true
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Request {
    Refresh,
    Enable(String),
    Disable(String),
    Uninstall(String),
    Settings(String, Value),
}
pub fn connect(waker: Waker) {
    let _ = WAKER.set(waker);
}
/// Called by the shell's existing message-only widget control window.
pub fn receive(bytes: &[u8]) -> bool {
    if bytes.len() > 64 << 10 || serde_json::from_slice::<Request>(bytes).is_err() {
        return false;
    }
    let Some(waker) = WAKER.get() else {
        return false;
    };
    waker.send(Msg::Plugin {
        id: STUDIO.into(),
        generation: 0,
        line: Some(String::from_utf8_lossy(bytes).into_owned()),
    });
    true
}
/// Extends the same message-only host Studio uses for widget arrangement. No input or focus.
pub fn send(request: &Request) -> Result<()> {
    use windows::Win32::{
        Foundation::{HWND, LPARAM, WPARAM},
        System::DataExchange::COPYDATASTRUCT,
        UI::WindowsAndMessaging::*,
    };
    use windows::core::{PCWSTR, w};
    let bytes = serde_json::to_vec(request)?;
    let data = COPYDATASTRUCT {
        dwData: COPY_TAG,
        cbData: bytes.len() as u32,
        lpData: bytes.as_ptr().cast_mut().cast(),
    };
    // SAFETY: the payload lives throughout the synchronous copy; only our message-only host is targeted.
    unsafe {
        let host = FindWindowExW(
            Some(HWND_MESSAGE),
            None::<HWND>,
            w!("glaze.widgets.host"),
            PCWSTR::null(),
        )
        .context("Glaze shell is not running")?;
        let mut result = 0;
        ensure!(
            SendMessageTimeoutW(
                host,
                WM_COPYDATA,
                WPARAM(0),
                LPARAM(&data as *const _ as isize),
                SMTO_ABORTIFHUNG,
                500,
                Some(&mut result)
            )
            .0 != 0
                && result == 1,
            "The shell could not accept the plugin request. Restart Glaze to update it."
        );
    }
    Ok(())
}

/// A private immutable copy: review and install always refer to the same bytes.
pub struct Package {
    root: PathBuf,
    pub manifest: PluginManifest,
}
impl Drop for Package {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}
impl Package {
    pub fn prepare(source: &Path) -> Result<Self> {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_nanos();
        let root =
            std::env::temp_dir().join(format!("glaze-install-{}-{nonce}", std::process::id()));
        std::fs::create_dir(&root)?;
        let result = (|| {
            if source.is_dir() {
                let mut total = 0;
                copy_tree(source, &root, &mut total)?;
            } else {
                ensure!(
                    source
                        .extension()
                        .is_some_and(|e| e.eq_ignore_ascii_case("glaze-plugin")),
                    "Choose a .glaze-plugin package or a plugin folder"
                );
                let mut zip = zip::ZipArchive::new(std::fs::File::open(source)?)
                    .context("The package is not a ZIP archive")?;
                ensure!(zip.len() <= 4096, "Package has too many files");
                let mut total = 0u64;
                let mut seen = BTreeSet::new();
                for i in 0..zip.len() {
                    let mut entry = zip.by_index(i)?;
                    let name = entry.name().replace('\\', "/");
                    let path = safe_path(name.trim_end_matches('/'))?;
                    ensure!(
                        seen.insert(name.to_lowercase()),
                        "Duplicate package path: {name}"
                    );
                    ensure!(
                        entry.unix_mode().is_none_or(|m| m & 0o170000 != 0o120000),
                        "Package symlinks are not supported"
                    );
                    total = total
                        .checked_add(entry.size())
                        .context("Package is too large")?;
                    ensure!(total <= MAX_BYTES, "Package exceeds 128 MiB");
                    let target = root.join(path);
                    if entry.is_dir() {
                        std::fs::create_dir_all(target)?;
                    } else {
                        std::fs::create_dir_all(target.parent().context("package path")?)?;
                        let mut file = std::fs::OpenOptions::new()
                            .write(true)
                            .create_new(true)
                            .open(target)?;
                        std::io::copy(
                            &mut std::io::Read::take(&mut entry, MAX_BYTES + 1),
                            &mut file,
                        )?;
                    }
                }
            }
            let text = std::fs::read_to_string(root.join("plugin.json"))
                .context("Package needs plugin.json at its root")?;
            let value: Value = serde_json::from_str(&text)?;
            let id = value["id"].as_str().context("plugin.json needs an id")?;
            let manifest = PluginManifest::parse(&text, id)?;
            let component = manifest
                .component
                .as_ref()
                .context("Install packages must contain a WebAssembly component")?;
            compatible_api(manifest.api.as_deref().unwrap_or(""))?;
            ensure!(
                root.join(component).is_file(),
                "Missing component: {component}"
            );
            for module in &manifest.native {
                let path = safe_path(module)?;
                ensure!(root.join(path).is_file(), "Missing native module: {module}");
            }
            for permission in &manifest.permissions {
                permission_text(permission)?;
            }
            resolve_settings(&manifest, &serde_json::json!({}))?;
            ensure!(
                manifest.native.is_empty()
                    || manifest.permissions.iter().any(|p| p == "native-code"),
                "Native modules require the native-code permission"
            );
            Ok(manifest)
        })();
        match result {
            Ok(manifest) => Ok(Self { root, manifest }),
            Err(e) => {
                let _ = std::fs::remove_dir_all(root);
                Err(e)
            }
        }
    }
    /// No install directories or compiler processes are created before explicit consent.
    pub fn install(
        &self,
        consent: bool,
        plugins: &Path,
        compile: impl FnOnce(&Path) -> Result<()>,
    ) -> Result<PathBuf> {
        ensure!(
            consent,
            "Permission consent is required before installing this plugin"
        );
        let target = plugins.join(&self.manifest.id);
        ensure!(
            !target.exists(),
            "{} is already installed; uninstall it before installing another version",
            self.manifest.name
        );
        std::fs::create_dir_all(plugins)?;
        std::fs::create_dir(&target)?;
        let result = (|| {
            let mut total = 0;
            copy_tree(&self.root, &target, &mut total)?;
            compile(&target)
        })();
        if let Err(e) = result {
            let _ = std::fs::remove_dir_all(&target);
            return Err(e);
        }
        Ok(target)
    }
}
fn safe_path(name: &str) -> Result<&Path> {
    let path = Path::new(name);
    ensure!(
        !name.is_empty()
            && !name.contains([':', '\\'])
            && !name.split('/').any(|n| n.ends_with(['.', ' ']))
            && path.components().all(|c| matches!(c, Component::Normal(_))),
        "Unsafe package path: {name:?}"
    );
    Ok(path)
}
fn copy_tree(from: &Path, to: &Path, total: &mut u64) -> Result<()> {
    for entry in std::fs::read_dir(from)? {
        let entry = entry?;
        let meta = entry.path().symlink_metadata()?;
        ensure!(
            !meta.file_type().is_symlink() && meta.file_attributes() & 0x400 == 0,
            "Plugin folders cannot contain links or junctions"
        );
        let target = to.join(entry.file_name());
        if meta.is_dir() {
            std::fs::create_dir(&target)?;
            copy_tree(&entry.path(), &target, total)?;
        } else {
            *total += meta.len();
            ensure!(*total <= MAX_BYTES, "Package exceeds 128 MiB");
            std::fs::copy(entry.path(), target)?;
        }
    }
    Ok(())
}
use std::os::windows::fs::MetadataExt;

pub fn compatible_api(api: &str) -> Result<()> {
    let version = api
        .split_once('.')
        .and_then(|(a, b)| Some((a.parse::<u32>().ok()?, b.parse::<u32>().ok()?)));
    ensure!(
        version.is_some_and(
            |(a, b)| a == glaze_plugin_api::API_MAJOR && b <= glaze_plugin_api::API_MINOR
        ),
        "Incompatible plugin SDK/API {api:?}. This Glaze supports {}.{}; update Glaze or choose a compatible plugin.",
        glaze_plugin_api::API_MAJOR,
        glaze_plugin_api::API_MINOR
    );
    Ok(())
}
/// Fill schema defaults and validate saved values at the shell boundary.
pub fn resolve_settings(manifest: &PluginManifest, saved: &Value) -> Result<Value> {
    let saved = saved
        .as_object()
        .context("Plugin settings must be an object")?;
    let mut resolved = serde_json::Map::new();
    for schema in &manifest.settings {
        let setting: crate::manifest::Setting =
            serde_json::from_value(schema.clone()).context("Invalid plugin settings schema")?;
        setting.validate()?;
        let value = match saved.get(setting.key()) {
            Some(value) => setting
                .fit(value)
                .with_context(|| format!("Invalid value for setting {}", setting.key()))?,
            None => setting.default_value(),
        };
        ensure!(
            resolved.insert(setting.key().into(), value).is_none(),
            "Duplicate plugin setting: {}",
            setting.key()
        );
    }
    ensure!(
        saved.keys().all(|key| resolved.contains_key(key)),
        "Unknown plugin setting"
    );
    Ok(Value::Object(resolved))
}
pub fn permission_text(permission: &str) -> Result<String> {
    if let Some(key) = permission.strip_prefix("network:https://{settings.").and_then(|p| p.strip_suffix("[].url}")) {
        ensure!(!key.is_empty() && key.bytes().all(|c| c.is_ascii_alphanumeric() || b"_-".contains(&c)), "Invalid network URL-list setting");
        return Ok(format!("Can send HTTPS requests to hosts configured in {key}"));
    }
    if let Some(key) = permission.strip_prefix("network:ws://127.0.0.1:{settings.").and_then(|p| p.strip_suffix('}')) {
        ensure!(!key.is_empty() && key.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'_' || c == b'-'), "Invalid network port setting");
        return Ok(format!("Can connect to 127.0.0.1 using the configured {key}"));
    }
    if let Some(origin) = permission.strip_prefix("network:") {
        ensure!(
            crate::net::Origin::parse(origin).is_some(),
            "Invalid network permission: {permission}"
        );
        return Ok(format!("Can connect to {origin}"));
    }
    if let Some(name) = permission.strip_prefix("process:") {
        ensure!(!name.is_empty() && name.bytes().all(|c| c.is_ascii_alphanumeric() || b"-_.".contains(&c)), "Invalid process permission");
        return Ok(format!("Can run {name} on this PC and read its output"));
    }
    if let Some(path) = permission.strip_prefix("file:") {
        ensure!(path.starts_with("~/") && !path.contains(".."), "Invalid file permission");
        return Ok(format!("Can read {path}"));
    }
    if let Some(pipe) = permission.strip_prefix("pipe:") {
        ensure!(
            !pipe.is_empty() && !pipe.contains(['/', '\\']),
            "Invalid pipe permission"
        );
        return Ok(format!("Can connect to the local pipe {pipe}"));
    }
    Ok(match permission {
        "storage" => "Can save data in its own storage",
        "secrets" => "Can store secrets in its own Credential Manager namespace",
        "windows" => "Can read window titles and the foreground app",
        "launch" => "Can open apps, files and links",
        "notifications" => "Can show notifications",
        "native-code" => "Runs native code outside the WebAssembly sandbox",
        "desktop-menu" => "Adds items to the desktop menu",
        "shortcuts" => "Registers global keyboard shortcuts",
        "location" => "Can request your location",
        _ => bail!("Unknown permission: {permission}"),
    }
    .into())
}
/// Only user-installed plugin files are removed; bundled copies are disabled separately.
pub fn uninstall(id: &str) -> Result<()> {
    cleanup(
        id,
        &files::user("plugins")?,
        &files::local("plugins/cache")?,
        &files::user("plugin-data")?,
        remove_secrets,
    )
}
fn cleanup(
    id: &str,
    plugins: &Path,
    cache: &Path,
    data: &Path,
    secrets: impl FnOnce(&str) -> Result<()>,
) -> Result<()> {
    ensure!(valid_id(id), "Invalid plugin id");
    secrets(id)?;
    for path in [plugins.join(id), data.join(id)] {
        if path.exists() {
            std::fs::remove_dir_all(path)?;
        }
    }
    if cache.is_dir() {
        for entry in std::fs::read_dir(cache)? {
            let entry = entry?;
            let name = entry.file_name().to_string_lossy().into_owned();
            let ours = name
                .strip_prefix(&format!("{id}-"))
                .and_then(|s| s.strip_suffix(".cwasm"))
                .is_some_and(|s| !s.is_empty() && s.chars().all(|c| c.is_ascii_hexdigit()));
            if ours && entry.file_type()?.is_file() {
                std::fs::remove_file(entry.path())?;
            }
        }
    }
    Ok(())
}
fn remove_secrets(id: &str) -> Result<()> {
    use windows::Win32::{Foundation::ERROR_NOT_FOUND, Security::Credentials::*};
    use windows::core::PCWSTR;
    let prefix = format!("glaze/integrations/plugin/{id}/");
    let filter: Vec<u16> = format!("{prefix}*\0").encode_utf16().collect();
    let mut count = 0;
    let mut credentials = std::ptr::null_mut();
    // SAFETY: Windows owns this returned array and its strings until CredFree below.
    unsafe {
        if let Err(e) = CredEnumerateW(PCWSTR(filter.as_ptr()), None, &mut count, &mut credentials)
        {
            if e.code() == windows::core::HRESULT::from_win32(ERROR_NOT_FOUND.0) {
                return Ok(());
            }
            return Err(e.into());
        }
        let result = (|| {
            for credential in std::slice::from_raw_parts(credentials, count as usize) {
                let c = &**credential;
                if c.TargetName.to_string()?.starts_with(&prefix) {
                    CredDeleteW(PCWSTR(c.TargetName.0), c.Type, None)?;
                }
            }
            Ok(())
        })();
        CredFree(credentials.cast());
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn folder_parse_sdk_and_consent() {
        let dir = files::temp_dir("package-consent");
        let source = dir.join("source");
        std::fs::create_dir(&source).unwrap();
        std::fs::write(source.join("plugin.json"), r#"{"id":"test","name":"Test","author":"A","component":"test.wasm","api":"0.1","permissions":["storage"]}"#).unwrap();
        std::fs::write(source.join("test.wasm"), b"wasm").unwrap();
        let package = Package::prepare(&source).unwrap();
        std::fs::write(source.join("test.wasm"), b"changed after review").unwrap();
        assert_eq!(package.manifest.author.as_deref(), Some("A"));
        assert!(
            package
                .install(false, &dir.join("installed"), |_| panic!(
                    "compile before consent"
                ))
                .is_err()
        );
        assert!(!dir.join("installed").exists());
        assert!(
            package
                .install(true, &dir.join("installed"), |_| Ok(()))
                .unwrap()
                .join("test.wasm")
                .is_file()
        );
        assert_eq!(std::fs::read(dir.join("installed/test/test.wasm")).unwrap(), b"wasm", "installation uses the reviewed copy");
        assert!(compatible_api("9.0").is_err());
        assert!(compatible_api("0.2").is_err());
        let failed = dir.join("failed");
        assert!(
            package
                .install(true, &failed, |_| bail!("Compiler refused the component"))
                .is_err()
        );
        assert!(
            !failed.join("test").exists(),
            "failed compilation rolls back the installed copy"
        );
        std::fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn zip_parse_and_traversal() {
        use std::io::Write;
        let dir = files::temp_dir("package-zip");
        for (filename, bad) in [
            ("valid.glaze-plugin", false),
            ("bad.glaze-plugin", true),
        ] {
            let path = dir.join(filename);
            let mut zip = zip::ZipWriter::new(std::fs::File::create(&path).unwrap());
            let options = zip::write::SimpleFileOptions::default();
            zip.start_file("plugin.json", options).unwrap();
            zip.write_all(br#"{"id":"test","name":"Test","component":"test.wasm","api":"0.1"}"#)
                .unwrap();
            zip.start_file(if bad { "../test.wasm" } else { "test.wasm" }, options)
                .unwrap();
            zip.write_all(b"wasm").unwrap();
            zip.finish().unwrap();
            assert_eq!(Package::prepare(&path).is_err(), bad);
        }
        std::fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn uninstall_cleans_only_owned_namespace() {
        let dir = files::temp_dir("uninstall");
        for p in ["plugins/test", "data/test/kv", "cache", "data/other"] {
            std::fs::create_dir_all(dir.join(p)).unwrap();
        }
        for p in [
            "cache/test-ab12.cwasm",
            "cache/test-more-ab12.cwasm",
            "cache/other-ab12.cwasm",
            "data/test/state.bin",
        ] {
            std::fs::write(dir.join(p), b"x").unwrap();
        }
        let mut called = false;
        cleanup(
            "test",
            &dir.join("plugins"),
            &dir.join("cache"),
            &dir.join("data"),
            |id| {
                assert_eq!(id, "test");
                called = true;
                Ok(())
            },
        )
        .unwrap();
        assert!(called);
        assert!(!dir.join("plugins/test").exists());
        assert!(!dir.join("data/test").exists());
        assert!(!dir.join("cache/test-ab12.cwasm").exists());
        assert!(dir.join("cache/test-more-ab12.cwasm").exists());
        assert!(dir.join("data/other").exists());
        std::fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn stats_route_preserves_all_counters_and_status_reason() {
        use glaze_plugin_api::wire::{HelperMsg, Status};
        let mut s = Snapshot::default();
        s.route(&HelperMsg::Stats {
            plugin: "test".into(),
            memory: 123,
            cpu_us: 456,
            events: 7,
        });
        s.route(&HelperMsg::Status {
            plugin: "test".into(),
            status: Status::Stopped("CPU budget".into()),
        });
        assert_eq!(
            s.plugins["test"],
            Metrics {
                measured: true,
                memory: 0,
                cpu_us: 456,
                events: 7,
                status: "Stopped: CPU budget".into()
            }
        );
        let decoded: Snapshot = serde_json::from_slice(&serde_json::to_vec(&s).unwrap()).unwrap();
        assert_eq!(decoded.plugins, s.plugins);
    }

    #[test]
    fn cleanup_removes_the_real_secret_namespace_without_touching_a_neighbor() {
        let id = format!("cleanup-{}", std::process::id());
        let neighbor = format!("{id}-other");
        for key in [
            format!("plugin/{id}/a"),
            format!("plugin/{id}/nested/b"),
            format!("plugin/{neighbor}/a"),
        ] {
            glaze_integrations::credentials::write(&key, "test-only-secret").unwrap();
        }
        remove_secrets(&id).unwrap();
        assert!(glaze_integrations::credentials::read(&format!("plugin/{id}/a")).is_err());
        assert!(glaze_integrations::credentials::read(&format!("plugin/{id}/nested/b")).is_err());
        let key = format!("plugin/{neighbor}/a");
        assert!(glaze_integrations::credentials::read(&key).is_ok());
        glaze_integrations::credentials::remove(&key).unwrap();
    }

    #[test]
    fn settings_follow_the_manifest_schema() {
        let manifest=PluginManifest::parse(r#"{"id":"x","name":"X","component":"x.wasm","api":"0.1","settings":[{"key":"live","type":"boolean","label":"Live","default":true},{"key":"count","type":"number","label":"Count","min":1,"max":5,"default":2}]}"#,"x").unwrap();
        assert_eq!(
            resolve_settings(&manifest, &serde_json::json!({})).unwrap(),
            serde_json::json!({"live":true,"count":2.0})
        );
        assert!(resolve_settings(&manifest, &serde_json::json!({"live":"yes"})).is_err());
        assert!(resolve_settings(&manifest, &serde_json::json!({"count":6})).is_err());
    }
}
