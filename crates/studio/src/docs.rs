//! The files Studio edits, as JSON documents: `config.json`, `bars.json`, `widgets.json` and
//! Studio's own `studio.json`. Every edit is a list of pointer operations applied to the file's
//! raw JSON (so keys Studio doesn't know survive), checked by the owning program's own parser,
//! and written atomically. Edits are undoable; a slider drag is one step.
//!
//! Rules (plans/native/studio-redesign.md, 5.8 and 6):
//! - a file that doesn't parse is never written; its pages show a banner and edit nothing until
//!   the user chooses Start over, which keeps a `.bak`;
//! - a file changed on disk since Studio read it is read again first, and the edit is applied to
//!   the newer text, so an editor never overwrites a newer file.

use std::path::{Path, PathBuf};
use std::time::Instant;

use anyhow::{Context, Result, bail};
use serde_json::{Map, Value};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DocId {
    Config,
    Bars,
    Widgets,
    /// Studio's own state: its appearance, where it was, recent wallpapers. Not undoable.
    Studio,
    /// `integrations.json` (docs/integrations.md): which integrations are on, their endpoints.
    Integrations,
}

impl DocId {
    pub const ALL: [DocId; 5] = [DocId::Config, DocId::Bars, DocId::Widgets, DocId::Studio, DocId::Integrations];

    /// How a banner names the file's settings.
    pub fn label(self) -> &'static str {
        match self {
            DocId::Config => "Your settings",
            DocId::Bars => "Your bar settings",
            DocId::Widgets => "Your widget settings",
            DocId::Studio => "Studio's settings",
            DocId::Integrations => "Your integration settings",
        }
    }

    fn index(self) -> usize {
        self as usize
    }

    /// Checks `text` with the owning program's parser; returns the raw JSON and the effective
    /// values (defaults filled in) Studio reads from.
    pub fn parse(self, text: &str) -> Result<(Value, Value)> {
        let raw: Value = serde_json::from_str(text)?;
        if !raw.is_object() {
            bail!("the file isn't a JSON object");
        }
        let effective = match self {
            DocId::Config => serde_json::to_value(mypaper_config::Config::parse(text)?)?,
            DocId::Bars => {
                // The parsed config has every default; the layered JSON adds each bar's look and
                // motion (which the config doesn't write out).
                let config = mypaper_bars::BarsConfig::parse(text)?;
                let mut effective = serde_json::to_value(&config)?;
                let layered = mypaper_bars::presets::layered(raw.as_object().expect("checked"))?;
                mypaper_bars::presets::merge(&mut effective, &layered);
                effective
            }
            DocId::Widgets => {
                mypaper_widgets::layout::Layout::parse(text)?;
                raw.clone()
            }
            DocId::Studio => raw.clone(),
            DocId::Integrations => serde_json::to_value(mypaper_integrations::config::Config::parse(text)?)?,
        };
        Ok((raw, effective))
    }

    /// What a missing file means.
    fn empty(self) -> &'static str {
        match self {
            DocId::Bars => r#"{ "version": 1 }"#,
            _ => "{}",
        }
    }
}

/// One file.
#[derive(Debug, Clone)]
pub struct Doc {
    /// `None` for fixture documents (goldens): never written.
    pub path: Option<PathBuf>,
    pub raw: Value,
    pub effective: Value,
    /// Why the file on disk can't be read (it's left alone until Start over).
    pub error: Option<String>,
    /// The text last read or written, to notice other writers.
    seen: Option<String>,
}

impl Doc {
    fn load(id: DocId, path: Option<PathBuf>) -> Self {
        let text = path
            .as_deref()
            .and_then(|p| std::fs::read_to_string(p).ok());
        Self::from_text(id, path, text)
    }

    fn from_text(id: DocId, path: Option<PathBuf>, text: Option<String>) -> Self {
        let source = text.as_deref().unwrap_or(id.empty());
        match id.parse(source) {
            Ok((raw, effective)) => Self {
                path,
                raw,
                effective,
                error: None,
                seen: text,
            },
            Err(e) => {
                let (raw, effective) = id.parse(id.empty()).expect("defaults parse");
                Self {
                    path,
                    raw,
                    effective,
                    error: Some(format!("{e:#}")),
                    seen: text,
                }
            }
        }
    }
}

/// One pointer operation: set a value, or remove it (back to inherited).
#[derive(Debug, Clone, PartialEq)]
pub struct Op {
    pub doc: DocId,
    pub path: String,
    pub value: Option<Value>,
}

/// An undo step: each document's raw JSON before and after.
#[derive(Debug, Clone)]
pub struct Step {
    pub label: String,
    changes: Vec<(DocId, Value, Value)>,
}

/// What the page bar's save state says.
#[derive(Debug, Clone, PartialEq)]
pub enum Saved {
    Idle,
    Done(Instant),
    Failed(String),
}

pub struct Docs {
    docs: Vec<Doc>,
    undo: Vec<Step>,
    redo: Vec<Step>,
    /// A drag in progress: the raw documents before it.
    live: Option<Vec<(DocId, Value)>>,
    pub saved: Saved,
    /// Bumped on every change: previews cache on it.
    pub version: u64,
}

/// Where the documents live.
pub fn paths() -> Vec<Option<PathBuf>> {
    let dir = mypaper_config::dir().ok();
    vec![
        mypaper_config::path().ok(),
        mypaper_bars::config::path().ok(),
        mypaper_widgets::files::layout().ok(),
        dir.as_ref().map(|d| d.join("studio.json")),
        dir.map(|d| d.join("integrations.json")),
    ]
}

impl Docs {
    pub fn load() -> Self {
        let docs = DocId::ALL
            .iter()
            .zip(paths())
            .map(|(id, path)| Doc::load(*id, path))
            .collect();
        Self::with(docs)
    }

    /// Documents from text, never written (goldens and tests).
    #[cfg(test)]
    pub fn fixture(texts: [&str; 4]) -> Self {
        let docs = DocId::ALL
            .iter()
            .zip(texts.into_iter().chain(["{}"]))
            .map(|(id, text)| Doc::from_text(*id, None, Some(text.to_owned())))
            .collect();
        Self::with(docs)
    }

    fn with(docs: Vec<Doc>) -> Self {
        Self {
            docs,
            undo: Vec::new(),
            redo: Vec::new(),
            live: None,
            saved: Saved::Idle,
            version: 1,
        }
    }

    pub fn doc(&self, id: DocId) -> &Doc {
        &self.docs[id.index()]
    }

    pub fn error(&self, id: DocId) -> Option<&str> {
        self.doc(id).error.as_deref()
    }

    /// The value at `path`: the file's own, else its owner's default.
    pub fn get(&self, id: DocId, path: &str) -> Option<&Value> {
        let doc = self.doc(id);
        doc.raw
            .pointer(path)
            .or_else(|| doc.effective.pointer(path))
    }

    /// Whether the file itself sets `path` (rather than inheriting it).
    pub fn is_set(&self, id: DocId, path: &str) -> bool {
        self.doc(id).raw.pointer(path).is_some()
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    /// Applies `ops` as one undoable step named `label`, writing every changed file.
    pub fn edit(&mut self, label: &str, ops: &[Op]) -> Result<()> {
        if self.live.is_some() {
            self.commit_live(label)?;
        }
        let changes = self.apply(ops)?;
        if changes.is_empty() {
            return Ok(());
        }
        let undoable: Vec<_> = changes
            .into_iter()
            .filter(|(id, ..)| *id != DocId::Studio)
            .collect();
        if !undoable.is_empty() {
            self.undo.push(Step {
                label: label.to_owned(),
                changes: undoable,
            });
            self.redo.clear();
        }
        Ok(())
    }

    /// Applies `ops` in memory only (a drag in progress); [`commit_live`](Self::commit_live)
    /// writes them as one step.
    pub fn live(&mut self, ops: &[Op]) -> Result<()> {
        let mut staged: Vec<(DocId, Value)> = Vec::new();
        for op in ops {
            if let Some(e) = self.error(op.doc) {
                bail!("{}: {e}", op.doc.label());
            }
            if !staged.iter().any(|(id, _)| *id == op.doc) {
                staged.push((op.doc, self.doc(op.doc).raw.clone()));
            }
            let raw = &mut staged.iter_mut().find(|(id, _)| *id == op.doc).unwrap().1;
            apply_op(raw, op);
        }
        let mut checked = Vec::new();
        for (id, raw) in staged {
            let text = render(&raw)?;
            let (raw, effective) = id.parse(&text)?;
            checked.push((id, raw, effective));
        }
        let before = self.live.get_or_insert_with(Vec::new);
        for (id, raw, effective) in checked {
            if !before.iter().any(|(b, _)| *b == id) {
                before.push((id, self.docs[id.index()].raw.clone()));
            }
            let doc = &mut self.docs[id.index()];
            doc.raw = raw;
            doc.effective = effective;
        }
        self.version += 1;
        Ok(())
    }

    /// Writes a drag's documents as one step.
    pub fn commit_live(&mut self, label: &str) -> Result<()> {
        let Some(before) = self.live.take() else {
            return Ok(());
        };
        let mut changes = Vec::new();
        for (id, old) in before {
            let new = self.doc(id).raw.clone();
            if old != new {
                self.write(id, &new)?;
                changes.push((id, old, new));
            }
        }
        if !changes.is_empty() {
            self.undo.push(Step {
                label: label.to_owned(),
                changes,
            });
            self.redo.clear();
        }
        Ok(())
    }

    /// Takes back the last step; returns its label.
    pub fn undo(&mut self) -> Result<Option<String>> {
        self.commit_live("Change")?;
        let Some(step) = self.undo.pop() else {
            return Ok(None);
        };
        for (id, before, _) in &step.changes {
            self.replace(*id, before.clone())?;
        }
        let label = step.label.clone();
        self.redo.push(step);
        Ok(Some(label))
    }

    pub fn redo(&mut self) -> Result<Option<String>> {
        let Some(step) = self.redo.pop() else {
            return Ok(None);
        };
        for (id, _, after) in &step.changes {
            self.replace(*id, after.clone())?;
        }
        let label = step.label.clone();
        self.undo.push(step);
        Ok(Some(label))
    }

    /// Writes `id`'s file again, byte for byte, so the programs watching it reload what it
    /// points at (an icon pack's folder changed). Nothing is written while it doesn't exist.
    pub fn touch(&self, id: DocId) -> Result<()> {
        let Some(path) = &self.doc(id).path else { return Ok(()) };
        match std::fs::read(path) {
            Ok(bytes) => write_atomic(path, &bytes),
            Err(_) => Ok(()),
        }
    }

    /// Reads `id` again after an outside change. Returns whether anything changed. A file that
    /// stopped parsing keeps its last good values on screen, with the error shown.
    pub fn reload(&mut self, id: DocId) -> bool {
        let doc = &self.docs[id.index()];
        let Some(path) = doc.path.clone() else {
            return false;
        };
        let text = std::fs::read_to_string(&path).ok();
        if text == doc.seen {
            return false;
        }
        // Our own undo history no longer describes this file.
        self.undo.retain(|s| s.changes.iter().all(|(d, ..)| *d != id));
        self.redo.retain(|s| s.changes.iter().all(|(d, ..)| *d != id));
        let fresh = Doc::from_text(id, Some(path), text);
        let doc = &mut self.docs[id.index()];
        if fresh.error.is_some() {
            doc.error = fresh.error;
            doc.seen = fresh.seen;
        } else {
            *doc = fresh;
        }
        self.version += 1;
        true
    }

    /// Start over: the file is kept as `<name>.bak` and replaced by `defaults`.
    pub fn start_over(&mut self, id: DocId, defaults: &str) -> Result<Option<PathBuf>> {
        let path = self.doc(id).path.clone();
        let backup = match &path {
            Some(path) if path.exists() => {
                let mut name = path.as_os_str().to_owned();
                name.push(".bak");
                let backup = PathBuf::from(name);
                std::fs::copy(path, &backup).context("keeping a backup")?;
                Some(backup)
            }
            _ => None,
        };
        let (raw, effective) = id.parse(defaults)?;
        if let Some(path) = &path {
            write_atomic(path, defaults.as_bytes())?;
        }
        let doc = &mut self.docs[id.index()];
        doc.raw = raw;
        doc.effective = effective;
        doc.error = None;
        doc.seen = Some(defaults.to_owned());
        self.undo.retain(|s| s.changes.iter().all(|(d, ..)| *d != id));
        self.redo.retain(|s| s.changes.iter().all(|(d, ..)| *d != id));
        self.version += 1;
        Ok(backup)
    }

    /// Applies, checks and writes `ops`; returns each changed document's before and after.
    fn apply(&mut self, ops: &[Op]) -> Result<Vec<(DocId, Value, Value)>> {
        let mut ids: Vec<DocId> = Vec::new();
        for op in ops {
            if !ids.contains(&op.doc) {
                ids.push(op.doc);
            }
        }
        for id in &ids {
            if let Some(e) = self.error(*id) {
                bail!("{} can't be read, so Studio won't change it: {e}", id.label());
            }
            // Someone else wrote it since: start from their text.
            self.reload(*id);
            if let Some(e) = self.error(*id) {
                bail!("{} can't be read, so Studio won't change it: {e}", id.label());
            }
        }
        let mut staged = Vec::new();
        for id in ids {
            let before = self.doc(id).raw.clone();
            let mut after = before.clone();
            for op in ops.iter().filter(|op| op.doc == id) {
                apply_op(&mut after, op);
            }
            if after == before {
                continue;
            }
            let text = render(&after)?;
            let (raw, effective) = id.parse(&text).context("that change isn't valid")?;
            staged.push((id, before, raw, effective, text));
        }
        let mut changes = Vec::new();
        for (id, before, raw, effective, text) in staged {
            if let Some(path) = &self.doc(id).path
                && let Err(e) = write_atomic(path, text.as_bytes())
            {
                self.saved = Saved::Failed(format!("{e:#}"));
                return Err(e);
            }
            let doc = &mut self.docs[id.index()];
            doc.raw = raw.clone();
            doc.effective = effective;
            doc.seen = Some(text);
            changes.push((id, before, raw));
        }
        if !changes.is_empty() {
            self.saved = Saved::Done(Instant::now());
            self.version += 1;
        }
        Ok(changes)
    }

    /// Puts `raw` back as the whole document (undo and redo).
    fn replace(&mut self, id: DocId, raw: Value) -> Result<()> {
        let text = render(&raw)?;
        let (raw, effective) = id.parse(&text)?;
        self.write(id, &raw)?;
        let doc = &mut self.docs[id.index()];
        doc.raw = raw;
        doc.effective = effective;
        self.version += 1;
        Ok(())
    }

    fn write(&mut self, id: DocId, raw: &Value) -> Result<()> {
        let text = render(raw)?;
        if let Some(path) = &self.docs[id.index()].path {
            write_atomic(path, text.as_bytes()).inspect_err(|e| {
                self.saved = Saved::Failed(format!("{e:#}"));
            })?;
            self.saved = Saved::Done(Instant::now());
        }
        self.docs[id.index()].seen = Some(text);
        Ok(())
    }
}

fn render(raw: &Value) -> Result<String> {
    Ok(serde_json::to_string_pretty(raw)? + "\n")
}

/// Writes atomically (a temp file, then a rename), so watchers never see half a file.
pub fn write_atomic(path: &Path, bytes: &[u8]) -> Result<()> {
    std::fs::create_dir_all(path.parent().context("path has no parent")?)?;
    let mut temp = path.as_os_str().to_owned();
    temp.push(".tmp");
    let temp = PathBuf::from(temp);
    std::fs::write(&temp, bytes).with_context(|| format!("writing {}", temp.display()))?;
    std::fs::rename(&temp, path).with_context(|| format!("replacing {}", path.display()))
}

/// The JSON pointer's tokens, unescaped (`~1` is `/`, `~0` is `~`).
fn tokens(path: &str) -> Vec<String> {
    path.split('/')
        .skip(1)
        .map(|t| t.replace("~1", "/").replace("~0", "~"))
        .collect()
}

/// Escapes one pointer token.
pub fn token(key: &str) -> String {
    key.replace('~', "~0").replace('/', "~1")
}

/// Sets or removes `op.path` in `root`, creating objects on the way; removing prunes objects
/// left empty.
pub fn apply_op(root: &mut Value, op: &Op) {
    let tokens = tokens(&op.path);
    match &op.value {
        Some(value) => set(root, &tokens, value.clone()),
        None => {
            remove(root, &tokens);
        }
    }
}

fn set(node: &mut Value, tokens: &[String], value: Value) {
    let Some((first, rest)) = tokens.split_first() else {
        *node = value;
        return;
    };
    if let Value::Array(items) = node
        && let Ok(i) = first.parse::<usize>()
    {
        if i < items.len() {
            set(&mut items[i], rest, value);
        } else if i == items.len() && rest.is_empty() {
            items.push(value);
        }
        return;
    }
    if !node.is_object() {
        *node = Value::Object(Map::new());
    }
    let map = node.as_object_mut().expect("an object");
    let child = map.entry(first.clone()).or_insert(Value::Null);
    if rest.is_empty() {
        *child = value;
    } else {
        set(child, rest, value);
    }
}

/// Removes the value; returns whether the node is now an empty object (to prune).
fn remove(node: &mut Value, tokens: &[String]) -> bool {
    let Some((first, rest)) = tokens.split_first() else {
        return false;
    };
    match node {
        Value::Object(map) => {
            if rest.is_empty() {
                map.remove(first);
            } else if let Some(child) = map.get_mut(first)
                && remove(child, rest)
            {
                map.remove(first);
            }
            map.is_empty()
        }
        Value::Array(items) => {
            if let Ok(i) = first.parse::<usize>()
                && i < items.len()
            {
                if rest.is_empty() {
                    items.remove(i);
                } else {
                    remove(&mut items[i], rest);
                }
            }
            false
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn op(doc: DocId, path: &str, value: Option<Value>) -> Op {
        Op {
            doc,
            path: path.into(),
            value,
        }
    }

    #[test]
    fn edits_keep_unknown_keys_validate_and_undo_as_one_step() {
        let mut docs = Docs::fixture([
            r#"{ "theme": "aurora" }"#,
            r#"{ "version": 1, "top": { "height": 30 } }"#,
            "{}",
            "{}",
        ]);
        docs.edit(
            "Height",
            &[
                op(DocId::Bars, "/top/height", Some(json!(36))),
                op(DocId::Config, "/paused", Some(json!(true))),
            ],
        )
        .unwrap();
        assert_eq!(docs.get(DocId::Bars, "/top/height"), Some(&json!(36)));
        assert_eq!(docs.get(DocId::Config, "/theme"), Some(&json!("aurora")));
        // The effective values fill in what the file leaves out (the default design).
        assert!(docs.get(DocId::Bars, "/dock/size").is_some());
        // An invalid change is refused whole.
        assert!(
            docs.edit("Bad", &[op(DocId::Config, "/pause_on_batery", Some(json!(true)))])
                .is_err()
        );
        assert_eq!(docs.undo().unwrap().as_deref(), Some("Height"));
        assert_eq!(docs.get(DocId::Bars, "/top/height"), Some(&json!(30)));
        assert_eq!(docs.get(DocId::Config, "/paused"), Some(&json!(false)));
        docs.redo().unwrap();
        assert_eq!(docs.get(DocId::Config, "/paused"), Some(&json!(true)));
    }

    #[test]
    fn a_drag_is_one_step_and_removal_prunes_empty_sections() {
        let mut docs = Docs::fixture(["{}", r#"{ "version": 1 }"#, "{}", "{}"]);
        for v in [0.2, 0.4, 0.6] {
            docs.live(&[op(DocId::Config, "/look/material/clarity", Some(json!(v)))])
                .unwrap();
        }
        docs.commit_live("Clarity").unwrap();
        assert_eq!(docs.get(DocId::Config, "/look/material/clarity"), Some(&json!(0.6)));
        docs.edit("Reset", &[op(DocId::Config, "/look/material/clarity", None)])
            .unwrap();
        assert!(docs.doc(DocId::Config).raw.get("look").is_none(), "pruned");
        docs.undo().unwrap();
        docs.undo().unwrap();
        assert!(docs.doc(DocId::Config).raw.get("look").is_none(), "before the drag");
    }

    #[test]
    fn a_file_that_does_not_parse_is_never_edited() {
        let mut docs = Docs::fixture([r#"{ "theme": 4 }"#, r#"{ "version": 1 }"#, "{}", "{}"]);
        assert!(docs.error(DocId::Config).is_some());
        assert!(
            docs.edit("Pause", &[op(DocId::Config, "/paused", Some(json!(true)))])
                .is_err()
        );
        docs.start_over(DocId::Config, "{}").unwrap();
        assert!(docs.error(DocId::Config).is_none());
    }

    #[test]
    fn array_items_can_be_set_appended_and_removed() {
        let mut v = json!({ "list": ["a", "b"] });
        apply_op(&mut v, &op(DocId::Studio, "/list/1", Some(json!("B"))));
        apply_op(&mut v, &op(DocId::Studio, "/list/2", Some(json!("c"))));
        apply_op(&mut v, &op(DocId::Studio, "/list/0", None));
        assert_eq!(v, json!({ "list": ["B", "c"] }));
        assert_eq!(tokens("/a~1b/c~0d"), ["a/b", "c~d"]);
    }
}
