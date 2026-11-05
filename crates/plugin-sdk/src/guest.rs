//! The WebAssembly guest SDK (docs/plugins.md, "Writing one"): build views with the same
//! [`View`] builders native plugins use, implement [`Plugin`], and call [`export!`]. Build with
//! `cargo build --release --target wasm32-wasip2`.
//!
//! Capabilities are the WIT imports, re-exported typed: [`ui`], [`style`], [`timers`],
//! [`events`], [`http`], [`launch`], [`notify`], [`secrets`], [`storage`], [`system`],
//! [`native`], [`log`]. I/O replies come back as [`Event`]s.

use std::any::Any;
use std::cell::RefCell;

use crate::view::{Align, Face, Role, Tone, View};

#[doc(hidden)]
pub mod bindings {
    wit_bindgen::generate!({
        path: "../plugin-api/wit",
        world: "plugin",
        pub_export_macro: true,
        export_macro_name: "__export_plugin",
        default_bindings_module: "mypaper_plugin_sdk::guest::bindings",
    });
}

pub use bindings::exports::mypaper::plugin::lifecycle::{Action, Event, FaceOpen, InitInfo};
pub use bindings::mypaper::plugin::types::{Error, SizeClass};
pub use bindings::mypaper::plugin::{events, http, launch, native, notify, secrets, storage, style, system, timers};

/// A plugin. One value lives for the instance's lifetime; `save` is what survives an unload.
pub trait Plugin: Default + 'static {
    /// `state` is what [`Plugin::save`] returned before the last unload.
    fn init(&mut self, _info: &InitInfo, _state: Option<&[u8]>) -> Result<(), String> {
        Ok(())
    }
    fn on_event(&mut self, event: Event);
    fn save(&mut self) -> Vec<u8> {
        Vec::new()
    }
}

thread_local! {
    static PLUGIN: RefCell<Option<Box<dyn Any>>> = const { RefCell::new(None) };
}

fn with<T: Plugin, R>(f: impl FnOnce(&mut T) -> R) -> R {
    PLUGIN.with_borrow_mut(|slot| {
        let plugin = slot.get_or_insert_with(|| Box::new(T::default()));
        f(plugin.downcast_mut::<T>().expect("one plugin type per component"))
    })
}

#[doc(hidden)]
pub fn __init<T: Plugin>(info: InitInfo) -> Result<(), String> {
    with::<T, _>(|p| p.init(&info, info.state.as_deref()))
}
#[doc(hidden)]
pub fn __on_event<T: Plugin>(event: Event) {
    with::<T, _>(|p| p.on_event(event))
}
#[doc(hidden)]
pub fn __save<T: Plugin>() -> Vec<u8> {
    with::<T, _>(|p| p.save())
}

/// Exports `$ty` (a [`Plugin`]) as the component's `lifecycle`.
#[macro_export]
macro_rules! export {
    ($ty:ty) => {
        struct __MypaperPlugin;
        impl $crate::guest::bindings::exports::mypaper::plugin::lifecycle::Guest for __MypaperPlugin {
            fn init(info: $crate::guest::InitInfo) -> Result<(), String> {
                $crate::guest::__init::<$ty>(info)
            }
            fn on_event(event: $crate::guest::Event) {
                $crate::guest::__on_event::<$ty>(event)
            }
            fn save() -> Vec<u8> {
                $crate::guest::__save::<$ty>()
            }
        }
        $crate::guest::bindings::__export_plugin!(__MypaperPlugin with_types_in $crate::guest::bindings);
    };
}

/// Faces, bar modules and panels.
pub mod ui {
    use super::bindings::mypaper::plugin::ui as wit;
    use super::*;

    /// Shows `face` in `instance`; the host drops it when nothing changed.
    pub fn set_view(instance: &str, face: Face) {
        let mut nodes = Vec::new();
        flatten(&face.body, &mut nodes);
        wit::set_view(instance, &wit::Face { nodes, footer: face.footer, refresh: face.refresh });
    }

    pub fn clear(instance: &str) {
        wit::clear(instance);
    }
}

/// `log::write` at info level.
pub fn log(message: &str) {
    bindings::mypaper::plugin::log::write(bindings::mypaper::plugin::log::Level::Info, message);
}

/// A GET to a declared origin; the reply arrives as `Event::Http` with the returned id.
pub fn get(url: &str) -> Result<u64, Error> {
    http::fetch(&http::Request { method: http::Method::Get, url: url.into(), headers: Vec::new(), body: None, timeout_ms: None })
}

/// The face's settings JSON as a value (`{}` when empty or invalid).
pub fn settings(open: &FaceOpen) -> serde_json::Value {
    serde_json::from_str(&open.settings).unwrap_or_else(|_| serde_json::json!({}))
}

use bindings::mypaper::plugin::types as t;
use bindings::mypaper::plugin::view as v;

fn tone(tone: &Tone) -> t::Tone {
    match tone {
        Tone::Label => t::Tone::Label,
        Tone::Secondary => t::Tone::Secondary,
        Tone::Tertiary => t::Tone::Tertiary,
        Tone::Accent => t::Tone::Accent,
        Tone::Warning => t::Tone::Warning,
        Tone::Destructive => t::Tone::Destructive,
        Tone::Positive => t::Tone::Positive,
        Tone::Hex(hex) => t::Tone::Hex(hex.clone()),
    }
}

fn tones(color: &Option<Tone>) -> Option<t::Tone> {
    color.as_ref().map(tone)
}

fn role(role: Role) -> t::TextRole {
    match role {
        Role::LargeTitle => t::TextRole::LargeTitle,
        Role::Title1 => t::TextRole::Title1,
        Role::Title2 => t::TextRole::Title2,
        Role::Title3 => t::TextRole::Title3,
        Role::Headline => t::TextRole::Headline,
        Role::Body => t::TextRole::Body,
        Role::Callout => t::TextRole::Callout,
        Role::Subheadline => t::TextRole::Subheadline,
        Role::Footnote => t::TextRole::Footnote,
        Role::Caption => t::TextRole::Caption,
    }
}

/// The tree as the WIT pre-order node list.
fn flatten(view: &View, out: &mut Vec<v::Node>) {
    use v::NodeKind as K;
    let leaf = |kind| v::Node { kind, children: 0 };
    let (kind, children): (K, &[View]) = match view {
        View::Stack { children, gap } => (K::Stack(*gap), children),
        View::Row { children, gap } => (K::Row(*gap), children),
        View::List { children, gap } => (K::List(*gap), children),
        View::Grid { children, columns, gap } => (K::Grid(v::Grid { columns: *columns, gap: *gap }), children),
        View::Card { title, child } => (K::Card(v::Card { title: title.clone() }), std::slice::from_ref(&**child)),
        other => {
            let kind = match other {
                View::Text { text, role: r, color, align } => K::Text(v::Text {
                    text: text.clone(),
                    role: role(*r),
                    color: tones(color),
                    align: match align {
                        Align::Start => t::Align::Start,
                        Align::Center => t::Align::Center,
                        Align::End => t::Align::End,
                    },
                }),
                View::Icon { icon, size, color } => K::Icon(v::Icon { icon: icon.clone(), size: *size, color: tones(color) }),
                View::Button { label, action, icon, primary } => K::Button(v::Button { label: label.clone(), action: action.clone(), icon: icon.clone(), primary: *primary }),
                View::IconButton { icon, label, action, enabled } => K::IconButton(v::IconButton { icon: icon.clone(), label: label.clone(), action: action.clone(), enabled: *enabled }),
                View::Select { label, action, options, selected } => K::Select(v::Select {
                    label: label.clone(),
                    action: action.clone(),
                    options: options.iter().map(|o| v::SelectOption { label: o.label.clone(), value: o.value.clone() }).collect(),
                    selected: selected.clone(),
                }),
                View::Field { placeholder, action } => K::Field(v::Field { placeholder: placeholder.clone(), action: action.clone() }),
                View::ListRow { title, detail, icon, trailing, action } => K::ListRow(v::ListRow { title: title.clone(), detail: detail.clone(), icon: icon.clone(), trailing: trailing.clone(), action: action.clone() }),
                View::Stat { label, value, detail } => K::Stat(v::Stat { label: label.clone(), value: value.clone(), detail: detail.clone() }),
                View::Metric { value, label, color, width } => K::Metric(v::Metric { value: value.clone(), label: label.clone(), color: tones(color), width: *width }),
                View::Ring { fraction, label, color } => K::Ring(v::Ring { fraction: *fraction, label: label.clone(), color: tones(color) }),
                View::LimitBar { left, pace, color, badge, label, reset } => K::LimitBar(v::LimitBar { left: *left, pace: *pace, color: tones(color), badge: badge.clone(), label: label.clone(), reset: reset.clone() }),
                View::SegmentedBar { segments } => K::SegmentedBar(segments.iter().map(|s| v::Segment { fraction: s.fraction, color: tone(&s.color) }).collect()),
                View::Sparkline { values, color } => K::Sparkline(v::Sparkline { values: values.clone(), color: tones(color) }),
                View::AreaChart { series, x_labels, unit } => K::AreaChart(v::AreaChart {
                    series: series.iter().map(|s| v::Series { label: s.label.clone(), values: s.values.clone(), color: tones(&s.color) }).collect(),
                    x_labels: x_labels.to_vec(),
                    unit: unit.clone(),
                }),
                View::Empty { icon, title, detail } => K::Empty(v::Empty { icon: icon.clone(), title: title.clone(), detail: detail.clone() }),
                _ => K::Spacer,
            };
            out.push(leaf(kind));
            return;
        }
    };
    out.push(v::Node { kind, children: children.len() as u32 });
    for child in children {
        flatten(child, out);
    }
}
