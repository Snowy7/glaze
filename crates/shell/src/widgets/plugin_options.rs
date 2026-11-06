//! A plugin widget's options (docs/plugins.md, "Face options"): right-click the widget, choose
//! "Options…", and a kit menu built from the face's option schema (its widget `settings`) edits
//! the instance's settings in place. A change is saved to `widgets.json` and reaches the plugin
//! live as `event::options`. Studio's Widgets page edits the same values.
use mypaper_ui::menu::{Entry, Menu, screen_at};
use mypaper_widgets::manifest::Setting;
use serde_json::{Map, Value};

use super::Widgets;

/// What a chosen option row sets: (instance, option key, new value).
pub(super) type Choice = (String, String, Value);

/// Number presets for a `number` option: every step when there are a few, else a spread that
/// keeps the default and the current value.
fn presets(min: Option<f64>, max: Option<f64>, step: Option<f64>, default: f64, current: f64) -> Vec<f64> {
    let step = step.filter(|s| *s > 0.0).unwrap_or(1.0);
    let lo = min.unwrap_or(0.0);
    let hi = max.unwrap_or((default * 4.0).max(lo + 10.0 * step));
    let snap = |v: f64| (lo + ((v - lo) / step).round() * step).clamp(lo, hi);
    let mut out: Vec<f64> = if (hi - lo) / step <= 11.0 {
        (0..=((hi - lo) / step).round() as usize).map(|i| lo + i as f64 * step).collect()
    } else {
        (0..=7).map(|i| snap(lo + (hi - lo) * i as f64 / 7.0)).collect()
    };
    out.extend([snap(default), snap(current)]);
    out.sort_by(f64::total_cmp);
    out.dedup();
    out
}

fn number_label(v: f64) -> String {
    if v.fract() == 0.0 { format!("{v:.0}") } else { format!("{v}") }
}

/// The menu rows for `schema` with `values` (the instance's settings, defaults filled in).
/// Text-like options have no row here; they're edited in Studio.
pub(super) fn entries(instance: &str, schema: &[Setting], values: &Map<String, Value>) -> Vec<Entry<Choice>> {
    let pick = |key: &str, value: Value| (instance.to_owned(), key.to_owned(), value);
    schema
        .iter()
        .filter_map(|setting| {
            let key = setting.key();
            let current = values.get(key).cloned().unwrap_or_else(|| setting.default_value());
            Some(match setting {
                Setting::Boolean { label, .. } => {
                    let on = current.as_bool().unwrap_or(false);
                    Entry::check(label, pick(key, (!on).into()), on)
                }
                Setting::Select { label, options, .. } => Entry::Submenu {
                    label: label.clone(),
                    items: options
                        .iter()
                        .map(|option| {
                            let value = option.value();
                            let shown = match option {
                                mypaper_widgets::manifest::SelectOption::Detailed(c) => c.label.as_deref().unwrap_or(value),
                                mypaper_widgets::manifest::SelectOption::Plain(v) => v,
                            };
                            Entry::radio(shown, pick(key, value.into()), current.as_str() == Some(value))
                        })
                        .collect(),
                },
                Setting::Number { label, min, max, step, default, .. } => {
                    let now = current.as_f64().unwrap_or(*default);
                    Entry::Submenu {
                        label: format!("{label}: {}", number_label(now)),
                        items: presets(*min, *max, *step, *default, now)
                            .into_iter()
                            .map(|v| {
                                let value = if v.fract() == 0.0 { Value::from(v as i64) } else { Value::from(v) };
                                Entry::radio(&number_label(v), pick(key, value), v == now)
                            })
                            .collect(),
                    }
                }
                _ => return None,
            })
        })
        .collect()
}

impl Widgets {
    /// Right-click on plugin widget `id`: "Options…" over its option rows, when it has any.
    pub(super) fn open_plugin_options(&mut self, ui: &mut mypaper_ui::Ui, id: &str, at: (i32, i32)) {
        let Some(entry) = self.entries.get(id) else { return };
        let Some(installed) = self.registry.get(&entry.model.placed.widget) else { return };
        let manifest = &installed.manifest;
        let values = manifest.resolve_settings(&entry.model.placed.settings);
        let rows = entries(id, &manifest.settings, &values);
        if rows.is_empty() {
            return;
        }
        let items = vec![Entry::Submenu { label: "Options…".into(), items: rows }];
        if self.options_menu.is_none() {
            self.options_menu = Menu::new("Widget options").ok();
        }
        if let Some(menu) = &mut self.options_menu
            && let Err(error) = menu.open(ui, items, at, screen_at(at.0, at.1), &entry.tokens)
        {
            log::warn!("Widget options: {error:#}");
        }
    }

    /// Sets one option of plugin widget `id`: saved to the layout, then sent to the plugin as
    /// `event::options` by the next face sync.
    pub(super) fn set_plugin_option(&mut self, (id, key, value): Choice) {
        let Some(entry) = self.entries.get(&id) else { return };
        let Some(installed) = self.registry.get(&entry.model.placed.widget) else { return };
        let Ok(checked) = installed.manifest.check_settings(Map::from_iter([(key, value)])) else { return };
        let mut next = self.layout.clone();
        if next.set_settings(&id, checked.clone()).is_err() || !self.commit_layout(next) {
            return;
        }
        if let Some(entry) = self.entries.get_mut(&id) {
            entry.model.placed.settings.extend(checked);
            entry.model.dirty = true;
        }
        self.sync_plugin_faces();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn schema() -> Vec<Setting> {
        serde_json::from_value(json!([
            { "type": "boolean", "key": "show-subagents", "label": "Show subagents", "default": true },
            { "type": "number", "key": "max-items", "label": "Max items", "min": 3, "max": 50, "default": 20 },
            { "type": "select", "key": "sort", "label": "Sort", "options": ["recent", "name"], "default": "recent" },
            { "type": "string", "key": "note", "label": "Note", "default": "" },
        ]))
        .unwrap()
    }

    #[test]
    fn option_rows_follow_the_schema_and_flip_or_pick_values() {
        let values = Map::from_iter([("show-subagents".to_owned(), json!(true)), ("max-items".to_owned(), json!(20))]);
        let rows = entries("t3--running-1", &schema(), &values);
        assert_eq!(rows.len(), 3, "the text option is edited in Studio, not here");
        let Entry::Action { command, checked, .. } = &rows[0] else { panic!("a check row") };
        assert_eq!(*checked, Some(true));
        assert_eq!(command, &("t3--running-1".to_owned(), "show-subagents".to_owned(), json!(false)), "choosing it turns it off");
        let Entry::Submenu { label, items } = &rows[1] else { panic!("number presets") };
        assert_eq!(label, "Max items: 20");
        let values: Vec<&str> = items.iter().map(Entry::label).collect();
        assert!(values.contains(&"3") && values.contains(&"20") && values.contains(&"50"), "{values:?}");
        assert!(values.len() <= 10, "{values:?}");
        let Entry::Submenu { items, .. } = &rows[2] else { panic!("a choice") };
        assert_eq!(items.len(), 2);
    }

    #[test]
    fn small_number_ranges_offer_every_step() {
        assert_eq!(presets(Some(1.0), Some(5.0), None, 3.0, 3.0), [1.0, 2.0, 3.0, 4.0, 5.0]);
    }
}
