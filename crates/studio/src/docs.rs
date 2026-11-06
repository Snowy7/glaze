//! The files Studio edits, as JSON documents: `config.json`, `bars.json`, `widgets.json` and
//! Studio's own `studio.json`, integrations and connection preferences. Every edit is a list of pointer operations applied to the file's
//! raw JSON (so keys Studio doesn't know survive), checked by the owning program's own parser,
//! and written atomically. Edits are undoable; a slider drag is one step.
//!
//! Rules (plans/native/studio-redesign.md, 5.8 and 6):
//! - a file that doesn't parse is never written; its pages show a banner and edit nothing until
//!   the user chooses Start over, which asks first and keeps a timestamped `.bak`;
//! - a file saved by a newer mypaper isn't broken: it says "Update Studio to edit this" and
//!   offers nothing destructive (no Start over);
//! - a file changed on disk since Studio read it is read again first, and the edit is applied to
//!   the newer text, so an editor never overwrites a newer file.

use std::path::{Path, PathBuf};
use std::time::Instant;

use anyhow::{Context, Result, bail};
use mypaper_widgets::layout;
use serde_json::{Map, Value};

/// What a file saved by a newer mypaper shows instead of an error.
pub const NEWER: &str = "Saved by a newer mypaper. Update Studio to edit this.";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReadIssue {
    Newer,
    Malformed { line: usize },
    Unavailable,
}

/// Unknown fields are a compatibility boundary, even when the file has no version number.
fn read_issue(id: DocId, source: &str, error: &anyhow::Error) -> ReadIssue {
    let future_version = serde_json::from_str::<Value>(source)
        .ok()
        .and_then(|raw| raw.get("version").and_then(Value::as_u64))
        .is_some_and(|version| match id {
            DocId::Widgets => version > u64::from(layout::VERSION),
            _ => version > 1,
        });
    if future_version
        || error.chain().any(|cause| {
            let message = cause.to_string();
            message.contains("unknown field") || message.contains("unknown variant")
        })
    {
        ReadIssue::Newer
    } else {
        let line = error
            .chain()
            .find_map(|cause| cause.downcast_ref::<serde_json::Error>())
            .map_or(1, |error| error.line().max(1));
        ReadIssue::Malformed { line }
    }
}

/// What [`Docs::start_over`] did.
#[derive(Debug, PartialEq)]
pub enum StartedOver {
    /// The user said no: nothing was touched.
    Declined,
    /// The defaults replaced the file, which was kept here first (none when there was no file).
    Done(Option<PathBuf>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DocId {
    Config,
    Bars,
    Widgets,
    /// Studio's own state: its appearance, where it was, recent wallpapers. Not undoable.
    Studio,
    /// `integrations.json` (docs/integrations.md): which integrations are on, their endpoints.
    Integrations,
    Plugins,
    /// A registered user wallpaper's existing `theme.json`; never a new settings field.
    Theme(usize),
}

impl DocId {
    pub const ALL: [DocId; 6] = [
        DocId::Config,
        DocId::Bars,
        DocId::Widgets,
        DocId::Studio,
        DocId::Integrations,
        DocId::Plugins,
    ];

    /// How a banner names the file's settings.
    pub fn label(self) -> &'static str {
        match self {
            DocId::Config => "Your settings",
            DocId::Bars => "Your bar settings",
            DocId::Widgets => "Your widget settings",
            DocId::Studio => "Studio's settings",
            DocId::Integrations => "Your integration settings",
            DocId::Plugins => "Your connection settings",
            DocId::Theme(_) => "This wallpaper's look",
        }
    }

    pub fn filename(self) -> &'static str {
        match self {
            Self::Config => "config.json",
            Self::Bars => "bars.json",
            Self::Widgets => "widgets.json",
            Self::Studio => "studio.json",
            Self::Integrations => "integrations.json",
            Self::Plugins => "plugin-manager.json",
            Self::Theme(_) => "theme.json",
        }
    }

    fn index(self) -> usize {
        match self {
            Self::Config => 0,
            Self::Bars => 1,
            Self::Widgets => 2,
            Self::Studio => 3,
            Self::Integrations => 4,
            Self::Plugins => 5,
            Self::Theme(index) => Self::ALL.len() + index,
        }
    }

    /// Checks `text` with the owning program's parser; returns the raw JSON and the effective
    /// values (defaults filled in) Studio reads from.
    pub fn parse(self, text: &str) -> Result<(Value, Value)> {
        let raw: Value = serde_json::from_str(text)?;
        if !raw.is_object() {
            bail!("the file isn't a JSON object");
        }
        let supported_version = if self == Self::Widgets {
            u64::from(layout::VERSION)
        } else {
            1
        };
        if raw
            .get("version")
            .and_then(Value::as_u64)
            .is_some_and(|v| v > supported_version)
        {
            bail!("{NEWER}");
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
            DocId::Theme(_) => {
                // The owner validates paths and all appearance/glass ranges. Parsing does
                // not open assets, so no synthetic folder is used for an actual write.
                mypaper_theme::LoadedTheme::parse(PathBuf::new(), text)?;
                raw.clone()
            }
            DocId::Plugins => {
                for key in raw.as_object().expect("checked").keys() {
                    if !matches!(key.as_str(), "disabled" | "uninstalled" | "settings") {
                        bail!("unknown field `{key}`");
                    }
                }
                serde_json::to_value(serde_json::from_str::<
                    mypaper_widgets::plugin_manager::Preferences,
                >(text)?)?
            }
            DocId::Integrations => {
                serde_json::to_value(mypaper_integrations::config::Config::parse(text)?)?
            }
        };
        Ok((raw, effective))
    }

    /// What a missing file means.
    fn empty(self) -> &'static str {
        match self {
            DocId::Bars => r#"{ "version": 1 }"#,
            DocId::Theme(_) => {
                r#"{"name":"Wallpaper","wallpaper":{"kind":"image","source":"wallpaper.png"}}"#
            }
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
    /// The file was saved by a newer mypaper (a later layout version): only a restart helps,
    /// and Start over is refused.
    pub newer: bool,
    pub issue: Option<ReadIssue>,
    /// The text last read or written, to notice other writers.
    seen: Option<String>,
}

impl Doc {
    fn load(id: DocId, path: Option<PathBuf>) -> Self {
        let text = path.as_deref().map(std::fs::read_to_string);
        match text {
            Some(Err(error)) if error.kind() != std::io::ErrorKind::NotFound => {
                let mut doc = Self::from_text(id, path, None);
                doc.issue = Some(ReadIssue::Unavailable);
                doc.error =
                    Some("Can't open this file. Check its permissions and try again.".into());
                doc
            }
            text => Self::from_text(id, path, text.and_then(|result| result.ok())),
        }
    }

    fn from_text(id: DocId, path: Option<PathBuf>, text: Option<String>) -> Self {
        let source = text.as_deref().unwrap_or(id.empty());
        match id.parse(source) {
            Ok((raw, effective)) => Self {
                path,
                raw,
                effective,
                error: None,
                newer: false,
                issue: None,
                seen: text,
            },
            Err(e) => {
                let issue = read_issue(id, source, &e);
                let newer = issue == ReadIssue::Newer;
                // A newer file isn't broken: no recovery copy, and its own message.
                if id == DocId::Widgets
                    && !newer
                    && let Some(path) = &path
                    && let Ok(bytes) = std::fs::read(path)
                {
                    let _ = mypaper_widgets::files::backup(path, &bytes);
                }
                let (raw, effective) = id.parse(id.empty()).expect("defaults parse");
                let message = if newer {
                    NEWER.to_owned()
                } else {
                    let ReadIssue::Malformed { line } = issue else {
                        unreachable!()
                    };
                    let name = path
                        .as_deref()
                        .and_then(Path::file_name)
                        .map(|name| name.to_string_lossy().into_owned())
                        .unwrap_or_else(|| id.filename().into());
                    format!("Can't read {name}, line {line}")
                };
                Self {
                    path,
                    raw,
                    effective,
                    error: Some(message),
                    newer,
                    issue: Some(issue),
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

#[derive(Debug, Clone)]
enum Pending {
    Edit(String, Vec<Op>),
    Undo,
    Redo,
}

pub struct Docs {
    docs: Vec<Doc>,
    undo: Vec<Step>,
    redo: Vec<Step>,
    /// A drag in progress: the raw documents before it.
    live: Option<Vec<(DocId, Value)>>,
    pending: Option<Pending>,
    failed_live: Option<String>,
    failed_doc: Option<DocId>,
    partial_save: bool,
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
        dir.as_ref().map(|d| d.join("integrations.json")),
        dir.map(|d| d.join("plugin-manager.json")),
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
            .zip(texts.into_iter().chain(["{}", "{}"]))
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
            pending: None,
            failed_live: None,
            failed_doc: None,
            partial_save: false,
            saved: Saved::Idle,
            version: 1,
        }
    }

    pub fn doc(&self, id: DocId) -> &Doc {
        &self.docs[id.index()]
    }

    /// The owning file is known even when a multi-file edit fails after an earlier write.
    pub fn failed_doc(&self) -> Option<DocId> {
        self.failed_doc
    }

    pub fn save_outcome(&self) -> &'static str {
        if self.failed_live.is_some() {
            "Change is active but not saved"
        } else if self.partial_save {
            "Some changes took effect; save is incomplete"
        } else {
            "Change did not take effect"
        }
    }

    pub fn ids(&self) -> impl Iterator<Item = DocId> + '_ {
        DocId::ALL
            .into_iter()
            .chain((0..self.docs.len() - DocId::ALL.len()).map(DocId::Theme))
    }

    /// Registers an existing, immediate user-theme child. Built-ins and missing or
    /// unreadable manifests remain read-only; each folder keeps its own undo identity.
    pub fn theme(&mut self, dir: &Path) -> Result<DocId> {
        self.theme_in(dir, &mypaper_config::user_themes_dir()?)
    }

    fn theme_in(&mut self, dir: &Path, user: &Path) -> Result<DocId> {
        let dir = dir.canonicalize().context("opening the wallpaper folder")?;
        let user = user
            .canonicalize()
            .context("opening your wallpaper folder")?;
        if dir.parent() != Some(user.as_path()) {
            bail!("a built-in wallpaper can't be changed: make a copy first");
        }
        let path = dir.join(mypaper_theme::MANIFEST);
        let source = std::fs::read_to_string(&path).context("reading theme.json")?;
        let index = self
            .docs
            .iter()
            .position(|doc| doc.path.as_ref() == Some(&path));
        let id = DocId::Theme(index.map_or(self.docs.len(), |i| i) - DocId::ALL.len());
        id.parse(&source)
            .context("this wallpaper can't be edited")?;
        if index.is_none() {
            self.docs.push(Doc::from_text(id, Some(path), Some(source)));
        }
        Ok(id)
    }

    /// Expands the editor's shared-family and clarity aliases into ordinary pointer ops.
    pub fn theme_ops(
        &mut self,
        dir: &Path,
        pointer: &str,
        value: Option<Value>,
    ) -> Result<Vec<Op>> {
        let doc = self.theme(dir)?;
        let (pointer, value) = match pointer.strip_suffix("/glass/clarity") {
            Some(base) => (
                format!("{base}/glass/tint"),
                value
                    .and_then(|v| v.as_f64())
                    .map(|c| serde_json::json!(((1.0 - c) * 1000.0).round() / 1000.0)),
            ),
            None => (pointer.to_owned(), value),
        };
        let paths = match pointer.strip_prefix("/@both") {
            Some(rest) => vec![format!("/widgets{rest}"), format!("/bars{rest}")],
            None => vec![pointer],
        };
        Ok(paths
            .into_iter()
            .map(|path| Op {
                doc,
                path,
                value: value.clone(),
            })
            .collect())
    }

    pub fn error(&self, id: DocId) -> Option<&str> {
        self.doc(id).error.as_deref()
    }

    /// Whether `id` was saved by a newer mypaper (restart to update; never start over).
    pub fn newer(&self, id: DocId) -> bool {
        self.doc(id).newer
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

    pub(crate) fn undo_count(&self) -> usize {
        self.undo.len()
    }

    /// Consecutive picks retain the first before-value and latest after-value.
    pub(crate) fn coalesce_last_pick(&mut self, checkpoint: usize) -> bool {
        if checkpoint == 0 || self.undo.len() != checkpoint + 1 {
            return false;
        }
        let latest = self.undo.pop().expect("length checked");
        let previous = self.undo.last_mut().expect("checkpoint is nonzero");
        previous.label = latest.label;
        for (id, before, after) in latest.changes {
            if let Some((_, _, final_value)) =
                previous.changes.iter_mut().find(|(doc, ..)| *doc == id)
            {
                *final_value = after;
            } else {
                previous.changes.push((id, before, after));
            }
        }
        true
    }

    /// Applies `ops` as one undoable step named `label`, writing every changed file.
    pub fn edit(&mut self, label: &str, ops: &[Op]) -> Result<()> {
        self.failed_doc = None;
        self.partial_save = false;
        let result = self.edit_inner(label, ops);
        if let Err(error) = &result {
            self.saved = Saved::Failed(format!("{error:#}"));
            self.pending = Some(Pending::Edit(label.into(), ops.to_vec()));
        } else {
            self.pending = None;
        }
        result
    }

    pub fn retry(&mut self) -> Result<()> {
        if let Some(label) = self.failed_live.clone() {
            return self.commit_live(&label);
        }
        match self.pending.clone() {
            Some(Pending::Edit(label, ops)) => self.edit(&label, &ops),
            Some(Pending::Undo) => self.undo().map(|_| ()),
            Some(Pending::Redo) => self.redo().map(|_| ()),
            None => {
                for id in self.ids().collect::<Vec<_>>() {
                    self.reload(id);
                }
                Ok(())
            }
        }
    }

    fn edit_inner(&mut self, label: &str, ops: &[Op]) -> Result<()> {
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
        for (id, old) in &before {
            let new = self.doc(*id).raw.clone();
            if *old != new {
                if let Err(error) = self.write(*id, &new) {
                    self.live = Some(before);
                    self.failed_live = Some(label.into());
                    return Err(error);
                }
                changes.push((*id, old.clone(), new));
            }
        }
        self.failed_live = None;
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
        self.partial_save = false;
        self.reload_themes();
        let Some(step) = self.undo.last().cloned() else {
            return Ok(None);
        };
        for (index, (id, before, _)) in step.changes.iter().enumerate() {
            if let Err(error) = self.replace(*id, before.clone()) {
                self.partial_save = index > 0;
                self.pending = Some(Pending::Undo);
                return Err(error);
            }
        }
        self.undo.pop();
        self.pending = None;
        let label = step.label.clone();
        self.redo.push(step);
        Ok(Some(label))
    }

    pub fn redo(&mut self) -> Result<Option<String>> {
        self.partial_save = false;
        self.reload_themes();
        let Some(step) = self.redo.last().cloned() else {
            return Ok(None);
        };
        for (index, (id, _, after)) in step.changes.iter().enumerate() {
            if let Err(error) = self.replace(*id, after.clone()) {
                self.partial_save = index > 0;
                self.pending = Some(Pending::Redo);
                return Err(error);
            }
        }
        self.redo.pop();
        self.pending = None;
        let label = step.label.clone();
        self.undo.push(step);
        Ok(Some(label))
    }

    fn reload_themes(&mut self) {
        for index in 0..self.docs.len() - DocId::ALL.len() {
            self.reload(DocId::Theme(index));
        }
    }

    /// Writes `id`'s file again, byte for byte, so the programs watching it reload what it
    /// points at (an icon pack's folder changed). Nothing is written while it doesn't exist.
    pub fn touch(&self, id: DocId) -> Result<()> {
        let Some(path) = &self.doc(id).path else {
            return Ok(());
        };
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
        self.undo
            .retain(|s| s.changes.iter().all(|(d, ..)| *d != id));
        self.redo
            .retain(|s| s.changes.iter().all(|(d, ..)| *d != id));
        let fresh = Doc::load(id, Some(path));
        let doc = &mut self.docs[id.index()];
        if fresh.error.is_some() {
            doc.error = fresh.error;
            doc.newer = fresh.newer;
            doc.issue = fresh.issue;
            doc.seen = fresh.seen;
        } else {
            *doc = fresh;
        }
        self.version += 1;
        true
    }

    /// Start over: `confirm` is asked first (Settings shows a dialog; it gets the backup's
    /// path), then the file is copied to `<name>.<YYYYMMDD-HHMMSS>.bak` (never replacing
    /// another), and only then replaced by `defaults`. A layout saved by a newer mypaper is
    /// refused outright: it isn't broken, this Studio is just old.
    pub fn start_over(
        &mut self,
        id: DocId,
        defaults: &str,
        confirm: impl FnOnce(Option<&Path>) -> bool,
    ) -> Result<StartedOver> {
        let path = self.doc(id).path.clone();
        let on_disk = path
            .as_deref()
            .and_then(|p| std::fs::read_to_string(p).ok());
        if self.doc(id).newer
            || on_disk.as_deref().is_some_and(|text| {
                id.parse(text)
                    .err()
                    .is_some_and(|error| read_issue(id, text, &error) == ReadIssue::Newer)
            })
        {
            bail!("{NEWER}");
        }
        if self.doc(id).issue == Some(ReadIssue::Unavailable) {
            bail!("Can't open this file. Check its permissions and try again.");
        }
        let backup = path.as_deref().filter(|p| p.exists()).map(timestamped);
        if !confirm(backup.as_deref()) {
            return Ok(StartedOver::Declined);
        }
        let (raw, effective) = id.parse(defaults)?;
        if let (Some(path), Some(backup)) = (&path, &backup) {
            use std::io::Write;
            let bytes = std::fs::read(path).context("reading the file to keep")?;
            let mut file = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(backup)
                .with_context(|| format!("keeping a backup at {}", backup.display()))?;
            file.write_all(&bytes)?;
            file.sync_all()?;
        }
        if let Some(path) = &path {
            write_atomic(path, defaults.as_bytes())?;
        }
        let doc = &mut self.docs[id.index()];
        doc.raw = raw;
        doc.effective = effective;
        doc.error = None;
        doc.issue = None;
        doc.newer = false;
        doc.seen = Some(defaults.to_owned());
        self.undo
            .retain(|s| s.changes.iter().all(|(d, ..)| *d != id));
        self.redo
            .retain(|s| s.changes.iter().all(|(d, ..)| *d != id));
        self.version += 1;
        Ok(StartedOver::Done(backup))
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
                bail!(
                    "{} can't be read, so Studio won't change it: {e}",
                    id.label()
                );
            }
            // Someone else wrote it since: start from their text.
            self.reload(*id);
            if let Some(e) = self.error(*id) {
                bail!(
                    "{} can't be read, so Studio won't change it: {e}",
                    id.label()
                );
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
            let (raw, effective) = id
                .parse(&text)
                .context("that change isn't valid")
                .inspect_err(|_| {
                    self.failed_doc = Some(id);
                })?;
            staged.push((id, before, raw, effective, text));
        }
        // Retain the original in-memory snapshots until all writes succeed. A retry of a
        // multi-file edit must still undo files that were written before a later failure.
        for (index, (id, _, raw, _, _)) in staged.iter().enumerate() {
            self.write(*id, raw).inspect_err(|_| {
                self.partial_save = index > 0;
            })?;
        }
        let mut changes = Vec::new();
        for (id, before, raw, effective, text) in staged {
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
        self.failed_doc = Some(id);
        let text = render(raw)?;
        if let Some(path) = &self.docs[id.index()].path {
            match std::fs::read_to_string(path) {
                Ok(current) if self.doc(id).seen.as_deref() != Some(&current) => {
                    if let Err(error) = id.parse(&current) {
                        let issue = read_issue(id, &current, &error);
                        let doc = &mut self.docs[id.index()];
                        doc.issue = Some(issue);
                        doc.newer = issue == ReadIssue::Newer;
                        doc.error = Some(if doc.newer {
                            NEWER.into()
                        } else {
                            let ReadIssue::Malformed { line } = issue else {
                                unreachable!()
                            };
                            format!("Can't read {}, line {line}", id.filename())
                        });
                        bail!("{}", doc.error.as_deref().unwrap());
                    }
                }
                Err(error) if error.kind() != std::io::ErrorKind::NotFound => {
                    if id == DocId::Widgets && error.kind() == std::io::ErrorKind::InvalidData {
                        if let Ok(bytes) = std::fs::read(path) {
                            let _ = mypaper_widgets::files::backup(path, &bytes);
                        }
                    }
                    self.saved = Saved::Failed("Couldn't save".into());
                    return Err(error.into());
                }
                _ => {}
            }
            let written = if id == DocId::Widgets {
                mypaper_widgets::layout::Layout::save_value(raw, path)
            } else {
                write_atomic(path, text.as_bytes())
            };
            written.inspect_err(|e| {
                self.saved = Saved::Failed(format!("{e:#}"));
            })?;
            self.saved = Saved::Done(Instant::now());
        }
        self.docs[id.index()].seen = Some(text);
        self.failed_doc = None;
        Ok(())
    }
}

/// `<path>.<YYYYMMDD-HHMMSS>.bak` in local time, or `-1`, `-2`, ... after it when taken.
fn timestamped(path: &Path) -> PathBuf {
    // SAFETY: fills and returns a plain struct.
    let t = unsafe { windows::Win32::System::SystemInformation::GetLocalTime() };
    let stamp = format!(
        "{:04}{:02}{:02}-{:02}{:02}{:02}",
        t.wYear, t.wMonth, t.wDay, t.wHour, t.wMinute, t.wSecond
    );
    (0..)
        .map(|n| {
            let mut name = path.as_os_str().to_owned();
            name.push(if n == 0 {
                format!(".{stamp}.bak")
            } else {
                format!(".{stamp}-{n}.bak")
            });
            PathBuf::from(name)
        })
        .find(|p| !p.exists())
        .expect("a free name")
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
    use crate::actions::Action;
    use serde_json::json;

    fn op(doc: DocId, path: &str, value: Option<Value>) -> Op {
        Op {
            doc,
            path: path.into(),
            value,
        }
    }

    #[test]
    fn retry_preserves_a_multi_file_step_and_failed_undo_and_redo() {
        let root = folder("multi-retry");
        let config = root.join("config.json");
        let bars = root.join("bars.json");
        std::fs::write(&config, "{}").unwrap();
        std::fs::write(&bars, "{\"version\":1}").unwrap();
        let mut docs = Docs::fixture(["{}", "{\"version\":1}", "{}", "{}"]);
        docs.docs[DocId::Config.index()] = Doc::load(DocId::Config, Some(config.clone()));
        docs.docs[DocId::Bars.index()] = Doc::load(DocId::Bars, Some(bars.clone()));
        let blocked = root.join("bars.json.tmp");
        std::fs::create_dir(&blocked).unwrap();
        assert!(
            docs.edit(
                "Desktop",
                &[
                    op(DocId::Config, "/paused", Some(json!(true))),
                    op(DocId::Bars, "/top/height", Some(json!(40))),
                ]
            )
            .is_err()
        );
        assert!(std::fs::read_to_string(&config).unwrap().contains("true"));
        assert_eq!(docs.failed_doc(), Some(DocId::Bars));
        assert_eq!(
            docs.save_outcome(),
            "Some changes took effect; save is incomplete"
        );
        assert_eq!(docs.doc(DocId::Config).raw, json!({}));
        std::fs::remove_dir(&blocked).unwrap();
        docs.retry().unwrap();
        assert_eq!(docs.undo_count(), 1);
        std::fs::create_dir(&blocked).unwrap();
        assert!(docs.undo().is_err());
        assert_eq!(docs.undo_count(), 1);
        std::fs::remove_dir(&blocked).unwrap();
        docs.retry().unwrap();
        assert_eq!(docs.doc(DocId::Config).raw, json!({}));
        assert_eq!(docs.doc(DocId::Bars).raw, json!({"version":1}));
        assert!(docs.can_redo());
        std::fs::create_dir(&blocked).unwrap();
        assert!(docs.redo().is_err());
        assert!(docs.can_redo());
        std::fs::remove_dir(&blocked).unwrap();
        docs.retry().unwrap();
        assert_eq!(docs.get(DocId::Config, "/paused"), Some(&json!(true)));
        assert_eq!(docs.get(DocId::Bars, "/top/height"), Some(&json!(40)));
        assert_eq!(docs.undo_count(), 1);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn unreadable_paths_are_not_missing_and_a_failed_slider_can_retry() {
        let root = folder("unreadable-and-drag");
        let path = root.join("config.json");
        std::fs::create_dir(&path).unwrap();
        let mut docs = Docs::fixture(["{}", "{\"version\":1}", "{}", "{}"]);
        docs.docs[0] = Doc::load(DocId::Config, Some(path.clone()));
        assert_eq!(docs.doc(DocId::Config).issue, Some(ReadIssue::Unavailable));
        assert!(
            docs.start_over(DocId::Config, "{}", |_| panic!(
                "must not reset an unreadable file"
            ))
            .is_err()
        );
        std::fs::remove_dir(&path).unwrap();
        std::fs::write(&path, "{}").unwrap();
        assert!(docs.reload(DocId::Config));
        docs.live(&[op(
            DocId::Config,
            "/look/material/clarity",
            Some(json!(0.7)),
        )])
        .unwrap();
        std::fs::create_dir(root.join("config.json.tmp")).unwrap();
        assert!(docs.commit_live("Clarity").is_err());
        assert_eq!(docs.failed_doc(), Some(DocId::Config));
        assert_eq!(docs.save_outcome(), "Change is active but not saved");
        assert_eq!(docs.undo_count(), 0);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "{}");
        std::fs::remove_dir(root.join("config.json.tmp")).unwrap();
        docs.retry().unwrap();
        assert_eq!(docs.undo_count(), 1);
        docs.undo().unwrap();
        assert!(docs.doc(DocId::Config).raw.get("look").is_none());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn every_document_rejects_a_future_version_including_studio_state() {
        for id in DocId::ALL {
            let doc = Doc::from_text(id, None, Some(r#"{"version":4294967295}"#.into()));
            assert_eq!(doc.issue, Some(ReadIssue::Newer), "{id:?}");
            assert_eq!(doc.error.as_deref(), Some(NEWER));
        }
    }

    #[test]
    fn unknown_fields_are_newer_and_cannot_be_started_over() {
        let root = folder("unknown-fields");
        for (id, source) in [
            (DocId::Config, "{\"future_setting\":true}"),
            (DocId::Bars, "{\"version\":1,\"future_setting\":true}"),
            (DocId::Widgets, "{\"future_setting\":true}"),
            (DocId::Plugins, "{\"future_setting\":true}"),
        ] {
            let path = root.join(id.filename());
            std::fs::write(&path, source).unwrap();
            let mut docs = Docs::fixture(["{}", "{\"version\":1}", "{}", "{}"]);
            docs.docs[id.index()] = Doc::load(id, Some(path.clone()));
            assert!(docs.newer(id), "{id:?}");
            assert_eq!(docs.error(id), Some(NEWER));
            assert!(
                docs.start_over(id, id.empty(), |_| panic!("must not offer reset"))
                    .is_err()
            );
            assert_eq!(std::fs::read_to_string(path).unwrap(), source);
        }
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn malformed_files_name_the_file_and_line_and_failed_writes_retry_the_edit() {
        let root = folder("retry");
        let path = root.join("config.json");
        std::fs::write(&path, "{\n  broken\n}").unwrap();
        let doc = Doc::load(DocId::Config, Some(path.clone()));
        assert_eq!(doc.issue, Some(ReadIssue::Malformed { line: 2 }));
        assert_eq!(doc.error.as_deref(), Some("Can't read config.json, line 2"));
        std::fs::write(&path, "{}").unwrap();
        let mut docs = Docs::fixture(["{}", "{\"version\":1}", "{}", "{}"]);
        docs.docs[0] = Doc::load(DocId::Config, Some(path.clone()));
        // A directory at the temporary filename deterministically denies the atomic write.
        std::fs::create_dir(root.join("config.json.tmp")).unwrap();
        assert!(
            docs.edit("Pause", &[op(DocId::Config, "/paused", Some(json!(true)))])
                .is_err()
        );
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "{}");
        std::fs::remove_dir(root.join("config.json.tmp")).unwrap();
        docs.retry().unwrap();
        assert_eq!(docs.get(DocId::Config, "/paused"), Some(&json!(true)));
        assert_eq!(docs.undo_count(), 1);
        docs.undo().unwrap();
        assert_eq!(docs.get(DocId::Config, "/paused"), Some(&json!(false)));
        std::fs::remove_dir_all(root).unwrap();
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
            docs.edit(
                "Bad",
                &[op(DocId::Config, "/pause_on_batery", Some(json!(true)))]
            )
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
        assert_eq!(
            docs.get(DocId::Config, "/look/material/clarity"),
            Some(&json!(0.6))
        );
        docs.edit(
            "Reset",
            &[op(DocId::Config, "/look/material/clarity", None)],
        )
        .unwrap();
        assert!(docs.doc(DocId::Config).raw.get("look").is_none(), "pruned");
        docs.undo().unwrap();
        docs.undo().unwrap();
        assert!(
            docs.doc(DocId::Config).raw.get("look").is_none(),
            "before the drag"
        );
    }

    #[test]
    fn a_file_that_does_not_parse_is_never_edited() {
        let mut docs = Docs::fixture([r#"{ "theme": 4 }"#, r#"{ "version": 1 }"#, "{}", "{}"]);
        assert!(docs.error(DocId::Config).is_some());
        assert!(
            docs.edit("Pause", &[op(DocId::Config, "/paused", Some(json!(true)))])
                .is_err()
        );
        docs.start_over(DocId::Config, "{}", |_| true).unwrap();
        assert!(docs.error(DocId::Config).is_none());
    }

    #[test]
    fn widget_drag_commit_refuses_and_backs_up_an_external_parse_failure() {
        let root =
            std::env::temp_dir().join(format!("mypaper-studio-write-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        let path = root.join("widgets.json");
        std::fs::write(&path, "{}").unwrap();
        let mut docs = Docs::fixture(["{}", "{\"version\":1}", "{}", "{}"]);
        docs.docs[DocId::Widgets.index()].path = Some(path.clone());
        docs.live(&[op(DocId::Widgets, "/overlay", Some(json!(false)))])
            .unwrap();
        let malformed = b"{ invalid external edit\xff";
        std::fs::write(&path, malformed).unwrap();
        assert!(docs.commit_live("Overlay").is_err());
        assert_eq!(std::fs::read(&path).unwrap(), malformed);
        assert_eq!(
            std::fs::read(root.join("widgets.json.bak")).unwrap(),
            malformed
        );
        std::fs::remove_dir_all(root).unwrap();
    }

    /// A fresh temp folder for one test.
    fn folder(name: &str) -> PathBuf {
        let temp = std::env::temp_dir().canonicalize().unwrap();
        let root = temp.join(format!("mypaper-studio-{name}-{}", std::process::id()));
        if root.exists() {
            assert_eq!(root.canonicalize().unwrap().parent(), Some(temp.as_path()));
            std::fs::remove_dir_all(&root).unwrap();
        }
        std::fs::create_dir_all(&root).unwrap();
        root
    }

    fn files_in(root: &Path) -> usize {
        std::fs::read_dir(root).unwrap().count()
    }

    fn theme_file(root: &Path, name: &str) -> PathBuf {
        let dir = root.join(name);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("theme.json"), r##"{"name":"Own","wallpaper":{"kind":"image","source":"photo.png"},"widgets":{"appearance":{"tint":"#239569"}}}"##).unwrap();
        dir
    }

    #[test]
    fn theme_drags_and_a_multi_document_restore_use_the_same_undo_stack() {
        let root = folder("theme-transactions");
        let dir = theme_file(&root, "own");
        let mut docs = Docs::fixture(["{}", r#"{"version":1}"#, "{}", "{}"]);
        let id = docs.theme_in(&dir, &root).unwrap();
        assert_eq!(docs.theme_in(&dir, &root).unwrap(), id);
        let original = docs.doc(id).raw.clone();
        for radius in [12, 18, 24] {
            docs.live(&[op(
                id,
                "/widgets/appearance/radius",
                Some(serde_json::json!(radius)),
            )])
            .unwrap();
            assert_eq!(
                serde_json::from_str::<Value>(
                    &std::fs::read_to_string(dir.join("theme.json")).unwrap()
                )
                .unwrap(),
                original,
                "a drag writes only on release"
            );
        }
        docs.commit_live("Wallpaper corners").unwrap();
        assert_eq!(docs.undo_count(), 1);
        let changed = docs.doc(id).raw.clone();
        docs.undo().unwrap();
        assert_eq!(docs.doc(id).raw, original);
        docs.redo().unwrap();
        assert_eq!(docs.doc(id).raw, changed);
        docs.edit(
            "Restore this desktop",
            &[
                op(id, "", Some(original.clone())),
                op(DocId::Config, "/theme", Some(serde_json::json!("own"))),
            ],
        )
        .unwrap();
        assert_eq!(docs.undo_count(), 2);
        docs.undo().unwrap();
        assert_eq!(docs.doc(id).raw, changed);
        assert!(docs.doc(DocId::Config).raw.pointer("/theme").is_none());
        docs.redo().unwrap();
        assert_eq!(docs.doc(id).raw, original);
        // The unchanged owner/older-reader schema accepts every saved document.
        mypaper_theme::LoadedTheme::load(&dir).unwrap();
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn theme_transactions_refuse_other_folders_and_preserve_external_edits() {
        let root = folder("theme-ownership");
        let user = root.join("themes");
        std::fs::create_dir_all(&user).unwrap();
        let bundled = theme_file(&root, "bundled");
        let dir = theme_file(&user, "own");
        let mut docs = Docs::fixture(["{}", r#"{"version":1}"#, "{}", "{}"]);
        assert!(docs.theme_in(&bundled, &user).is_err());
        let id = docs.theme_in(&dir, &user).unwrap();
        docs.edit(
            "Corners",
            &[op(
                id,
                "/widgets/appearance/radius",
                Some(serde_json::json!(24)),
            )],
        )
        .unwrap();
        let path = dir.join("theme.json");
        let external = r#"{"name":"External","wallpaper":{"kind":"image","source":"new.png"}}"#;
        std::fs::write(&path, external).unwrap();
        assert!(
            docs.undo().unwrap().is_none(),
            "outside edits invalidate only this file's undo steps"
        );
        assert_eq!(std::fs::read_to_string(&path).unwrap(), external);
        let future =
            r#"{"name":"Future","wallpaper":{"kind":"image","source":"new.png"},"new_field":true}"#;
        std::fs::write(&path, future).unwrap();
        assert!(
            docs.edit(
                "Corners",
                &[op(
                    id,
                    "/widgets/appearance/radius",
                    Some(serde_json::json!(12))
                )]
            )
            .is_err()
        );
        assert!(docs.newer(id));
        assert_eq!(std::fs::read_to_string(&path).unwrap(), future);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn a_layout_from_a_newer_mypaper_offers_restart_and_nothing_destructive() {
        let root = folder("newer-layout");
        let path = root.join("widgets.json");
        let current = r#"{ "version": 2, "instances": [] }"#;
        std::fs::write(&path, current).unwrap();
        let mut docs = Docs::fixture(["{}", "{\"version\":1}", current, "{}"]);
        docs.docs[DocId::Widgets.index()].path = Some(path.clone());
        docs.docs[DocId::Widgets.index()].seen = Some(current.into());
        // A newer mypaper saves its layout while this Studio is open.
        let newer = r#"{ "version": 3, "instances": [{ "instance": "clock-1" }], "future": true }"#;
        std::fs::write(&path, newer).unwrap();
        assert!(docs.reload(DocId::Widgets));
        assert!(docs.newer(DocId::Widgets));
        let (text, buttons) = crate::ui::banner_for(&docs, DocId::Widgets).unwrap();
        assert_eq!(text, NEWER);
        assert_eq!(
            buttons,
            vec![("Restart", Action::RestartStudio)],
            "Restart only: no Start over"
        );
        // Nothing can destroy it: Start over is refused without even asking, edits too.
        assert!(
            docs.start_over(DocId::Widgets, "{}", |_| panic!("never offered"))
                .is_err()
        );
        assert!(
            docs.edit(
                "Overlay",
                &[op(DocId::Widgets, "/overlay", Some(json!(false)))]
            )
            .is_err()
        );
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            newer,
            "left exactly as it was"
        );
        assert_eq!(files_in(&root), 1, "no recovery copy: it isn't broken");
        // Loaded fresh (Studio started while the file was newer): the same.
        let docs = Docs::fixture(["{}", "{\"version\":1}", newer, "{}"]);
        assert!(docs.newer(DocId::Widgets));
        let (_, buttons) = crate::ui::banner_for(&docs, DocId::Widgets).unwrap();
        assert!(
            buttons
                .iter()
                .all(|(_, a)| !matches!(a, Action::StartOver(_)))
        );
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn starting_over_asks_first_and_keeps_a_timestamped_backup_before_writing() {
        let root = folder("start-over");
        let path = root.join("widgets.json");
        let corrupt = "{ \"instances\": [ broken";
        std::fs::write(&path, corrupt).unwrap();
        let mut docs = Docs::fixture(["{}", "{\"version\":1}", corrupt, "{}"]);
        docs.docs[DocId::Widgets.index()].path = Some(path.clone());
        assert!(!docs.newer(DocId::Widgets) && docs.error(DocId::Widgets).is_some());
        let (_, buttons) = crate::ui::banner_for(&docs, DocId::Widgets).unwrap();
        assert!(buttons.contains(&("Start over", Action::StartOver(DocId::Widgets))));
        let defaults = r#"{ "version": 2, "instances": [] }"#;
        // Declined: nothing is touched.
        let mut asked = None;
        let declined = docs.start_over(DocId::Widgets, defaults, |backup| {
            asked = backup.map(Path::to_path_buf);
            false
        });
        assert_eq!(declined.unwrap(), StartedOver::Declined);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), corrupt);
        assert_eq!(files_in(&root), 1);
        // The question names the backup: widgets.json.<YYYYMMDD-HHMMSS>.bak, not written yet.
        let name = asked
            .unwrap()
            .file_name()
            .unwrap()
            .to_string_lossy()
            .into_owned();
        let stamp = name
            .strip_prefix("widgets.json.")
            .and_then(|s| s.strip_suffix(".bak"))
            .unwrap();
        assert!(stamp.len() == 15 && stamp.as_bytes()[8] == b'-', "{name}");
        assert!(
            stamp
                .chars()
                .filter(|c| *c != '-')
                .all(|c| c.is_ascii_digit()),
            "{name}"
        );
        // Confirmed: the old bytes are kept first, then the defaults are written.
        let done = docs.start_over(DocId::Widgets, defaults, |_| true).unwrap();
        let StartedOver::Done(Some(backup)) = done else {
            panic!("{done:?}")
        };
        assert_eq!(std::fs::read_to_string(&backup).unwrap(), corrupt);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), defaults);
        assert!(docs.error(DocId::Widgets).is_none());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn starting_widgets_over_never_replaces_an_earlier_recovery_copy() {
        let root =
            std::env::temp_dir().join(format!("mypaper-studio-reset-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        let path = root.join("widgets.json");
        let mut docs = Docs::fixture(["{}", "{\"version\":1}", "{}", "{}"]);
        docs.docs[DocId::Widgets.index()].path = Some(path.clone());
        std::fs::write(&path, "first malformed layout").unwrap();
        let done = |s| match s {
            StartedOver::Done(Some(backup)) => backup,
            other => panic!("{other:?}"),
        };
        let first = done(docs.start_over(DocId::Widgets, "{}", |_| true).unwrap());
        std::fs::write(&path, "second malformed layout").unwrap();
        let second = done(docs.start_over(DocId::Widgets, "{}", |_| true).unwrap());
        assert_ne!(first, second);
        assert_eq!(
            std::fs::read_to_string(first).unwrap(),
            "first malformed layout"
        );
        assert_eq!(
            std::fs::read_to_string(second).unwrap(),
            "second malformed layout"
        );
        std::fs::remove_dir_all(root).unwrap();
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
