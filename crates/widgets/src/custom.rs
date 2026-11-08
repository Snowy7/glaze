//! Declarative native widgets. Each JSON file is a catalog entry; instances share no mutable state.
use anyhow::{Context, Result, bail, ensure};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    io::Read,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

pub const LIMIT: usize = 256 * 1024;
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Template {
    Stat,
    List,
    Text,
    Image,
    Gauge,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum Source {
    Static {
        value: Value,
    },
    Url {
        url: String,
        #[serde(default = "root_path")]
        path: String,
    },
    File {
        path: String,
        #[serde(default)]
        json_path: Option<String>,
    },
    Command {
        program: String,
        #[serde(default)]
        args: Vec<String>,
        #[serde(default)]
        json_path: Option<String>,
    },
}
fn root_path() -> String {
    "$".into()
}
fn interval() -> u64 {
    300
}
fn widget_size() -> [u32; 2] {
    [3, 2]
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Definition {
    pub title: String,
    pub layout: Template,
    pub source: Source,
    /// Zero means manual refresh. Work runs only while at least one instance is visible.
    #[serde(default = "interval")]
    pub refresh_seconds: u64,
    #[serde(default = "widget_size")]
    pub size: [u32; 2],
    #[serde(default)]
    pub suffix: String,
    #[serde(default)]
    pub caption: String,
}
impl Definition {
    pub fn parse(text: &str) -> Result<Self> {
        ensure!(
            text.len() <= LIMIT,
            "Custom widget definition exceeds 256 KiB"
        );
        let definition: Self = serde_json::from_str(text)?;
        ensure!(
            !definition.title.trim().is_empty() && definition.title.chars().count() <= 80,
            "Widget title must contain 1–80 characters"
        );
        ensure!(
            definition.refresh_seconds == 0 || (10..=86400).contains(&definition.refresh_seconds),
            "Refresh must be 0 (manual) or 10–86400 seconds"
        );
        ensure!(
            definition.size.into_iter().all(|n| (2..=12).contains(&n)),
            "Widget size must be 2–12 cells"
        );
        match &definition.source {
            Source::Url { url, path } => {
                ensure!(
                    url.starts_with("https://")
                        && crate::net::Origin::of_url(url).is_some()
                        && !url.split('/').take(3).any(|s| s.contains('@')),
                    "Custom URLs must use HTTPS without embedded credentials"
                );
                select(&Value::Null, path)?;
            }
            Source::File { path, json_path } => {
                ensure!(!path.is_empty(), "File path is empty");
                if let Some(path) = json_path {
                    select(&Value::Null, path)?;
                }
            }
            Source::Command {
                program,
                args,
                json_path,
            } => {
                ensure!(
                    !program.is_empty() && args.len() <= 64,
                    "Command needs an executable and at most 64 arguments"
                );
                if let Some(path) = json_path {
                    select(&Value::Null, path)?;
                }
            }
            _ => {}
        }
        Ok(definition)
    }
    pub fn manifest(&self, id: &str) -> Result<crate::manifest::WidgetManifest> {
        let permissions = match &self.source {
            Source::Url { url, .. } => vec![format!(
                "network:{}",
                crate::net::Origin::of_url(url).context("Invalid URL")?
            )],
            _ => Vec::new(),
        };
        crate::manifest::WidgetManifest::parse(&json!({"id":id,"name":self.title,"category":"Custom","version":"1.0.0","author":"You","description":if self.caption.is_empty(){"Your custom native widget"}else{&self.caption},"size":{"default":self.size,"min":[2,2],"max":[12,12]},"instances":"multiple","permissions":permissions,"settings":[]}).to_string(),id)
    }
}
pub fn folder() -> Result<PathBuf> {
    crate::files::user("widgets")
}
pub fn path(id: &str) -> Result<PathBuf> {
    let stem = id
        .strip_prefix("custom-")
        .context("Expected a custom widget ID")?;
    ensure!(crate::manifest::valid_id(stem), "Invalid custom widget ID");
    Ok(folder()?.join(format!("{stem}.json")))
}
pub fn load(id: &str) -> Result<Definition> {
    Definition::parse(&String::from_utf8(read_file(&path(id)?)?)?)
}

/// Does not execute commands or fetch data. Used by both the desktop picker and Studio.
pub fn catalog() -> Vec<(String, Definition)> {
    let Ok(folder) = folder() else {
        return Vec::new();
    };
    let Ok(entries) = std::fs::read_dir(folder) else {
        return Vec::new();
    };
    let mut definitions = Vec::new();
    for entry in entries.flatten().take(512) {
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) != Some("json") {
            continue;
        }
        let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else {
            continue;
        };
        let id = format!("custom-{stem}");
        if !crate::manifest::valid_id(&id) {
            continue;
        }
        match load(&id) {
            Ok(definition) => definitions.push((id, definition)),
            Err(e) => log::warn!("Custom widget {}: {e}", path.display()),
        }
    }
    definitions.sort_by(|a, b| a.0.cmp(&b.0));
    definitions
}

#[derive(Debug)]
enum Segment {
    Key(String),
    Index(usize),
    All,
}
fn segments(path: &str) -> Result<Vec<Segment>> {
    let mut rest = path
        .strip_prefix('$')
        .context("JSONPath must start with $")?;
    let mut segments = Vec::new();
    while !rest.is_empty() {
        if let Some(after) = rest.strip_prefix('.') {
            let end = after.find(['.', '[']).unwrap_or(after.len());
            let key = &after[..end];
            ensure!(
                !key.is_empty(),
                "Recursive JSONPath is unsupported; use $.key[index] or [*]"
            );
            segments.push(if key == "*" {
                Segment::All
            } else {
                Segment::Key(key.into())
            });
            rest = &after[end..];
        } else if let Some(after) = rest.strip_prefix('[') {
            let end = after.find(']').context("Unclosed JSONPath bracket")?;
            let item = &after[..end];
            segments.push(if item == "*" {
                Segment::All
            } else if item.starts_with('"') {
                Segment::Key(serde_json::from_str(item)?)
            } else if item.starts_with('\'') && item.ends_with('\'') && item.len() >= 2 {
                Segment::Key(item[1..item.len() - 1].into())
            } else {
                Segment::Index(
                    item.parse()
                        .context("Expected a JSONPath index, quoted key or *")?,
                )
            });
            rest = &after[end + 1..];
        } else {
            bail!("Unsupported JSONPath; use $.key[index] or [*]");
        }
        ensure!(segments.len() <= 32, "JSONPath exceeds 32 levels");
    }
    Ok(segments)
}
/// A deliberately small JSONPath subset: keys, quoted keys, indices and wildcards.
pub fn select(value: &Value, path: &str) -> Result<Value> {
    let mut values = vec![value];
    let mut plural = false;
    for segment in segments(path)? {
        let mut next = Vec::new();
        for value in values {
            match &segment {
                Segment::Key(key) => {
                    if let Some(value) = value.get(key) {
                        next.push(value)
                    }
                }
                Segment::Index(index) => {
                    if let Some(value) = value.get(index) {
                        next.push(value)
                    }
                }
                Segment::All => {
                    plural = true;
                    match value {
                        Value::Array(values) => next.extend(values),
                        Value::Object(values) => next.extend(values.values()),
                        _ => {}
                    }
                }
            }
        }
        ensure!(next.len() <= 4096, "JSONPath matched too many values");
        values = next;
    }
    Ok(if plural || values.len() > 1 {
        Value::Array(values.into_iter().cloned().collect())
    } else {
        values.first().map_or(Value::Null, |v| (*v).clone())
    })
}
fn read_file(path: &Path) -> Result<Vec<u8>> {
    let file = std::fs::File::open(path)?;
    ensure!(
        file.metadata()?.len() <= LIMIT as u64,
        "Widget file exceeds 256 KiB"
    );
    let mut bytes = Vec::new();
    file.take(LIMIT as u64 + 1).read_to_end(&mut bytes)?;
    ensure!(bytes.len() <= LIMIT, "Widget file exceeds 256 KiB");
    Ok(bytes)
}
fn command(program: &str, args: &[String], cwd: &Path) -> Result<Vec<u8>> {
    use std::{
        os::windows::{io::AsRawHandle, process::CommandExt},
        process::{Command, Stdio},
    };
    use windows::Win32::{
        Foundation::{CloseHandle, HANDLE},
        System::{JobObjects::*, Threading::TerminateProcess},
    };
    // A kill-on-close job also releases child processes after a timeout or output overflow.
    // SAFETY: the job is owned here; the child handle is borrowed from std::process::Child.
    let job = unsafe { CreateJobObjectW(None, None)? };
    struct Job(HANDLE);
    impl Drop for Job {
        fn drop(&mut self) {
            /* SAFETY: closes our owned job and its remaining processes. */
            unsafe {
                let _ = CloseHandle(self.0);
            }
        }
    }
    let job = Job(job);
    let mut limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
    limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
    // SAFETY: passes a live structure of the documented size to our owned job.
    unsafe {
        SetInformationJobObject(
            job.0,
            JobObjectExtendedLimitInformation,
            (&limits as *const JOBOBJECT_EXTENDED_LIMIT_INFORMATION).cast(),
            std::mem::size_of_val(&limits) as u32,
        )?;
    }
    let mut child = Command::new(program)
        .args(args)
        .current_dir(cwd)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .creation_flags(0x08000000)
        .spawn()
        .context("Could not run custom widget command")?;
    // SAFETY: child owns a valid process handle. If assignment fails, terminate it before returning.
    if let Err(e) = unsafe { AssignProcessToJobObject(job.0, HANDLE(child.as_raw_handle())) } {
        // SAFETY: std::process::Child still owns this live process handle.
        unsafe {
            let _ = TerminateProcess(HANDLE(child.as_raw_handle()), 1);
        }
        return Err(e.into());
    }
    let stdout = child.stdout.take().unwrap();
    let reader = std::thread::spawn(move || {
        let mut bytes = Vec::new();
        stdout
            .take(LIMIT as u64 + 1)
            .read_to_end(&mut bytes)
            .map(|_| bytes)
    });
    let deadline = Instant::now() + Duration::from_secs(5);
    let status = loop {
        if let Some(status) = child.try_wait()? {
            break status;
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            drop(job);
            let _ = reader.join();
            bail!("Custom widget command timed out after 5 seconds");
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    drop(job);
    let bytes = reader
        .join()
        .map_err(|_| anyhow::anyhow!("Custom command output worker stopped"))??;
    ensure!(status.success(), "Custom widget command failed");
    ensure!(
        bytes.len() <= LIMIT,
        "Custom command output exceeds 256 KiB"
    );
    Ok(bytes)
}

/// Worker-only. Commands are executed only for a widget explicitly added by the user.
pub fn evaluate(id: &str, definition: &Definition) -> Result<Value> {
    let base = path(id)?
        .parent()
        .context("Widget folder is missing")?
        .to_owned();
    let (bytes, json_path) = match &definition.source {
        Source::Static { value } => return Ok(value.clone()),
        Source::Url { url, path } => (
            glaze_integrations::calendar_http::request("GET", url, &[], &[], LIMIT)?,
            Some(path.as_str()),
        ),
        Source::File { path, json_path } => {
            let file = Path::new(path);
            let file = if file.is_absolute() {
                file.to_owned()
            } else {
                base.join(file)
            };
            if definition.layout == Template::Image && json_path.is_none() {
                return Ok(Value::String(file.to_string_lossy().into_owned()));
            }
            (read_file(&file)?, json_path.as_deref())
        }
        Source::Command {
            program,
            args,
            json_path,
        } => (command(program, args, &base)?, json_path.as_deref()),
    };
    if let Some(path) = json_path {
        select(
            &serde_json::from_slice(&bytes).context("Data source did not return valid JSON")?,
            path,
        )
    } else {
        Ok(serde_json::from_slice(&bytes)
            .unwrap_or(Value::String(String::from_utf8(bytes)?.trim().into())))
    }
}

pub const EXAMPLES: [(&str, &str); 4] = [
    ("welcome", include_str!("custom_examples/welcome.json")),
    (
        "github-release",
        include_str!("custom_examples/github-release.json"),
    ),
    (
        "reading-list",
        include_str!("custom_examples/reading-list.json"),
    ),
    (
        "disk-space",
        include_str!("custom_examples/disk-space.json"),
    ),
];
/// Called by New custom widget. Existing definitions are never overwritten.
pub fn install_examples() -> Result<PathBuf> {
    use std::io::Write;
    let folder = folder()?;
    std::fs::create_dir_all(&folder)?;
    for (name, text) in EXAMPLES {
        match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(folder.join(format!("{name}.json")))
        {
            Ok(mut file) => file.write_all(text.as_bytes())?,
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(e) => return Err(e.into()),
        }
    }
    let notes = folder.join("reading-list.txt");
    if !notes.exists() {
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(notes)?;
        file.write_all(b"The Pragmatic Programmer\nA Philosophy of Software Design\nDesigning Data-Intensive Applications\n")?;
    }
    Ok(folder)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn json_paths_select_keys_indices_and_lists() {
        let value = json!({"a":[{"name":"one"},{"name":"two"}],"a.b":7});
        assert_eq!(select(&value, "$.a[1].name").unwrap(), "two");
        assert_eq!(
            select(&value, "$.a[*].name").unwrap(),
            json!(["one", "two"])
        );
        assert_eq!(select(&value, "$['a.b']").unwrap(), 7);
        assert!(select(&value, "$..name").is_err());
        let definition=Definition::parse(r#"{"title":"Local JSON","layout":"stat","source":{"type":"file","path":"data.json","jsonPath":"$.value"}}"#).unwrap();
        assert!(
            matches!(definition.source,Source::File{json_path:Some(ref path),..}if path=="$.value")
        );
    }
    #[test]
    fn examples_are_individual_multi_instance_catalog_entries() {
        for (name, text) in EXAMPLES {
            let definition = Definition::parse(text).unwrap();
            let manifest = definition.manifest(&format!("custom-{name}")).unwrap();
            assert!(manifest.instances.allows(5, 5));
        }
        assert!(Definition::parse(r#"{"title":"bad","layout":"text","source":{"type":"static","value":"x"},"refreshSeconds":1}"#).is_err());
    }
}
