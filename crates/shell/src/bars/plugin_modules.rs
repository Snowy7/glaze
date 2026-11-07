//! Plugin bar modules (docs/plugins.md, "Bar modules"): a component plugin's `bars` contribution
//! placed in a bar as `custom:<plugin>--<bar>`. The bars say which are placed ([`place`]), the
//! widgets host opens each as a plugin instance `bar:<plugin>--<bar>` and hands its views back
//! ([`sync`]), and the bars draw the view's text and icon as the module's label. A click is the
//! view's first action (or `click`), routed back to the plugin as an action, unless the module
//! names a `panel`: then the click opens that panel as a flyout (`flyouts::plugin_panel`), which
//! the host opens as the instance `panel:<plugin>--<panel>` while it is on screen.
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
    /// Placed module → the panel it opens (`hello--details`), from its manifest.
    module_panels: BTreeMap<String, String>,
    /// Panels on screen (`<plugin>--<panel>`) and their latest views.
    panels: BTreeSet<String>,
    panels_opened: BTreeSet<String>,
    panel_views: BTreeMap<String, Face>,
    /// `(panel instance, action)` clicked in a panel since the host last synced.
    panel_clicks: Vec<(String, String)>,
    /// Interned panel ids, so `flyouts::Kind::Plugin` stays `Copy`.
    names: Vec<String>,
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

/// The plugin module ids placed in `config`'s bars: the top bar's zones and the dock's
/// `left`/`right` modules (a plugin dock module is the same `custom:<plugin>--<bar>`).
pub fn place(config: &BarsConfig) {
    let placed: BTreeSet<String> = [&config.top.left, &config.top.center, &config.top.right, &config.dock.left, &config.dock.right]
        .into_iter()
        .flatten()
        .filter_map(|m| match m {
            ModuleRef::Custom(id) if parse(id).is_some() && config.modules.get(m).is_none_or(|o| o.command.is_none()) => Some(id.clone()),
            _ => None,
        })
        .collect();
    let module_panels = placed.iter().filter_map(|id| {
        let (plugin, bar) = parse(id)?;
        let (manifest, _) = mypaper_widgets::registry::find_plugin(plugin)?;
        let panel = manifest.bars.iter().find(|b| b.id == bar)?.panel.clone()?;
        manifest.panels.iter().any(|p| p.id == panel).then(|| (id.clone(), format!("{plugin}--{panel}")))
    }).collect();
    with(|s| {
        s.placed = placed;
        s.module_panels = module_panels;
    });
}

/// The interned number of panel `id` (`<plugin>--<panel>`), for `flyouts::Kind::Plugin`.
pub fn panel_number(id: &str) -> u16 {
    with(|s| match s.names.iter().position(|n| n == id) {
        Some(n) => n as u16,
        None => {
            s.names.push(id.to_owned());
            (s.names.len() - 1) as u16
        }
    })
}

/// The panel id behind `panel_number`.
pub fn panel_name(number: u16) -> Option<String> {
    with(|s| s.names.get(number as usize).cloned())
}

/// A panel flyout opened (`true`) or closed: the host opens or closes its instance.
pub fn show_panel(id: &str, shown: bool) {
    with(|s| if shown { s.panels.insert(id.to_owned()) } else { s.panel_views.remove(id); s.panels.remove(id) });
}

/// The panel's latest view.
pub fn panel_view(id: &str) -> Option<Face> {
    with(|s| s.panel_views.get(id).cloned())
}

/// A click on a panel's action.
pub fn panel_click(id: &str, action: String) {
    with(|s| s.panel_clicks.push((panel_instance(id), action)));
}

/// The instance id of panel `id` in the plugin host.
pub fn panel_instance(id: &str) -> String {
    format!("panel:{id}")
}

/// The panel a click on module `id` opens, if it names one.
pub fn panel_of(id: &str) -> Option<String> {
    with(|s| s.module_panels.get(id).cloned())
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

pub(super) fn action_name(id: &str) -> Option<String> {
    with(|s| s.placed.contains(id).then(|| {
        s.views.get(id).and_then(|face| first_action(&face.body)).unwrap_or_else(|| "click".into())
    }))
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

/// What the widgets host does each tick, by instance id ([`instance`], [`panel_instance`]):
/// `(instance, plugin, face)` to open, instances to close, and `(instance, action)` clicks to send.
pub struct Sync {
    pub open: Vec<(String, String, String)>,
    pub close: Vec<String>,
    pub clicks: Vec<(String, String)>,
}

/// [`sync`] for the panels on screen; a changed view returns `true` in `.1` (redraw the flyout).
pub fn sync_panels(view: impl Fn(&str) -> Option<Face>) -> (Sync, bool) {
    with(|s| {
        let opened = std::mem::replace(&mut s.panels_opened, s.panels.clone());
        let mut changed = false;
        for id in &s.panels {
            if let Some(latest) = view(&panel_instance(id)).filter(|f| s.panel_views.get(id) != Some(f)) {
                s.panel_views.insert(id.clone(), latest);
                changed = true;
            }
        }
        let sync = Sync {
            open: s.panels.iter().filter(|id| !opened.contains(*id)).filter_map(|id| parse(id).map(|(p, f)| (panel_instance(id), p.to_owned(), f.to_owned()))).collect(),
            close: opened.iter().filter(|id| !s.panels.contains(*id)).map(|id| panel_instance(id)).collect(),
            clicks: std::mem::take(&mut s.panel_clicks),
        };
        (sync, changed)
    })
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
            if let Some(latest) = latest
                && s.views.get(id) != Some(&latest)
            {
                s.views.insert(id.clone(), latest);
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
