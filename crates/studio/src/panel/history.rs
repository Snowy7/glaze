//! Session history stores only desktop documents. Restoring leaves Settings and Connections alone.

use serde_json::Value;

use crate::app::App;
use crate::docs::{DocId, Op};

#[derive(Debug, Clone, PartialEq)]
pub struct Desktop {
    config: Vec<(&'static str, Option<Value>)>,
    bars: Value,
    widgets: Value,
}

impl Desktop {
    fn read(app: &App) -> Self {
        Self {
            config: ["/theme", "/look", "/desktop_menu"].into_iter()
                .map(|path| (path, app.docs.doc(DocId::Config).raw.pointer(path).cloned())).collect(),
            bars: app.docs.doc(DocId::Bars).raw.clone(),
            widgets: app.docs.doc(DocId::Widgets).raw.clone(),
        }
    }

    fn ops(&self) -> Vec<Op> {
        let mut ops: Vec<_> = self.config.iter().map(|(path, value)| Op {
            doc: DocId::Config, path: (*path).into(), value: value.clone(),
        }).collect();
        ops.extend([
            Op { doc: DocId::Bars, path: String::new(), value: Some(self.bars.clone()) },
            Op { doc: DocId::Widgets, path: String::new(), value: Some(self.widgets.clone()) },
        ]);
        ops
    }
}

#[derive(Debug, Clone)]
pub struct Entry {
    pub title: String,
    pub detail: String,
    desktop: Desktop,
}

#[derive(Debug, Default)]
pub struct History {
    pub entries: Vec<Entry>,
    pub selected: Option<usize>,
    last: Option<Desktop>,
}

pub fn observe(app: &App) {
    let Some(state) = &app.panel else { return };
    let desktop = Desktop::read(app);
    let mut history = state.history.borrow_mut();
    if history.last.as_ref() == Some(&desktop) { return }
    let mut changed = Vec::new();
    if let Some(last) = &history.last {
        for ((path, value), (_, before)) in desktop.config.iter().zip(&last.config) {
            if value != before { changed.push(match *path { "/theme" => "Wallpaper", "/look" => "Look", _ => "Desktop menu" }); }
        }
        if desktop.bars != last.bars { changed.push("Bars"); }
        if desktop.widgets != last.widgets { changed.push("Widgets"); }
    }
    let title = if changed.is_empty() { "When Studio opened".into() } else { changed.join(" · ") };
    let detail = app.library.active().map_or("Your desktop".to_owned(), |i| app.library.items[i].name.clone());
    history.entries.push(Entry { title, detail, desktop: desktop.clone() });
    // Session bounded independently of the undo stack. Existing Studios never read this state.
    if history.entries.len() > 100 { history.entries.remove(0); }
    history.last = Some(desktop);
}

pub fn restore(app: &mut App) {
    let selected = app.panel.as_ref().and_then(|state| {
        let history = state.history.borrow();
        history.selected.and_then(|i| history.entries.get(i)).cloned()
    });
    let Some(entry) = selected else { return };
    app.apply("Restore this desktop", entry.desktop.ops());
    app.panel.as_ref().unwrap().history.borrow_mut().selected = None;
    app.toast_undo("Restored this desktop");
}
