//! Visible custom widgets own bounded, short-lived data workers and their explicit refresh deadlines.
use crate::widgets::model::Model as WidgetModel;
use mypaper_ui::{HitId, Image};
use mypaper_widgets::custom::{Definition, Template};
use serde_json::Value;
use std::{
    sync::Arc,
    time::{Duration, Instant},
};

pub const REFRESH: HitId = 7200;
pub const EDIT: HitId = 7201;
#[derive(Default)]
pub struct Model {
    pub definition: Option<Definition>,
    pub value: Value,
    pub image: Option<Arc<Image>>,
    pub error: Option<String>,
    pub next: Option<Instant>,
    task: Option<super::Task<Loaded>>,
    loaded: bool,
}
struct Loaded {
    definition: Definition,
    value: Value,
    image: Option<Arc<Image>>,
}
impl Model {
    pub fn busy(&self) -> bool {
        self.task.is_some()
    }
}
fn load(id: &str) -> anyhow::Result<Loaded> {
    let definition = mypaper_widgets::custom::load(id)?;
    let value = mypaper_widgets::custom::evaluate(id, &definition)?;
    let image = if definition.layout == Template::Image {
        let location = value
            .as_str()
            .or(value["url"].as_str())
            .ok_or_else(|| anyhow::anyhow!("An image template needs an image path or HTTPS URL"))?;
        let bytes = if location.starts_with("https://") {
            mypaper_integrations::calendar_http::request(
                "GET",
                location,
                &[],
                &[],
                8 * 1024 * 1024,
            )?
        } else {
            use std::io::Read;
            let path = std::path::Path::new(location);
            let path = if path.is_absolute() {
                path.to_owned()
            } else {
                mypaper_widgets::custom::folder()?.join(path)
            };
            let file = std::fs::File::open(path)?;
            anyhow::ensure!(
                file.metadata()?.len() <= 8 * 1024 * 1024,
                "Image exceeds 8 MiB"
            );
            let mut bytes = Vec::new();
            file.take(8 * 1024 * 1024 + 1).read_to_end(&mut bytes)?;
            anyhow::ensure!(bytes.len() <= 8 * 1024 * 1024, "Image exceeds 8 MiB");
            bytes
        };
        let ui = mypaper_ui::Ui::new()?;
        Some(Arc::new(ui.decode_image(&bytes, (1024, 1024))?))
    } else {
        None
    };
    Ok(Loaded {
        definition,
        value,
        image,
    })
}
pub fn sync(widget: &mut WidgetModel, active: bool) {
    let state = &mut widget.integration.custom;
    if !active {
        state.next = None;
        state.task = None;
        return;
    }
    if let Some(result) = state.task.as_ref().and_then(super::Task::take) {
        state.task = None;
        state.loaded = true;
        match result {
            Ok(loaded) => {
                state.definition = Some(loaded.definition);
                state.value = loaded.value;
                state.image = loaded.image;
                state.error = None;
            }
            Err(error) => state.error = Some(error),
        }
        state.next = state
            .definition
            .as_ref()
            .filter(|d| d.refresh_seconds > 0)
            .map(|d| Instant::now() + Duration::from_secs(d.refresh_seconds));
        widget.dirty = true;
    }
    if state.task.is_none()
        && (!state.loaded || state.next.is_some_and(|next| next <= Instant::now()))
    {
        let id = widget.placed.widget.clone();
        state.next = None;
        state.task = Some(super::Task::spawn(move || load(&id)));
        widget.dirty = true;
    }
    if state.loaded && state.next.is_none() && state.task.is_none() {
        state.next = state
            .definition
            .as_ref()
            .filter(|d| d.refresh_seconds > 0)
            .map(|d| Instant::now() + Duration::from_secs(d.refresh_seconds));
    }
}
pub fn action(widget: &mut WidgetModel, hit: HitId) -> bool {
    let state = &mut widget.integration.custom;
    match hit {
        REFRESH => {
            state.loaded = false;
            state.next = None;
        }
        EDIT => match mypaper_widgets::custom::path(&widget.placed.widget) {
            Ok(path) => {
                let result =
                    mypaper_integrations::launcher::open(&mypaper_integrations::launcher::Recent {
                        title: String::new(),
                        target: path.to_string_lossy().into_owned(),
                        arguments: String::new(),
                    });
                state.error = result.err().map(|e| e.to_string());
            }
            Err(e) => state.error = Some(e.to_string()),
        },
        _ => {}
    }
    true
}
