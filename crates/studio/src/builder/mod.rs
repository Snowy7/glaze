//! Wallpaper composition. Drafts are ordinary, hidden theme folders; edits use Studio's
//! document history, while imported assets remain local to the draft until Save.
mod drop_target;
mod preview;
mod storage;
#[cfg(test)]
mod tests;
mod view;

use crate::{
    actions::{Action, Edit},
    app::App,
    docs::DocId,
    panel::Page,
};
use anyhow::{Context, Result, bail};
use serde_json::{Value, json};
use std::{
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

#[derive(Debug)]
pub struct Editor {
    pub dir: PathBuf,
    pub doc: DocId,
    pub selected: Option<usize>,
    pub effects: Vec<Effect>,
    pub category: Option<String>,
    pub shader: Option<PathBuf>,
    shader_origin: Option<PathBuf>,
    pub source: String,
    pub diagnostics: Vec<Diagnostic>,
    pending_code: Option<Instant>,
    next_poll: Instant,
    version: u64,
    preview_stamp: Option<std::time::SystemTime>,
    validation_key: Option<u64>,
    drag: Option<usize>,
    /// Your own stack being edited: Save writes back here. `None` saves a new theme.
    pub origin: Option<PathBuf>,
    /// The draft as opened, for Revert.
    initial: Value,
    /// Studio's picture budget before the editor raised it.
    images_budget: Option<usize>,
    /// The document as last saved (`None` until a new wallpaper's first save).
    pub saved: Option<Value>,
    /// A new, untouched draft shows the "start from" choices.
    pub fresh: bool,
    /// The gallery card under the pointer, previewed on the canvas (never in undo history).
    pub hover: Option<String>,
    hover_since: Option<Instant>,
    hover_shown: Option<String>,
    owns_folder: bool,
    _drop_target: Option<drop_target::Target>,
    pub preview: preview::Preview,
}

impl Drop for Editor {
    fn drop(&mut self) {
        // Stop our renderer before deleting the files it watches.
        self.preview = preview::Preview::default();
        self._drop_target.take();
        if self.owns_folder
            && self
                .dir
                .file_name()
                .is_some_and(|n| n.to_string_lossy().starts_with(".draft-"))
        {
            let _ = std::fs::remove_dir_all(&self.dir);
        }
    }
}

/// A resolved package; schema and validation belong to the engine's shared theme crate.
#[derive(Debug, Clone)]
pub struct Effect {
    pub manifest: glaze_theme::EffectManifest,
    pub dir: PathBuf,
}
impl std::ops::Deref for Effect {
    type Target = glaze_theme::EffectManifest;
    fn deref(&self) -> &Self::Target {
        &self.manifest
    }
}
impl Effect {
    pub fn category(&self) -> &str {
        use glaze_theme::stack::Category;
        match self.manifest.category {
            Category::Weather => "weather",
            Category::Camera => "camera",
            Category::Color => "color",
            Category::Motion => "motion",
            Category::Stylize => "stylize",
            Category::Light => "light",
        }
    }
    pub fn schemas(&self) -> Vec<Value> {
        self.params
            .iter()
            .filter_map(|p| serde_json::to_value(p).ok())
            .collect()
    }
}

#[derive(Debug, Clone)]
pub struct Diagnostic {
    pub line: usize,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Command {
    New,
    Edit(usize),
    Select(Option<usize>),
    Gallery,
    Category(Option<String>),
    AddEffect(String),
    Base(&'static str),
    Import(bool),
    Duplicate,
    Remove,
    Move(i32),
    Drag(usize),
    Shader,
    External,
    Save(bool),
    CustomEffect,
    Credits,
    GradientStop(bool),
    /// Close the effect gallery sheet.
    CloseGallery,
    SaveCopy,
    Revert,
    Discard,
    /// Base fit: cover, contain, stretch or center.
    Fit(&'static str),
    /// Base focus from the 3x3 anchor picker, as fractions.
    Focus(u8, u8),
    /// An existing wallpaper (library index) as the base.
    BaseTheme(usize),
    /// Import a base of one kind: photo, video or scene.
    Pick(&'static str),
    /// The layer menu's actions, on the layer at this index.
    DuplicateAt(usize),
    RemoveAt(usize),
    MoveAt(usize, i32),
}

pub fn active(app: &App) -> Option<&Editor> {
    app.panel.as_ref()?.builder.as_ref()
}
pub fn raw(app: &App) -> Value {
    active(app)
        .map(|e| app.docs.doc(e.doc).raw.clone())
        .unwrap_or(Value::Null)
}
pub fn edit(app: &App, path: impl Into<String>, label: &str) -> Edit {
    Edit::new(
        active(app).expect("builder page owns a draft").doc,
        path.into(),
        label,
    )
}
fn state(app: &mut App) -> &mut Editor {
    app.panel.as_mut().unwrap().builder.as_mut().unwrap()
}
/// The draft differs from what was last saved, or was never saved.
pub fn unsaved(app: &App) -> bool {
    active(app).is_some_and(|editor| editor.saved.as_ref() != Some(&raw(app)))
}
pub fn is_page(page: Page) -> bool {
    matches!(page, Page::Create | Page::Effects | Page::Shader)
}

/// Use the engine's parser before a theme enters document history or Save.
pub fn validate_theme(text: &str) -> Result<()> {
    glaze_theme::LoadedTheme::parse(PathBuf::new(), text)?;
    Ok(())
}

fn effects(dir: &Path) -> Vec<Effect> {
    // Later roots win. A theme's packaged copies come first, so built-ins (and their fixes)
    // win over old snapshots, as in the engine's `stack::effect_search`.
    let mut roots = vec![
        dir.join("effects"),
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../effects"),
    ];
    if let Ok(exe) = std::env::current_exe()
        && let Some(parent) = exe.parent()
    {
        roots.push(parent.join("effects"));
    }
    if let Ok(data) = std::env::var("APPDATA") {
        roots.push(PathBuf::from(data).join("glaze/effects"));
    }
    let mut found = std::collections::BTreeMap::new();
    for root in roots {
        for entry in std::fs::read_dir(root).into_iter().flatten().flatten() {
            let folder = entry.path();
            if let Ok(text) = std::fs::read_to_string(folder.join("effect.json"))
                && let Ok(manifest) = glaze_theme::stack::parse_effect(&text)
            {
                found.insert(
                    manifest.id.clone(),
                    Effect {
                        manifest,
                        dir: folder,
                    },
                );
            }
        }
    }
    let mut found: Vec<_> = found.into_values().collect();
    found.sort_by(|a, b| (a.category(), &a.name).cmp(&(b.category(), &b.name)));
    found
}

fn copy_tree(from: &Path, to: &Path) -> Result<()> {
    std::fs::create_dir_all(to)?;
    for entry in std::fs::read_dir(from)? {
        let entry = entry?;
        if entry.file_type()?.is_symlink() {
            bail!("linked files cannot be packaged");
        }
        if entry.file_type()?.is_dir() {
            copy_tree(&entry.path(), &to.join(entry.file_name()))?;
        } else {
            std::fs::copy(entry.path(), to.join(entry.file_name()))?;
        }
    }
    Ok(())
}

fn begin(app: &mut App, from: Option<usize>) -> Result<()> {
    let root = glaze_config::user_themes_dir()?;
    std::fs::create_dir_all(&root)?;
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)?
        .as_nanos();
    let dir = root.join(format!(".draft-{}-{stamp}", std::process::id()));
    let mut origin = None;
    let result = (|| -> Result<Editor> {
        let mut value = json!({"name":"Untitled wallpaper","hidden":true,"wallpaper":{"kind":"stack","fps":30,"scale":1.0,"base":{"kind":"color","color":"#202124"},"layers":[]}});
        if let Some(index) = from {
            let wallpaper = app
                .library
                .items
                .get(index)
                .context("wallpaper is no longer in the library")?;
            let text = std::fs::read_to_string(wallpaper.dir.join("theme.json"))?;
            validate_theme(&text)?;
            let theme: Value = serde_json::from_str(&text)?;
            if let Some(reason) = cannot_edit(wallpaper) {
                bail!(reason);
            }
            if theme.pointer("/wallpaper/kind").and_then(Value::as_str) == Some("stack") {
                value = theme;
                copy_tree(&wallpaper.dir, &dir)?;
                if wallpaper.editable() {
                    origin = Some(wallpaper.dir.clone());
                } else {
                    value["name"] = json!(format!("{} (edited)", wallpaper.name));
                }
            } else {
                // The wallpaper as it is becomes the base; its own fps and scale carry over.
                let fps = theme.pointer("/wallpaper/fps").and_then(Value::as_u64).unwrap_or(30);
                let scale = theme.pointer("/wallpaper/scale").and_then(Value::as_f64).unwrap_or(1.0);
                value = json!({"name": format!("{} (edited)", wallpaper.name), "hidden": true,
                    "wallpaper": {"kind": "stack", "fps": fps, "scale": scale.clamp(0.25, 1.0),
                        "base": {"kind": "theme", "theme": wallpaper.apply_as}, "layers": []}});
            }
            value["hidden"] = json!(true);
        }
        std::fs::create_dir_all(dir.join("media"))?;
        let stack: glaze_theme::StackWallpaper =
            serde_json::from_value(value["wallpaper"].clone())?;
        fill_defaults(&mut value["wallpaper"], &serde_json::to_value(stack)?);
        if !dir.join("credits.json").exists() {
            std::fs::write(dir.join("credits.json"), "[]")?;
        }
        crate::docs::write_atomic(
            &dir.join("theme.json"),
            serde_json::to_string_pretty(&value)?.as_bytes(),
        )?;
        let doc = app.docs.theme(&dir)?;
        Ok(Editor {
            effects: effects(&dir),
            dir: dir.clone(),
            doc,
            selected: None,
            category: None,
            shader: None,
            shader_origin: None,
            source: String::new(),
            diagnostics: vec![],
            pending_code: None,
            next_poll: Instant::now(),
            version: u64::MAX,
            preview_stamp: None,
            validation_key: None,
            drag: None,
            images_budget: app.images.borrow().budget(),
            origin: origin.clone(),
            initial: app.docs.doc(doc).raw.clone(),
            saved: origin.is_some().then(|| app.docs.doc(doc).raw.clone()),
            fresh: from.is_none(),
            hover: None,
            hover_since: None,
            hover_shown: None,
            owns_folder: true,
            _drop_target: None,
            preview: preview::Preview::default(),
        })
    })();
    match result {
        Ok(editor) => {
            app.panel.as_mut().unwrap().builder = Some(editor);
            // The canvas shows the engine's 1280x720 frame; the gallery a page of thumbnails.
            // Both fit in this budget, so nothing a frame paints is evicted while it paints.
            app.images.borrow_mut().set_budget(EDITOR_IMAGES);
            if !app.fixture {
                state(app)._drop_target = app
                    .window
                    .and_then(|window| drop_target::Target::new(window, app.thread));
            }
            app.run(Action::Panel(crate::panel::Command::Open(Page::Create)));
            Ok(())
        }
        Err(error) => {
            let _ = std::fs::remove_dir_all(&dir);
            Err(error)
        }
    }
}

pub fn run(app: &mut App, command: Command) {
    let result = execute(app, command);
    if let Err(error) = result {
        app.alert(format!("{error:#}"));
    }
    app.dirty = true;
}

fn execute(app: &mut App, command: Command) -> Result<()> {
    match command {
        Command::New => return begin(app, None),
        Command::Edit(index) => return begin(app, Some(index)),
        _ => {}
    }
    if active(app).is_none() {
        return Ok(());
    }
    match command {
        Command::Select(index) => {
            state(app).selected = index;
        }
        Command::Gallery => {
            state(app).effects = effects(&active(app).unwrap().dir);
            app.run(Action::Panel(crate::panel::Command::Open(Page::Effects)));
        }
        Command::Category(category) => state(app).category = category,
        Command::Base(kind) => {
            let base = match kind {
                "gradient" => {
                    json!({"kind":"gradient","stops":[{"color":"#151922","at":0.0},{"color":"#8b9aaa","at":1.0}],"angle":120,"type":"linear"})
                }
                "shader" => {
                    let path = active(app).unwrap().dir.join("base.wgsl");
                    std::fs::write(
                        path,
                        "// @param amount float 0..1 = 0.5 \"Amount\"\nfn wallpaper(frag: vec2f) -> vec4f {\n    let uv = frag / paper.resolution;\n    return vec4f(uv, params.amount, 1.0);\n}\n",
                    )?;
                    json!({"kind":"shader","source":"base.wgsl","params":{}})
                }
                "glsl" => {
                    std::fs::write(
                        active(app).unwrap().dir.join("base.glsl"),
                        "void mainImage(out vec4 color, in vec2 fragCoord) {\n    vec2 uv = fragCoord / iResolution.xy;\n    color = vec4(uv, 0.5 + 0.5 * sin(iTime), 1.0);\n}\n",
                    )?;
                    json!({"kind":"glsl","source":"base.glsl","author":"","license":"","url":""})
                }
                _ => json!({"kind":"color","color":"#202124"}),
            };
            app.set(&edit(app, "/wallpaper/base", "Background"), base);
            state(app).fresh = false;
            state(app).selected = None;
        }
        Command::GradientStop(add) => {
            let mut base = raw(app)["wallpaper"]["base"].clone();
            if let Some(stops) = base["stops"].as_array_mut() {
                if add {
                    stops.push(json!({"color":"#808080","at":0.5}));
                } else if stops.len() > 1 {
                    stops.pop();
                }
                stops.sort_by(|a, b| {
                    a["at"]
                        .as_f64()
                        .unwrap_or(0.0)
                        .total_cmp(&b["at"].as_f64().unwrap_or(0.0))
                });
                app.set(&edit(app, "/wallpaper/base", "Gradient stops"), base);
            }
        }
        Command::AddEffect(id) => {
            let mut layers = layers(app);
            let name = active(app)
                .unwrap()
                .effects
                .iter()
                .find(|e| e.id == id)
                .map(|e| e.name.clone())
                .unwrap_or_else(|| id.clone());
            let name = layer_label(&layers, &name);
            layers.push(json!({"id":name,"kind":"effect","effect":id,"enabled":true,"mix":1.0,"blend":"normal","params":{}}));
            set_layers(app, layers, "Add effect");
            // The document write above replaced any hover preview on disk.
            state(app).hover = None;
            state(app).hover_shown = None;
            state(app).selected = Some(layers_len(app).saturating_sub(1));
            app.run(Action::Panel(crate::panel::Command::Open(Page::Create)));
        }
        Command::Import(layer) => {
            if let Some(path) = crate::dialogs::pick(
                app.window,
                "Import wallpaper asset",
                false,
                &[(
                    "Images, videos, shaders and 3D models",
                    "*.png;*.jpg;*.jpeg;*.webp;*.mp4;*.webm;*.mov;*.wgsl;*.glsl;*.glb;*.gltf",
                )],
            ) {
                import(app, &path, layer)?;
            }
        }
        Command::Duplicate => {
            if let Some(i) = active(app).unwrap().selected {
                let mut layers = layers(app);
                if let Some(mut layer) = layers.get(i).cloned() {
                    layer["id"] = json!(layer_label(
                        &layers,
                        &format!("{} copy", layer["id"].as_str().unwrap_or("Layer"))
                    ));
                    layers.insert(i + 1, layer);
                    set_layers(app, layers, "Duplicate layer");
                    state(app).selected = Some(i + 1);
                }
            }
        }
        Command::Remove => {
            if let Some(i) = active(app).unwrap().selected {
                let mut layers = layers(app);
                if i < layers.len() {
                    layers.remove(i);
                    set_layers(app, layers, "Delete layer");
                    state(app).selected = None;
                }
            }
        }
        Command::Move(delta) => {
            if let Some(i) = active(app).unwrap().selected {
                let to =
                    (i as i32 + delta).clamp(0, layers_len(app).saturating_sub(1) as i32) as usize;
                reorder(app, i, to);
            }
        }
        Command::Drag(_) => {}
        Command::Shader => {
            let value = raw(app);
            let source = if let Some(i) = active(app).unwrap().selected {
                let id = value["wallpaper"]["layers"][i]["effect"]
                    .as_str()
                    .context("select a shader or effect first")?;
                let effect = active(app)
                    .unwrap()
                    .effects
                    .iter()
                    .find(|e| e.id == id)
                    .context("effect is missing")?;
                let custom = active(app).unwrap().dir.join("effects").join(id);
                if effect.dir != custom {
                    copy_tree(&effect.dir, &custom)?;
                }
                let manifest: Value =
                    serde_json::from_slice(&std::fs::read(custom.join("effect.json"))?)?;
                custom.join(
                    manifest["passes"][0]["source"]
                        .as_str()
                        .context("effect has no pass")?,
                )
            } else {
                active(app).unwrap().dir.join(
                    value["wallpaper"]["base"]["source"]
                        .as_str()
                        .context("choose a shader base first")?,
                )
            };
            let text = std::fs::read_to_string(&source)?;
            let working = active(app)
                .unwrap()
                .dir
                .join(".editor")
                .join(source.file_name().context("shader has no file name")?);
            crate::docs::write_atomic(&working, text.as_bytes())?;
            state(app).source = text;
            state(app).shader_origin = Some(source);
            state(app).shader = Some(working);
            app.texts.remove(&crate::actions::FieldId::ShaderCode);
            app.run(Action::Panel(crate::panel::Command::Open(Page::Shader)));
        }
        Command::External => {
            if let Some(path) = active(app).unwrap().shader.as_ref() {
                crate::system::open(&path.to_string_lossy());
            }
        }
        Command::Credits => crate::system::open(
            &active(app)
                .unwrap()
                .dir
                .join("credits.json")
                .to_string_lossy(),
        ),
        Command::Save(apply) => save(app, apply)?,
        Command::CustomEffect => {
            let id = format!("custom-{}", unique());
            let folder = active(app).unwrap().dir.join("effects").join(&id);
            std::fs::create_dir_all(&folder)?;
            std::fs::write(
                folder.join("effect.wgsl"),
                "fn effect(frag: vec2f, uv: vec2f) -> vec4f {\n    return sample_src(uv);\n}\n",
            )?;
            std::fs::write(
                folder.join("effect.json"),
                serde_json::to_vec_pretty(
                    &json!({"id":id,"name":"Custom effect","category":"stylize","description":"Your own WGSL pass","animated":false,"cost":0.2,"author":"","license":"","passes":[{"source":"effect.wgsl","scale":1.0}],"params":[]}),
                )?,
            )?;
            state(app).effects = effects(&active(app).unwrap().dir);
            execute(app, Command::AddEffect(id))?;
            execute(app, Command::Shader)?;
        }
        Command::CloseGallery => {
            app.run(Action::Panel(crate::panel::Command::Open(Page::Create)));
        }
        Command::SaveCopy => {
            let name = raw(app)["name"].as_str().unwrap_or("Wallpaper").to_owned();
            state(app).origin = None;
            app.set(&edit(app, "/name", "Name"), json!(format!("{name} copy")));
            save(app, false)?;
        }
        Command::Revert => {
            let initial = active(app).unwrap().initial.clone();
            app.set(&edit(app, "", "Revert"), initial);
            state(app).selected = None;
        }
        Command::Discard => {
            // Leaving the builder pages drops the draft (see `tick`).
            app.run(Action::Panel(crate::panel::Command::Open(Page::Wallpapers)));
        }
        Command::Fit(fit) => app.set(&edit(app, "/wallpaper/base/fit", "Fit"), json!(fit)),
        Command::Focus(x, y) => app.set(
            &edit(app, "/wallpaper/base/focus", "Position"),
            json!([f32::from(x) / 2.0, f32::from(y) / 2.0]),
        ),
        Command::BaseTheme(index) => {
            let wallpaper = app.library.items.get(index).context("wallpaper is no longer in the library")?;
            if let Some(reason) = cannot_be_base(wallpaper) {
                bail!(reason);
            }
            let base = json!({"kind": "theme", "theme": wallpaper.apply_as});
            app.set(&edit(app, "/wallpaper/base", "Background"), base);
            state(app).fresh = false;
        }
        Command::Pick(kind) => {
            let (title, filter) = match kind {
                "video" => ("Choose a video", "*.mp4;*.webm;*.mov"),
                "scene" => ("Choose a 3D model", "*.glb;*.gltf"),
                _ => ("Choose a photo", "*.png;*.jpg;*.jpeg;*.webp"),
            };
            if let Some(path) = crate::dialogs::pick(app.window, title, false, &[(title, filter)]) {
                import(app, &path, false)?;
                state(app).fresh = false;
            }
        }
        Command::DuplicateAt(i) | Command::RemoveAt(i) | Command::MoveAt(i, _) => {
            state(app).selected = Some(i);
            let then = match command {
                Command::DuplicateAt(_) => Command::Duplicate,
                Command::RemoveAt(_) => Command::Remove,
                Command::MoveAt(_, by) => Command::Move(by),
                _ => unreachable!(),
            };
            execute(app, then)?;
        }
        Command::New | Command::Edit(_) => {}
    }
    Ok(())
}

/// Why `wallpaper` can't open in the editor, or `None` when it can.
pub fn cannot_edit(wallpaper: &crate::library::Wallpaper) -> Option<&'static str> {
    use crate::library::Kind;
    match wallpaper.kind {
        None => Some("This wallpaper doesn't load, so it can't be edited."),
        Some(Kind::Web | Kind::App) => {
            Some("Web pages and apps draw in their own window, so they can't be edited.")
        }
        Some(_) => None,
    }
}

/// Why `wallpaper` can't be a background, or `None`. A composition is edited directly instead.
fn cannot_be_base(wallpaper: &crate::library::Wallpaper) -> Option<&'static str> {
    if wallpaper.kind == Some(crate::library::Kind::Stack) {
        return Some("A composition can't be the background of another; edit it instead.");
    }
    cannot_edit(wallpaper)
}

/// Wallpapers that can be a background, by library index.
pub fn base_choices(app: &App) -> Vec<usize> {
    (0..app.library.items.len())
        .filter(|&i| cannot_be_base(&app.library.items[i]).is_none())
        .collect()
}

/// Decoded-picture budget while the editor is open (Studio keeps 2 MB otherwise).
const EDITOR_IMAGES: usize = 24 << 20;

/// Delete, Escape: the editor's own keys. Ctrl+Z / Ctrl+Y are Studio's undo and redo.
pub fn on_key(app: &mut App, vk: u16) -> bool {
    if active(app).is_none() || app.field.is_some() {
        return false;
    }
    match (vk, crate::panel::current_page(app)) {
        (0x1B, Page::Effects | Page::Shader) => run(app, Command::CloseGallery),
        (0x2E, Page::Create) if active(app).unwrap().selected.is_some() => run(app, Command::Remove),
        _ => return false,
    }
    true
}

fn unique() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos()
}
fn layer_label(layers: &[Value], name: &str) -> String {
    let mut label = name.to_owned();
    let mut n = 2;
    while layers.iter().any(|l| l["id"].as_str() == Some(&label)) {
        label = format!("{name} {n}");
        n += 1;
    }
    label
}

fn fill_defaults(value: &mut Value, defaults: &Value) {
    match (value, defaults) {
        (Value::Object(value), Value::Object(defaults)) => {
            for (key, default) in defaults {
                if let Some(own) = value.get_mut(key) {
                    fill_defaults(own, default);
                } else {
                    value.insert(key.clone(), default.clone());
                }
            }
        }
        (Value::Array(value), Value::Array(defaults)) => {
            for (own, default) in value.iter_mut().zip(defaults) {
                fill_defaults(own, default);
            }
        }
        _ => {}
    }
}
pub fn layers(app: &App) -> Vec<Value> {
    raw(app)["wallpaper"]["layers"]
        .as_array()
        .cloned()
        .unwrap_or_default()
}

fn package_effects(app: &mut App) -> Result<()> {
    let editor = active(app).context("no composition is open")?;
    let mut copied = false;
    for layer in layers(app) {
        if let Some(effect) = editor
            .effects
            .iter()
            .find(|e| layer["effect"].as_str() == Some(&e.id))
        {
            let to = editor.dir.join("effects").join(&effect.id);
            if !to.exists() {
                copy_tree(&effect.dir, &to)?;
                copied = true;
            }
        }
    }
    if copied {
        state(app).effects = effects(&active(app).unwrap().dir);
    }
    Ok(())
}
fn layers_len(app: &App) -> usize {
    layers(app).len()
}
pub fn set_layers(app: &mut App, layers: Vec<Value>, label: &str) {
    app.set(&edit(app, "/wallpaper/layers", label), json!(layers));
}
pub fn reorder(app: &mut App, from: usize, to: usize) {
    let mut layers = layers(app);
    if glaze_ui::sortable::reorder(&mut layers, from, to) {
        set_layers(app, layers, "Reorder layers");
        state(app).selected = Some(to);
    }
}

pub fn import(app: &mut App, path: &Path, layer: bool) -> Result<()> {
    let extension = path
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    let kind = match extension.as_str() {
        "glb" | "gltf" => {
            if layer {
                "model"
            } else {
                "scene"
            }
        }
        "mp4" | "webm" | "mov" => "video",
        "wgsl" => "shader",
        "glsl" => "glsl",
        _ if crate::library::is_image(path) => "image",
        _ => bail!("unsupported asset"),
    };
    if layer && !matches!(kind, "model" | "image") {
        bail!("layers accept images and 3D models; use this file as a base");
    }
    let relative = format!(
        "media/{}/{}",
        unique(),
        path.file_name()
            .context("file has no name")?
            .to_string_lossy()
    );
    let dest = active(app)
        .context("open Create first")?
        .dir
        .join(&relative);
    std::fs::create_dir_all(dest.parent().context("asset folder is missing")?)?;
    std::fs::copy(path, &dest)?;
    if extension == "gltf" {
        let gltf: Value = serde_json::from_slice(&std::fs::read(path)?)?;
        for array in ["buffers", "images"] {
            for asset in gltf[array].as_array().into_iter().flatten() {
                if let Some(uri) = asset["uri"].as_str() {
                    if uri.starts_with("data:") {
                        continue;
                    }
                    let uri = Path::new(uri);
                    if uri.is_absolute()
                        || uri
                            .components()
                            .any(|c| !matches!(c, std::path::Component::Normal(_)))
                    {
                        bail!("glTF resources must be local relative files");
                    }
                    let target = dest.parent().unwrap().join(uri);
                    std::fs::create_dir_all(target.parent().unwrap())?;
                    std::fs::copy(path.parent().unwrap().join(uri), target)?;
                }
            }
        }
    }
    let value = if layer {
        let name = path.file_stem().and_then(|s| s.to_str()).unwrap_or("Layer");
        let mut layer = json!({"id":layer_label(&layers(app),name),"kind":kind,"source":relative,"rect":[0.1,0.1,0.5,0.5],"mix":1.0,"blend":"normal","enabled":true});
        if kind == "model" {
            layer["rotation"] = json!([0.0, 0.0, 0.0]);
            layer["light"] = json!([0.4, 0.8, 0.5]);
        }
        layer
    } else {
        match kind {
            "image" => json!({"kind":kind,"source":relative,"fit":"cover","focus":[0.5,0.5]}),
            "video" => json!({"kind":kind,"source":relative,"fit":"cover"}),
            "glsl" => json!({"kind":kind,"source":relative,"author":"","license":"","url":""}),
            "scene" => {
                json!({"kind":kind,"source":relative,"orbit":0.0,"parallax":0.3,"environment":"sky","exposure":1.0})
            }
            _ => json!({"kind":kind,"source":relative,"params":{}}),
        }
    };
    if layer {
        let mut layers = layers(app);
        layers.push(value);
        set_layers(app, layers, "Import layer");
        state(app).selected = Some(layers_len(app) - 1);
    } else {
        app.set(&edit(app, "/wallpaper/base", "Background"), value);
        state(app).fresh = false;
        state(app).selected = None;
    }
    let credits = active(app).unwrap().dir.join("credits.json");
    let mut items: Vec<Value> = std::fs::read(&credits)
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default();
    items.push(json!({"file":relative,"name":path.file_name().unwrap().to_string_lossy(),"author":"","license":"","url":""}));
    crate::docs::write_atomic(&credits, &serde_json::to_vec_pretty(&items)?)?;
    Ok(())
}

pub fn code(app: &App) -> String {
    active(app).map(|e| e.source.clone()).unwrap_or_default()
}
pub fn code_changed(app: &mut App, source: String) {
    // Windowless RichEdit returns paragraph breaks as bare CR, including pasted LF text.
    let source = source.replace("\r\n", "\n").replace('\r', "\n");
    if let Some(editor) = app.panel.as_mut().and_then(|s| s.builder.as_mut()) {
        if editor.source != source {
            editor.source = source;
            editor.pending_code = Some(Instant::now() + Duration::from_millis(100));
        }
    }
}

/// Source versions are immutable. Changing the theme's reference makes code edits use the
/// same undo/redo history as parameters, including edits from an external editor.
fn flush_code(app: &mut App) -> Result<()> {
    let editor = active(app).context("no composition is open")?;
    let Some(working) = editor.shader.clone() else {
        return Ok(());
    };
    let source = editor.source.clone();
    let selected = editor.selected;
    let dir = editor.dir.clone();
    crate::docs::write_atomic(&working, source.as_bytes())?;
    state(app).pending_code = None;
    let value = raw(app);
    let origin = if let Some(index) = selected {
        let id = value["wallpaper"]["layers"][index]["effect"]
            .as_str()
            .context("layer is no longer an effect")?;
        let effect = active(app)
            .unwrap()
            .effects
            .iter()
            .find(|e| e.id == id)
            .context("effect package is missing")?
            .clone();
        let mut manifest = effect.manifest.clone();
        manifest.id = format!("custom-{}", unique());
        let folder = dir.join("effects").join(&manifest.id);
        std::fs::create_dir_all(&folder)?;
        // Effect passes are self-contained WGSL; thumbnails are optional while editing.
        for pass in &manifest.passes {
            let to = folder.join(&pass.source);
            std::fs::create_dir_all(to.parent().unwrap())?;
            std::fs::copy(effect.dir.join(&pass.source), to)?;
        }
        let annotations = glaze_theme::stack::shader_params(&source);
        if !annotations.is_empty() {
            manifest.params = annotations;
        }
        let origin = folder.join(&manifest.passes[0].source);
        crate::docs::write_atomic(&origin, source.as_bytes())?;
        crate::docs::write_atomic(
            &folder.join("effect.json"),
            &serde_json::to_vec_pretty(&manifest)?,
        )?;
        let id = manifest.id.clone();
        state(app).effects.push(Effect {
            manifest,
            dir: folder,
        });
        app.set(
            &edit(
                app,
                format!("/wallpaper/layers/{index}/effect"),
                "Shader source",
            ),
            json!(id),
        );
        origin
    } else {
        let path = PathBuf::from("shaders")
            .join(unique().to_string())
            .join(working.file_name().context("shader has no file name")?);
        let origin = dir.join(&path);
        crate::docs::write_atomic(&origin, source.as_bytes())?;
        app.set(
            &edit(app, "/wallpaper/base/source", "Shader source"),
            json!(path.to_string_lossy().replace('\\', "/")),
        );
        origin
    };
    state(app).shader_origin = Some(origin);
    Ok(())
}

fn sync_code(app: &mut App) -> Result<()> {
    let editor = active(app).context("no composition is open")?;
    let value = raw(app);
    let origin = if let Some(index) = editor.selected {
        let effect = editor
            .effects
            .iter()
            .find(|e| value["wallpaper"]["layers"][index]["effect"].as_str() == Some(&e.id))
            .context("effect package is missing")?;
        effect.dir.join(&effect.passes[0].source)
    } else {
        editor.dir.join(
            value["wallpaper"]["base"]["source"]
                .as_str()
                .context("shader source is missing")?,
        )
    };
    if editor.shader_origin.as_ref() == Some(&origin) {
        return Ok(());
    }
    let source = std::fs::read_to_string(&origin)?;
    if let Some(working) = &editor.shader {
        crate::docs::write_atomic(working, source.as_bytes())?;
    }
    if let Some(field) = app
        .field
        .as_ref()
        .filter(|f| f.id == crate::actions::FieldId::ShaderCode)
    {
        field.field.set_text(&source)?;
    }
    state(app).source = source;
    state(app).shader_origin = Some(origin);
    app.dirty = true;
    Ok(())
}
pub fn deadline(app: &App) -> Option<Instant> {
    let editor = active(app)?;
    Some(
        editor
            .pending_code
            .map_or(editor.next_poll, |t| t.min(editor.next_poll)),
    )
}

pub fn tick(app: &mut App) {
    if active(app).is_none() || app.fixture {
        return;
    }
    if !is_page(crate::panel::current_page(app)) {
        let doc = active(app).unwrap().doc;
        let budget = active(app).unwrap().images_budget;
        app.panel.as_mut().unwrap().builder.take();
        if let Some(budget) = budget {
            app.images.borrow_mut().set_budget(budget);
        }
        // Reloading the removed transient file also retires its undo/redo steps.
        app.docs.reload(doc);
        return;
    }
    for (path, point) in drop_target::take() {
        let layer = crate::view::regions(app)
            .inspector
            .is_some_and(|r| r.contains(point));
        if let Err(error) = import(app, &path, layer) {
            app.alert(format!("Import: {error:#}"));
        }
    }
    let now = Instant::now();
    let due = active(app).unwrap().pending_code.is_some_and(|t| t <= now);
    if due {
        if let Err(error) = flush_code(app) {
            state(app).diagnostics = vec![Diagnostic {
                line: 1,
                message: error.to_string(),
            }];
        }
        app.dirty = true;
    }
    if active(app).unwrap().next_poll > now {
        return;
    }
    let version = app.docs.version;
    state(app).next_poll = now + Duration::from_millis(100);
    if active(app).unwrap().version != version {
        let doc = active(app).unwrap().doc;
        if matches!(app.drag, Some(crate::app::Drag::Slider { .. })) {
            if let Err(error) = app.docs.preview_live(doc) {
                app.alert(format!("Preview: {error:#}"));
            }
        }
        state(app).version = version;
        if crate::panel::current_page(app) == Page::Shader
            && active(app).unwrap().pending_code.is_none()
        {
            let _ = sync_code(app);
        }
        if let Err(error) = package_effects(app) {
            app.alert(format!("Effect package: {error:#}"));
        }
        app.dirty = true;
    }
    let dir = active(app).unwrap().dir.clone();
    let old_error = active(app).unwrap().preview.error.clone();
    state(app).preview.tick(&dir);
    if active(app).unwrap().preview.error != old_error {
        app.dirty = true;
    }
    if let Some(path) = active(app).unwrap().shader.clone()
        && !due
        && active(app).unwrap().pending_code.is_none()
    {
        if let Ok(source) = std::fs::read_to_string(path)
            && source != active(app).unwrap().source
        {
            code_changed(app, source);
            app.dirty = true;
        }
    }
    hover_preview(app, now);
    let snapshot = active(app).unwrap().dir.join("preview.jpg");
    let stamp = std::fs::metadata(&snapshot).ok().and_then(|m| m.modified().ok());
    if stamp != active(app).unwrap().preview_stamp {
        state(app).preview_stamp = stamp;
        // Only the canvas picture is decoded again; it stays on screen until the new one lands.
        // (Clearing the whole cache here blanked every picture at the engine's 10 fps.)
        app.images.borrow_mut().reload(&snapshot);
        app.dirty = true;
    }
    validate_code(app);
}

/// Runs the gallery card under the pointer on the canvas after a short pause, by writing the
/// draft's file with that effect on top. Undo history never sees it; leaving the card writes
/// the document back.
fn hover_preview(app: &mut App, now: Instant) {
    let hovered = (crate::panel::current_page(app) == Page::Effects)
        .then(|| app.input.hover)
        .flatten()
        .and_then(|k| {
            active(app)?
                .effects
                .iter()
                .find(|e| view::effect_key(&e.id) == k)
                .map(|e| e.id.clone())
        });
    if hovered != active(app).unwrap().hover {
        state(app).hover = hovered;
        state(app).hover_since = Some(now);
        app.dirty = true;
    }
    let editor = active(app).unwrap();
    let settled = editor.hover_since.is_none_or(|t| now >= t + Duration::from_millis(140));
    if editor.hover == editor.hover_shown || !settled {
        return;
    }
    let shown = editor.hover.clone();
    let result = match &shown {
        Some(id) => {
            let mut value = raw(app);
            if let Some(layers) = value["wallpaper"]["layers"].as_array_mut() {
                layers.push(json!({"id": "Preview", "kind": "effect", "effect": id}));
            }
            serde_json::to_string_pretty(&value).map_err(anyhow::Error::from).and_then(|text| {
                crate::docs::write_atomic(&active(app).unwrap().dir.join("theme.json"), text.as_bytes())
            })
        }
        None => app.docs.preview_live(active(app).unwrap().doc),
    };
    if let Err(error) = result {
        log::warn!("effect hover preview: {error:#}");
    }
    state(app).hover_shown = shown;
}

fn save(app: &mut App, apply: bool) -> Result<()> {
    if active(app).unwrap().pending_code.is_some() {
        flush_code(app)?;
    }
    let root = glaze_config::user_themes_dir()?;
    let target = match active(app).unwrap().origin.clone() {
        Some(dir) => storage::save_in_place(app, &root, &dir)?,
        None => storage::publish(app, &root)?,
    };
    // Later saves update this theme rather than making another.
    state(app).origin = Some(target.clone());
    state(app).saved = Some(raw(app));
    app.library = crate::library::Library::scan(crate::app::theme_name(&app.docs).as_deref());
    if apply {
        app.set(
            &Edit::new(DocId::Config, "/theme", "Apply wallpaper"),
            json!(target.to_string_lossy()),
        );
    }
    app.toast(
        if apply {
            "Wallpaper saved and applied"
        } else {
            "Wallpaper saved"
        },
        None,
    );
    Ok(())
}

pub use view::{page, stage};

pub fn shader_params(source: &str) -> Vec<Value> {
    glaze_theme::stack::shader_params(source)
        .iter()
        .filter_map(|p| serde_json::to_value(p).ok())
        .collect()
}

fn same_param(a: &Value, b: &Value) -> bool {
    match (a.as_f64(), b.as_f64()) {
        (Some(a), Some(b)) => (a - b).abs() <= 1e-6 * a.abs().max(b.abs()).max(1.0),
        _ => {
            a == b
                || a.as_str()
                    .zip(b.as_str())
                    .is_some_and(|(a, b)| a.starts_with('#') && a.eq_ignore_ascii_case(b))
        }
    }
}

fn validate_code(app: &mut App) {
    use glaze_theme::stack;
    use std::hash::{Hash, Hasher};
    let Some(editor) = active(app) else {
        return;
    };
    let Some(path) = &editor.shader else {
        return;
    };
    let errors_path = std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .map(|p| p.join("glaze/stack-errors.json"));
    let error_stamp = errors_path
        .as_ref()
        .and_then(|p| std::fs::metadata(p).ok())
        .and_then(|m| m.modified().ok());
    let mut hash = std::collections::hash_map::DefaultHasher::new();
    (&editor.source, path, app.docs.version, error_stamp).hash(&mut hash);
    let key = hash.finish();
    if editor.validation_key == Some(key) {
        return;
    }
    let source = editor.source.clone();
    let result = if path.extension().is_some_and(|e| e == "glsl") {
        stack::check::glsl(&source).map(drop)
    } else if let Some(effect) = editor
        .selected
        .and_then(|i| raw(app)["wallpaper"]["layers"].get(i).cloned())
        .and_then(|l| {
            editor
                .effects
                .iter()
                .find(|e| l["effect"].as_str() == Some(&e.id))
        })
    {
        stack::check::pass(&effect.manifest, &source).map(drop)
    } else {
        let values = serde_json::from_value(raw(app)["wallpaper"]["base"]["params"].clone())
            .unwrap_or_default();
        stack::check::shader_base(&source, &values).map(drop)
    };
    let mut errors: Vec<_> = result
        .err()
        .into_iter()
        .map(|d| Diagnostic {
            line: d.line.unwrap_or(1) as usize,
            message: d.message,
        })
        .collect();
    if let Ok(local) = std::env::var("LOCALAPPDATA")
        && let Ok(bytes) = std::fs::read(PathBuf::from(local).join("glaze/stack-errors.json"))
        && let Ok(value) = serde_json::from_slice::<Value>(&bytes)
    {
        let same_theme = value.get("dir").and_then(Value::as_str).map_or_else(
            || {
                value
                    .get("theme")
                    .and_then(Value::as_str)
                    .is_some_and(|id| editor.dir.file_name().is_some_and(|name| name == id))
            },
            |dir| Path::new(dir) == editor.dir,
        );
        if same_theme {
            collect_engine_errors(&value, &mut errors);
        }
    }
    let changed = errors.len() != editor.diagnostics.len()
        || errors
            .iter()
            .zip(&editor.diagnostics)
            .any(|(a, b)| a.line != b.line || a.message != b.message);
    state(app).validation_key = Some(key);
    if changed {
        state(app).diagnostics = errors;
        app.dirty = true;
    }
}

fn collect_engine_errors(value: &Value, errors: &mut Vec<Diagnostic>) {
    if let Some(message) = value.get("message").and_then(Value::as_str) {
        let line = value.get("line").and_then(Value::as_u64).unwrap_or(1) as usize;
        if !errors
            .iter()
            .any(|e| e.line == line && e.message == message)
        {
            errors.push(Diagnostic {
                line,
                message: message.into(),
            });
        }
    }
    match value {
        Value::Array(items) => {
            for item in items {
                collect_engine_errors(item, errors);
            }
        }
        Value::Object(map) => {
            for (key, item) in map {
                if matches!(key.as_str(), "errors" | "diagnostics") {
                    collect_engine_errors(item, errors);
                }
            }
        }
        _ => {}
    }
}

pub fn cost_line(app: &App) -> String {
    let value = raw(app);
    let Ok(stack) =
        serde_json::from_value::<glaze_theme::StackWallpaper>(value["wallpaper"].clone())
    else {
        return "Cost unavailable".into();
    };
    let Some(editor) = active(app) else {
        return String::new();
    };
    let pixels = app.display().map_or(1920.0 * 1080.0, |m| {
        f64::from(m.rect.width) * f64::from(m.rect.height)
    });
    estimate(&stack, &editor.effects, pixels)
}

/// The cost line for `stack` on a screen of `pixels`: only what runs every frame counts,
/// split exactly where the engine splits (`stack::per_frame_from`). Still layers before the
/// first animated one bake once; each effect's `cost` is GPU ms at 1920x1080.
fn estimate(stack: &glaze_theme::StackWallpaper, effects: &[Effect], pixels: f64) -> String {
    use glaze_theme::Layer;
    let find = |id: &str| effects.iter().find(|e| e.id == id);
    let mut moves = Vec::with_capacity(stack.layers.len());
    for layer in &stack.layers {
        moves.push(
            layer.active()
                && match layer {
                    Layer::Effect(l) => match find(&l.effect) {
                        Some(effect) => effect.animated,
                        None => return "Cost unavailable · missing effect".into(),
                    },
                    Layer::Model(l) => l.moves(),
                    Layer::Image(_) => false,
                },
        );
    }
    let Some(from) = glaze_theme::stack::per_frame_from(stack.base.moves(), moves) else {
        return "Still: no cost at rest".into();
    };
    let ms: f64 = stack.layers[from..]
        .iter()
        .filter(|l| l.active())
        .filter_map(|l| match l {
            Layer::Effect(l) => find(&l.effect).map(|e| f64::from(e.cost)),
            Layer::Image(_) | Layer::Model(_) => None,
        })
        .sum();
    if ms <= 0.0 {
        // Only the background moves; its own cost isn't measured per effect.
        return format!("Moving: the background runs at {} fps", stack.fps);
    }
    let resolution = pixels / (1920.0 * 1080.0);
    let percent = ms * f64::from(stack.fps) * f64::from(stack.scale).powi(2) * resolution / 10.0;
    format!("Moving: about {percent:.1}% GPU at {} fps", stack.fps)
}

/// Reordering consumes only events addressed to our own layer handles.
pub fn event(app: &mut App, event: &glaze_ui::Event) -> bool {
    use glaze_ui::{Button, Event};
    if active(app).is_none() {
        return false;
    }
    match event {
        Event::Press(pointer, Button::Left) => {
            let from = app
                .frame
                .targets
                .iter()
                .rev()
                .find_map(|t| match &t.action {
                    Action::Panel(crate::panel::Command::Builder(Command::Drag(i)))
                        if t.visible.is_some_and(|r| r.contains(pointer.at)) =>
                    {
                        Some(*i)
                    }
                    _ => None,
                });
            if let Some(from) = from {
                state(app).drag = Some(from);
                app.blur_field();
                if let Some(window) = app.window {
                    // SAFETY: capture belongs only to this Studio window and this drag.
                    unsafe {
                        windows::Win32::UI::Input::KeyboardAndMouse::SetCapture(window);
                    }
                }
                return true;
            }
        }
        Event::Move(pointer) if active(app).unwrap().drag.is_some() => {
            app.input.pointer = Some(pointer.at);
            return true;
        }
        Event::Release(pointer, Button::Left) if active(app).unwrap().drag.is_some() => {
            let from = state(app).drag.take().unwrap();
            let to = app
                .frame
                .targets
                .iter()
                .rev()
                .find_map(|t| match &t.action {
                    Action::Panel(crate::panel::Command::Builder(
                        Command::Drag(i) | Command::Select(Some(i)),
                    )) if t.visible.is_some_and(|r| r.contains(pointer.at)) => Some(*i),
                    _ => None,
                });
            if app.window.is_some() {
                let _ = unsafe { windows::Win32::UI::Input::KeyboardAndMouse::ReleaseCapture() };
            }
            if let Some(to) = to {
                reorder(app, from, to);
            }
            app.dirty = true;
            return true;
        }
        Event::CaptureLost | Event::Key(0x1B) if active(app).unwrap().drag.is_some() => {
            state(app).drag = None;
            if app.window.is_some() {
                // SAFETY: this editor acquired the capture above; cancellation releases it.
                let _ = unsafe { windows::Win32::UI::Input::KeyboardAndMouse::ReleaseCapture() };
            }
            app.dirty = true;
            return true;
        }
        _ => {}
    }
    false
}
