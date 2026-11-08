//! Wallpaper composition. Drafts are ordinary, hidden theme folders; edits use Studio's
//! document history, while imported assets remain local to the draft until Save.
mod view;
#[cfg(test)]
mod tests;

use std::{path::{Path, PathBuf}, time::{Duration, Instant}};
use anyhow::{Context, Result, bail};
use serde_json::{Value, json};
use crate::{actions::{Action, Edit}, app::App, docs::DocId, panel::Page};

#[derive(Debug)]
pub struct Editor {
    pub dir: PathBuf,
    pub doc: DocId,
    pub selected: Option<usize>,
    pub effects: Vec<Effect>,
    pub category: Option<String>,
    pub shader: Option<PathBuf>,
    pub source: String,
    pub diagnostics: Vec<Diagnostic>,
    pending_code: Option<Instant>,
    next_poll: Instant,
    version: u64,
    drag: Option<usize>,
    owns_folder: bool,
}

impl Drop for Editor {
    fn drop(&mut self) {
        if self.owns_folder && self.dir.file_name().is_some_and(|n| n.to_string_lossy().starts_with(".draft-")) {
            let _ = std::fs::remove_dir_all(&self.dir);
        }
    }
}

/// A resolved package; schema and validation belong to the engine's shared theme crate.
#[derive(Debug, Clone)]
pub struct Effect { pub manifest: mypaper_theme::EffectManifest, pub dir: PathBuf }
impl std::ops::Deref for Effect {
    type Target = mypaper_theme::EffectManifest;
    fn deref(&self) -> &Self::Target { &self.manifest }
}
impl Effect {
    pub fn category(&self) -> &str {
        use mypaper_theme::stack::Category;
        match self.manifest.category { Category::Weather=>"weather", Category::Camera=>"camera", Category::Color=>"color", Category::Motion=>"motion", Category::Stylize=>"stylize", Category::Light=>"light" }
    }
    pub fn schemas(&self) -> Vec<Value> { self.params.iter().filter_map(|p| serde_json::to_value(p).ok()).collect() }
}

#[derive(Debug, Clone)]
pub struct Diagnostic { pub line: usize, pub message: String }

#[derive(Debug, Clone, PartialEq)]
pub enum Command {
    New, Edit(usize), Select(Option<usize>), Gallery, Category(Option<String>),
    AddEffect(String), Base(&'static str), Import(bool), Duplicate, Remove,
    Move(i32), Drag(usize), Shader, External, Save(bool), CustomEffect,
}

pub fn active(app: &App) -> Option<&Editor> { app.panel.as_ref()?.builder.as_ref() }
pub fn raw(app: &App) -> Value { active(app).map(|e| app.docs.doc(e.doc).raw.clone()).unwrap_or(Value::Null) }
pub fn edit(app: &App, path: impl Into<String>, label: &str) -> Edit {
    Edit::new(active(app).expect("builder page owns a draft").doc, path.into(), label)
}
fn state(app: &mut App) -> &mut Editor { app.panel.as_mut().unwrap().builder.as_mut().unwrap() }
pub fn is_page(page: Page) -> bool { matches!(page, Page::Create | Page::Effects | Page::Shader) }

/// Use the engine's parser before a theme enters document history or Save.
pub fn validate_theme(text: &str) -> Result<()> {
    mypaper_theme::LoadedTheme::parse(PathBuf::new(), text)?;
    Ok(())
}

fn effects(dir: &Path) -> Vec<Effect> {
    let mut roots = vec![PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../effects")];
    if let Ok(exe) = std::env::current_exe() && let Some(parent) = exe.parent() { roots.push(parent.join("effects")); }
    if let Ok(data) = std::env::var("APPDATA") { roots.push(PathBuf::from(data).join("mypaper/effects")); }
    roots.push(dir.join("effects"));
    let mut found = std::collections::BTreeMap::new();
    for root in roots {
        for entry in std::fs::read_dir(root).into_iter().flatten().flatten() {
            let folder = entry.path();
            if let Ok(text) = std::fs::read_to_string(folder.join("effect.json")) && let Ok(manifest) = mypaper_theme::stack::parse_effect(&text) {
                found.insert(manifest.id.clone(), Effect { manifest, dir: folder });
            }
        }
    }
    let mut found: Vec<_> = found.into_values().collect();
    found.sort_by(|a,b| (a.category(), &a.name).cmp(&(b.category(), &b.name)));
    found
}

fn copy_tree(from: &Path, to: &Path) -> Result<()> {
    std::fs::create_dir_all(to)?;
    for entry in std::fs::read_dir(from)? {
        let entry = entry?;
        if entry.file_type()?.is_symlink() { bail!("linked files cannot be packaged"); }
        if entry.file_type()?.is_dir() { copy_tree(&entry.path(), &to.join(entry.file_name()))?; }
        else { std::fs::copy(entry.path(), to.join(entry.file_name()))?; }
    }
    Ok(())
}

fn begin(app: &mut App, from: Option<usize>) -> Result<()> {
    let root = mypaper_config::user_themes_dir()?;
    std::fs::create_dir_all(&root)?;
    let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)?.as_nanos();
    let dir = root.join(format!(".draft-{}-{stamp}", std::process::id()));
    let result = (|| -> Result<Editor> {
        let mut value = json!({"name":"Untitled wallpaper","hidden":true,"wallpaper":{"kind":"stack","fps":30,"scale":1.0,"base":{"kind":"color","color":"#202124"},"layers":[]}});
        if let Some(index) = from {
            let wallpaper = app.library.items.get(index).context("wallpaper is no longer in the library")?;
            let text = std::fs::read_to_string(wallpaper.dir.join("theme.json"))?;
            validate_theme(&text)?;
            value = serde_json::from_str(&text)?;
            if value.pointer("/wallpaper/kind").and_then(Value::as_str) != Some("stack") { bail!("only stack wallpapers open in Create"); }
            copy_tree(&wallpaper.dir, &dir)?;
            value["hidden"] = json!(true);
        }
        std::fs::create_dir_all(dir.join("media"))?;
        crate::docs::write_atomic(&dir.join("theme.json"), serde_json::to_string_pretty(&value)?.as_bytes())?;
        let doc = app.docs.theme(&dir)?;
        Ok(Editor { effects: effects(&dir), dir: dir.clone(), doc, selected: None, category: None, shader: None, source: String::new(), diagnostics: vec![], pending_code: None, next_poll: Instant::now(), version: u64::MAX, drag: None, owns_folder: true })
    })();
    match result {
        Ok(editor) => { app.panel.as_mut().unwrap().builder = Some(editor); app.run(Action::Panel(crate::panel::Command::Open(Page::Create))); Ok(()) }
        Err(error) => { let _ = std::fs::remove_dir_all(&dir); Err(error) }
    }
}

pub fn run(app: &mut App, command: Command) {
    let result = execute(app, command);
    if let Err(error) = result { app.alert(format!("{error:#}")); }
    app.dirty = true;
}

fn execute(app: &mut App, command: Command) -> Result<()> {
    match command {
        Command::New => return begin(app, None),
        Command::Edit(index) => return begin(app, Some(index)),
        _ => {},
    }
    if active(app).is_none() { return Ok(()); }
    match command {
        Command::Select(index) => { state(app).selected = index; }
        Command::Gallery => { state(app).effects = effects(&active(app).unwrap().dir); app.run(Action::Panel(crate::panel::Command::Open(Page::Effects))); }
        Command::Category(category) => state(app).category = category,
        Command::Base(kind) => {
            let base = match kind {
                "gradient" => json!({"kind":"gradient","stops":[{"color":"#151922","at":0.0},{"color":"#8b9aaa","at":1.0}],"angle":120,"type":"linear"}),
                "shader" => {
                    let path = active(app).unwrap().dir.join("base.wgsl");
                    std::fs::write(path, "// @param amount float 0..1 = 0.5 \"Amount\"\nfn wallpaper(frag: vec2f) -> vec4f {\n    let uv = frag / paper.resolution;\n    return vec4f(uv, params.amount, 1.0);\n}\n")?;
                    json!({"kind":"shader","source":"base.wgsl","params":{}})
                }
                _ => json!({"kind":"color","color":"#202124"}),
            };
            app.set(&edit(app,"/wallpaper/base","Base"),base);
            state(app).selected = None;
        }
        Command::AddEffect(id) => {
            let mut layers = layers(app);
            layers.push(json!({"id":format!("{id}-{}", unique()),"kind":"effect","effect":id,"enabled":true,"mix":1.0,"blend":"normal","params":{}}));
            set_layers(app,layers,"Add effect");
            state(app).selected = Some(layers_len(app).saturating_sub(1));
            app.run(Action::Panel(crate::panel::Command::Open(Page::Create)));
        }
        Command::Import(layer) => {
            if let Some(path) = crate::dialogs::pick(app.window,"Import wallpaper asset",false,&[("Images, videos, shaders and 3D models","*.png;*.jpg;*.jpeg;*.webp;*.mp4;*.webm;*.mov;*.wgsl;*.glsl;*.glb;*.gltf")]) { import(app,&path,layer)?; }
        }
        Command::Duplicate => {
            if let Some(i) = active(app).unwrap().selected {
                let mut layers = layers(app);
                if let Some(mut layer) = layers.get(i).cloned() { layer["id"] = json!(format!("layer-{}",unique())); layers.insert(i+1,layer); set_layers(app,layers,"Duplicate layer"); state(app).selected = Some(i+1); }
            }
        }
        Command::Remove => { if let Some(i) = active(app).unwrap().selected { let mut layers = layers(app); if i < layers.len() { layers.remove(i); set_layers(app,layers,"Delete layer"); state(app).selected = None; } } }
        Command::Move(delta) => { if let Some(i) = active(app).unwrap().selected { let to = (i as i32 + delta).clamp(0,layers_len(app).saturating_sub(1) as i32) as usize; reorder(app,i,to); } }
        Command::Drag(_) => {},
        Command::Shader => {
            let value = raw(app);
            let source = if let Some(i) = active(app).unwrap().selected {
                let id = value["wallpaper"]["layers"][i]["effect"].as_str().context("select a shader or effect first")?;
                let effect = active(app).unwrap().effects.iter().find(|e|e.id==id).context("effect is missing")?;
                let custom = active(app).unwrap().dir.join("effects").join(id);
                if effect.dir != custom { copy_tree(&effect.dir,&custom)?; }
                let manifest: Value = serde_json::from_slice(&std::fs::read(custom.join("effect.json"))?)?;
                custom.join(manifest["passes"][0]["source"].as_str().context("effect has no pass")?)
            } else { active(app).unwrap().dir.join(value["wallpaper"]["base"]["source"].as_str().context("choose a shader base first")?) };
            state(app).source = std::fs::read_to_string(&source)?;
            state(app).shader = Some(source);
            app.texts.remove(&crate::actions::FieldId::ShaderCode);
            app.run(Action::Panel(crate::panel::Command::Open(Page::Shader)));
        }
        Command::External => if let Some(path) = active(app).unwrap().shader.as_ref() { crate::system::open(&path.to_string_lossy()); },
        Command::Save(apply) => save(app,apply)?,
        Command::CustomEffect => {
            let id = format!("custom-{}",unique());
            let folder = active(app).unwrap().dir.join("effects").join(&id);
            std::fs::create_dir_all(&folder)?;
            std::fs::write(folder.join("effect.wgsl"), "fn effect(frag: vec2f, uv: vec2f) -> vec4f {\n    return sample_src(uv);\n}\n")?;
            std::fs::write(folder.join("effect.json"),serde_json::to_vec_pretty(&json!({"id":id,"name":"Custom effect","category":"stylize","description":"Your own WGSL pass","animated":false,"cost":0.2,"author":"","license":"","passes":[{"source":"effect.wgsl","scale":1.0}],"params":[]}))?)?;
            state(app).effects = effects(&active(app).unwrap().dir);
            execute(app,Command::AddEffect(id))?;
            execute(app,Command::Shader)?;
        }
        Command::New | Command::Edit(_) => {},
    }
    Ok(())
}

fn unique() -> u128 { std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_nanos() }
pub fn layers(app: &App) -> Vec<Value> { raw(app)["wallpaper"]["layers"].as_array().cloned().unwrap_or_default() }
fn layers_len(app: &App) -> usize { layers(app).len() }
pub fn set_layers(app: &mut App, layers: Vec<Value>, label: &str) { app.set(&edit(app,"/wallpaper/layers",label),json!(layers)); }
pub fn reorder(app: &mut App, from: usize, to: usize) { let mut layers=layers(app); if mypaper_ui::sortable::reorder(&mut layers,from,to) { set_layers(app,layers,"Reorder layers"); state(app).selected=Some(to); } }

pub fn import(app: &mut App, path: &Path, layer: bool) -> Result<()> {
    let extension = path.extension().and_then(|s|s.to_str()).unwrap_or("").to_ascii_lowercase();
    let kind = match extension.as_str() { "glb"|"gltf" => if layer {"model"} else {"scene"}, "mp4"|"webm"|"mov" => "video", "wgsl"=>"shader", "glsl"=>"glsl", _ if crate::library::is_image(path)=>"image", _=>bail!("unsupported asset") };
    if layer && !matches!(kind,"model"|"image") { bail!("layers accept images and 3D models; use this file as a base"); }
    let relative = format!("media/{}-{}",unique(),path.file_name().context("file has no name")?.to_string_lossy());
    let dest = active(app).context("open Create first")?.dir.join(&relative);
    std::fs::copy(path,&dest)?;
    if extension == "gltf" {
        let gltf: Value=serde_json::from_slice(&std::fs::read(path)?)?;
        for array in ["buffers","images"] { for asset in gltf[array].as_array().into_iter().flatten() {
            if let Some(uri)=asset["uri"].as_str() { if uri.starts_with("data:"){continue;} let uri=Path::new(uri); if uri.is_absolute() || uri.components().any(|c| !matches!(c,std::path::Component::Normal(_))) {bail!("glTF resources must be local relative files");} let target=dest.parent().unwrap().join(uri); std::fs::create_dir_all(target.parent().unwrap())?; std::fs::copy(path.parent().unwrap().join(uri),target)?; }
        } }
    }
    let value = if layer { json!({"id":format!("layer-{}",unique()),"kind":kind,"source":relative,"rect":[0.1,0.1,0.5,0.5],"mix":1.0,"blend":"normal","enabled":true}) }
    else { match kind {
        "image"=>json!({"kind":kind,"source":relative,"fit":"cover","focus":[0.5,0.5]}),
        "video"=>json!({"kind":kind,"source":relative,"fit":"cover"}),
        "glsl"=>json!({"kind":kind,"source":relative,"author":"","license":"","url":""}),
        "scene"=>json!({"kind":kind,"source":relative,"orbit":0.0,"parallax":0.3,"environment":"sky","exposure":1.0}),
        _=>json!({"kind":kind,"source":relative,"params":{}}),
    }};
    if layer { let mut layers=layers(app);layers.push(value);set_layers(app,layers,"Import layer");state(app).selected=Some(layers_len(app)-1); }
    else { app.set(&edit(app,"/wallpaper/base","Import base"),value);state(app).selected=None; }
    let credits=active(app).unwrap().dir.join("credits.json");
    let mut items: Vec<Value>=std::fs::read(&credits).ok().and_then(|b|serde_json::from_slice(&b).ok()).unwrap_or_default();
    items.push(json!({"file":relative,"name":path.file_name().unwrap().to_string_lossy(),"author":"","license":"","url":""}));
    crate::docs::write_atomic(&credits,&serde_json::to_vec_pretty(&items)?)?;
    Ok(())
}

pub fn code(app: &App) -> String { active(app).map(|e|e.source.clone()).unwrap_or_default() }
pub fn code_changed(app: &mut App, source: String) { if let Some(editor)=app.panel.as_mut().and_then(|s|s.builder.as_mut()) { if editor.source!=source {editor.source=source;editor.pending_code=Some(Instant::now()+Duration::from_millis(100));} } }
pub fn deadline(app: &App) -> Option<Instant> { let editor=active(app)?; Some(editor.pending_code.map_or(editor.next_poll,|t|t.min(editor.next_poll))) }

pub fn tick(app: &mut App) {
    if active(app).is_none() || app.fixture {return;}
    if !is_page(crate::panel::current_page(app)) { app.panel.as_mut().unwrap().builder.take(); return; }
    let now=Instant::now();
    let due=active(app).unwrap().pending_code.is_some_and(|t|t<=now);
    if due {
        let editor=state(app);editor.pending_code=None;
        if let Some(path)=&editor.shader { if let Err(error)=crate::docs::write_atomic(path,editor.source.as_bytes()) {editor.diagnostics=vec![Diagnostic{line:1,message:error.to_string()}];} }
        app.dirty=true;
    }
    if active(app).unwrap().next_poll>now {return;}
    let version=app.docs.version;
    let editor=state(app);editor.next_poll=now+Duration::from_millis(100);
    if editor.version!=version {editor.version=version;app.dirty=true;}
    if let Some(path)=active(app).unwrap().shader.clone() && !due && active(app).unwrap().pending_code.is_none() {
        if let Ok(source)=std::fs::read_to_string(path) && source!=active(app).unwrap().source {state(app).source=source;app.dirty=true;}
    }
}

fn save(app: &mut App, apply: bool) -> Result<()> {
    if active(app).unwrap().pending_code.is_some() { let editor=state(app); if let Some(path)=&editor.shader {crate::docs::write_atomic(path,editor.source.as_bytes())?;}editor.pending_code=None; }
    let mut value=raw(app);
    let name=value["name"].as_str().unwrap_or("").trim().to_owned();
    if name.is_empty() {bail!("give your wallpaper a name");}
    value["hidden"]=json!(false);
    // Store only deviations; defaults continue to evolve with each effect package.
    for layer in value["wallpaper"]["layers"].as_array_mut().context("missing layers")? {
        if let Some(effect)=active(app).unwrap().effects.iter().find(|e|layer["effect"].as_str()==Some(&e.id)) && let Some(params)=layer.get_mut("params").and_then(Value::as_object_mut) { params.retain(|id,v| !effect.schemas().iter().any(|p|p["id"].as_str()==Some(id.as_str()) && &p["default"]==v)); }
    }
    let text=serde_json::to_string_pretty(&value)?;
    validate_theme(&text)?;
    let slug=name.chars().map(|c|if c.is_ascii_alphanumeric(){c.to_ascii_lowercase()}else{'-'}).collect::<String>().split('-').filter(|s|!s.is_empty()).collect::<Vec<_>>().join("-");
    let root=mypaper_config::user_themes_dir()?;
    let slug=if slug.is_empty(){"wallpaper"}else{&slug};
    let mut target=root.join(slug);let mut n=2;
    while target.exists(){target=root.join(format!("{slug}-{n}"));n+=1;}
    let draft=active(app).unwrap().dir.clone();
    copy_tree(&draft,&target)?;
    // Package all referenced effects so a saved composition can travel with its theme.
    for layer in value["wallpaper"]["layers"].as_array().unwrap() {
        if let Some(effect)=active(app).unwrap().effects.iter().find(|e|layer["effect"].as_str()==Some(&e.id)) {let dest=target.join("effects").join(&effect.id);if !dest.exists(){copy_tree(&effect.dir,&dest)?;}}
    }
    crate::docs::write_atomic(&target.join("theme.json"),text.as_bytes())?;
    app.library=crate::library::Library::scan(crate::app::theme_name(&app.docs).as_deref());
    if apply {app.set(&Edit::new(DocId::Config,"/theme","Apply wallpaper"),json!(target.to_string_lossy()));}
    app.toast(if apply {"Wallpaper saved and applied"}else{"Wallpaper saved"},None);
    Ok(())
}

pub use view::{page, stage};

pub fn shader_params(source: &str) -> Vec<Value> {
    mypaper_theme::stack::shader_params(source).iter().filter_map(|p|serde_json::to_value(p).ok()).collect()
}

pub fn cost_line(app: &App) -> String {
    let value=raw(app);
    let Ok(stack)=serde_json::from_value::<mypaper_theme::StackWallpaper>(value["wallpaper"].clone()) else {return "Cost unavailable".into();};
    let Some(editor)=active(app) else{return String::new();};
    let mut moving=stack.base.moves();let mut ms=0.0;
    for layer in &stack.layers {
        if !layer.enabled() || layer.mix()==0.0 {continue;}
        match layer {
            mypaper_theme::Layer::Effect(layer)=>if let Some(effect)=editor.effects.iter().find(|e|e.id==layer.effect){moving|=effect.animated;ms+=f64::from(effect.cost);},
            mypaper_theme::Layer::Model(layer)=>moving|=layer.moves(),
            mypaper_theme::Layer::Image(_)=>{},
        }
    }
    if !moving{return "Still: no cost at rest".into();}
    let resolution=app.display().map_or(1.0,|m|f64::from(m.rect.width)*f64::from(m.rect.height)/(1920.0*1080.0));
    let percent=ms*f64::from(stack.fps)*f64::from(stack.scale).powi(2)*resolution/10.0;
    format!("about {percent:.1}% GPU at {} fps",stack.fps)
}

/// Reordering consumes only events addressed to our own layer handles.
pub fn event(app: &mut App, event: &mypaper_ui::Event) -> bool {
    use mypaper_ui::{Event,Button};
    if active(app).is_none(){return false;}
    match event {
        Event::Press(pointer,Button::Left)=>{
            let from=app.frame.targets.iter().rev().find_map(|t|match &t.action{
                Action::Panel(crate::panel::Command::Builder(Command::Drag(i))) if t.visible.is_some_and(|r|r.contains(pointer.at))=>Some(*i),_=>None,
            });
            if let Some(from)=from {state(app).drag=Some(from);if let Some(window)=app.window{unsafe{windows::Win32::UI::Input::KeyboardAndMouse::SetCapture(window);}}return true;}
        }
        Event::Move(pointer) if active(app).unwrap().drag.is_some()=>{app.input.pointer=Some(pointer.at);return true;}
        Event::Release(pointer,Button::Left) if active(app).unwrap().drag.is_some()=>{
            let from=state(app).drag.take().unwrap();
            let to=app.frame.targets.iter().rev().find_map(|t|match &t.action{
                Action::Panel(crate::panel::Command::Builder(Command::Drag(i)|Command::Select(Some(i)))) if t.visible.is_some_and(|r|r.contains(pointer.at))=>Some(*i),_=>None,
            });
            if app.window.is_some(){let _=unsafe{windows::Win32::UI::Input::KeyboardAndMouse::ReleaseCapture()};}
            if let Some(to)=to{reorder(app,from,to);}app.dirty=true;return true;
        }
        Event::CaptureLost|Event::Key(0x1B) if active(app).unwrap().drag.is_some()=>{state(app).drag=None;return true;}
        _=>{},
    }
    false
}
