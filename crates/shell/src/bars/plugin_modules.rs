//! Plugin bar modules (docs/plugins.md, "Bar modules"): a component plugin's `bars` contribution
//! placed in a bar as `custom:<plugin>--<bar>`. The bars say which are placed ([`place`]), the
//! widgets host opens each as a plugin instance `bar:<plugin>--<bar>` and hands its views back
//! ([`sync`]), and the bars draw the view's text and icon as the module's label. A click is the
//! view's first action (or `click`), routed back to the plugin as an action.
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};

use mypaper_bars::{BarsConfig, ModuleRef};
use mypaper_plugin_sdk::{Face, View};

#[derive(Default)]
struct Shared {
    /// Placed module ids (`hello--clock`).
    placed: BTreeSet<String>,
    /// The ids the host was last told to have open.
    opened: BTreeSet<String>,
    /// Their latest views.
    views: BTreeMap<String, Face>,
    /// Actions clicked since the host last synced: `(module, action)`.
    clicks: Vec<(String, String)>,
}

static SHARED: Mutex<Option<Shared>> = Mutex::new(None);
static CHANGED: AtomicBool = AtomicBool::new(false);

fn with<T>(f: impl FnOnce(&mut Shared) -> T) -> T {
    f(SHARED.lock().expect("plugin modules").get_or_insert_default())
}

/// `<plugin>--<bar>` split, for a custom module id that names a plugin's bar module.
pub fn parse(id: &str) -> Option<(&str, &str)> {
    id.split_once("--").filter(|(p, b)| !p.is_empty() && !b.is_empty())
}

/// The plugin module ids placed in `config`'s bars (both bars, every zone).
pub fn place(config: &BarsConfig) {
    let placed: BTreeSet<String> = [&config.top.left, &config.top.center, &config.top.right]
        .into_iter()
        .flatten()
        .filter_map(|m| match m {
            ModuleRef::Custom(id) if parse(id).is_some() && config.modules.get(m).is_none_or(|o| o.command.is_none()) => Some(id.clone()),
            _ => None,
        })
        .collect();
    with(|s| s.placed = placed);
}

/// What the bar shows for module `id`: its view's text, if the plugin drew one.
pub fn label(id: &str) -> Option<String> {
    with(|s| s.views.get(id).map(|face| text(&face.body)))
}

/// The view's text nodes, joined: a bar module is one line.
fn text(view: &View) -> String {
    match view {
        View::Text { text, .. } => text.clone(),
        View::Stack { children, .. } | View::Row { children, .. } | View::List { children, .. } | View::Grid { children, .. } => {
            children.iter().map(text).filter(|t| !t.is_empty()).collect::<Vec<_>>().join(" ")
        }
        View::Card { child, .. } | View::Animated { child, .. } => text(child),
        View::Metric { value, .. } | View::Stat { value, .. } => value.clone(),
        _ => String::new(),
    }
}

/// The first action in a view (what a click on the module does), or `click`.
fn first_action(view: &View) -> Option<String> {
    match view {
        View::Button { action, .. } | View::IconButton { action, .. } => Some(action.clone()),
        View::ListRow { action, .. } => action.clone(),
        View::Stack { children, .. } | View::Row { children, .. } | View::List { children, .. } | View::Grid { children, .. } => {
            children.iter().find_map(first_action)
        }
        View::Card { child, .. } | View::Animated { child, .. } => first_action(child),
        _ => None,
    }
}

/// A click on module `id`; whether it is a plugin module (and so handled).
pub fn click(id: &str) -> bool {
    with(|s| {
        if !s.placed.contains(id) {
            return false;
        }
        let action = s.views.get(id).and_then(|f| first_action(&f.body)).unwrap_or_else(|| "click".into());
        s.clicks.push((id.to_owned(), action));
        true
    })
}

/// Whether a module's view changed since the bars last asked.
pub fn take_changed() -> bool {
    CHANGED.swap(false, Ordering::Relaxed)
}

/// What the widgets host does each tick, by instance id ([`instance`]): `(instance, plugin, bar)`
/// to open, instances to close, and `(instance, action)` clicks to send.
pub struct Sync {
    pub open: Vec<(String, String, String)>,
    pub close: Vec<String>,
    pub clicks: Vec<(String, String)>,
}

/// The instance id of module `id` in the plugin host.
pub fn instance(id: &str) -> String {
    format!("bar:{id}")
}

/// Takes the views of the placed modules (`view` reads an instance's latest face) and hands back
/// what the host must open, close and send. An open is asked for once per placement.
pub fn sync(view: impl Fn(&str) -> Option<Face>) -> Sync {
    with(|s| {
        let open = std::mem::replace(&mut s.opened, s.placed.clone());
        let mut changed = false;
        for id in &s.placed {
            let latest = view(&instance(id));
            if latest.is_some() && s.views.get(id) != latest.as_ref() {
                s.views.insert(id.clone(), latest.expect("checked"));
                changed = true;
            }
        }
        s.views.retain(|id, _| s.placed.contains(id));
        if changed {
            CHANGED.store(true, Ordering::Relaxed);
        }
        Sync {
            open: s
                .placed
                .iter()
                .filter(|id| !open.contains(*id))
                .filter_map(|id| parse(id).map(|(p, b)| (instance(id), p.to_owned(), b.to_owned())))
                .collect(),
            close: open.iter().filter(|id| !s.placed.contains(*id)).map(|id| instance(id)).collect(),
            clicks: std::mem::take(&mut s.clicks).into_iter().map(|(id, action)| (instance(&id), action)).collect(),
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use mypaper_plugin_sdk::Role;

    #[test]
    fn a_module_shows_its_text_and_clicks_its_first_action() {
        let face = View::row([View::text("hello · 3", Role::Caption), View::button("Tint", "tint")]).face();
        assert_eq!(text(&face.body), "hello · 3");
        assert_eq!(first_action(&face.body).as_deref(), Some("tint"));
        assert_eq!(parse("hello--clock"), Some(("hello", "clock")));
        assert_eq!(parse("weather"), None);
    }
}
