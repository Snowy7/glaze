use super::{
    Command,
    controls::{notice, row},
    editors::{action_button, field},
};
use crate::{
    actions::{Action, Edit, FieldId},
    app::App,
    docs::DocId,
    ui::{B, Col},
};
use mypaper_ui::controls::key;
use serde_json::{Value, json};

/// Pick an installed app for a launcher entry; names and resolved targets are one edit.
pub fn launcher(
    app: &App,
    b: &mut B,
    c: &mut Col,
    path: &str,
    items: &[Value],
    index: Option<usize>,
) {
    let query_id = FieldId::Text(
        DocId::Studio,
        format!("/panel/draft/apps/{:x}", key((path, index))).into(),
    );
    let query = app.text(&query_id).trim().to_lowercase();
    field(
        app,
        b,
        c,
        query_id,
        if index.is_some() {
            "Choose an app"
        } else {
            "Add an app"
        },
        "Search installed apps",
        "",
    );
    let apps = super::icons::apps();
    let mut count = 0;
    for (name, target) in apps
        .iter()
        .filter(|(name, _)| name.to_lowercase().contains(&query))
        .take(if query.is_empty() { 4 } else { 6 })
    {
        let mut next = items.to_vec();
        let value = json!({"label":name,"target":target});
        if let Some(index) = index {
            if let Some(item) = next.get_mut(index) {
                *item = value;
            }
        } else {
            next.push(value);
        }
        row(
            b,
            c,
            Some(crate::theme::glyph::APP),
            name,
            "",
            "",
            Some(
                index
                    .and_then(|i| items.get(i))
                    .is_some_and(|item| item["target"] == *target),
            ),
            Action::Set(Edit::new(DocId::Widgets, path, "Launcher app"), json!(next)),
        );
        count += 1;
    }
    if count == 0 {
        notice(
            b,
            c,
            "No installed apps match. Use Advanced target for a path or link.",
        );
    }
    if index.is_none() {
        let advanced = key(("launcher-manual", path));
        action_button(b, c, "Advanced target", Action::More(advanced));
        if app.is_open(advanced) {
            let label_id = FieldId::Text(
                DocId::Studio,
                format!("/panel/draft/launcher-name{path}").into(),
            );
            let target_id = FieldId::Text(
                DocId::Studio,
                format!("/panel/draft/launcher-target{path}").into(),
            );
            field(app, b, c, label_id.clone(), "Name", "App or link name", "");
            field(
                app,
                b,
                c,
                target_id.clone(),
                "Target",
                "Path, app ID or link",
                "",
            );
            let label = app.text(&label_id).trim();
            let target = app.text(&target_id).trim();
            let mut next = items.to_vec();
            next.push(json!({"label":if label.is_empty() {target} else {label},"target":target}));
            super::controls::button(
                b,
                mypaper_ui::Rect::new(c.x, c.y, c.w, b.px(30.0)),
                "Add target",
                None,
                false,
                !target.is_empty(),
                Action::Set(Edit::new(DocId::Widgets, path, "Launcher app"), json!(next)),
            );
            c.y += b.px(38.0);
        }
    }
}

fn inherited_font(edit: &Edit) -> &'static str {
    if edit.doc == DocId::Config {
        "From wallpaper"
    } else if edit.path.starts_with("/theme:") {
        "The style's"
    } else {
        "From Look"
    }
}

pub fn font(_app: &App, b: &mut B, c: &mut Col, edit: Edit, current: &str) {
    row(
        b,
        c,
        None,
        &edit.label.clone(),
        if current.is_empty() {
            inherited_font(&edit)
        } else {
            current
        },
        "",
        None,
        Action::Panel(Command::PickFont(edit, current.to_owned())),
    );
}

/// Installed families are needed only when opening the chooser, never to display its row.
pub fn open_font(app: &mut App, edit: Edit, current: String) {
    let fonts = app.panel.as_ref().unwrap().fonts.get_or_init(|| {
        mypaper_ui::Ui::isolated()
            .map(|ui| ui.text.families())
            .unwrap_or_default()
    });
    let mut options = vec![(Value::Null, inherited_font(&edit).into())];
    options.extend(fonts.iter().map(|font| (json!(font), font.clone())));
    app.run(Action::Choose(crate::actions::Choices {
        edit,
        options,
        current: Some(if current.is_empty() {
            Value::Null
        } else {
            json!(current)
        }),
    }));
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ZoneTarget {
    Calendar,
    Record(String),
    AddRecord(String),
}

// City names lead; canonical IANA identifiers are implementation details stored on selection.
const CITIES: &[(&str, &str, &str)] = &[
    ("London", "United Kingdom", "Europe/London"),
    ("New York", "United States", "America/New_York"),
    ("Tokyo", "Japan", "Asia/Tokyo"),
    ("Sydney", "Australia", "Australia/Sydney"),
    ("Doha", "Qatar", "Asia/Qatar"),
    ("Dubai", "United Arab Emirates", "Asia/Dubai"),
    ("Abu Dhabi", "United Arab Emirates", "Asia/Dubai"),
    ("Riyadh", "Saudi Arabia", "Asia/Riyadh"),
    ("Kuwait City", "Kuwait", "Asia/Kuwait"),
    ("Manama", "Bahrain", "Asia/Bahrain"),
    ("Muscat", "Oman", "Asia/Muscat"),
    ("Jerusalem", "Israel", "Asia/Jerusalem"),
    ("Amman", "Jordan", "Asia/Amman"),
    ("Beirut", "Lebanon", "Asia/Beirut"),
    ("Baghdad", "Iraq", "Asia/Baghdad"),
    ("Tehran", "Iran", "Asia/Tehran"),
    ("Istanbul", "Turkey", "Europe/Istanbul"),
    ("Paris", "France", "Europe/Paris"),
    ("Berlin", "Germany", "Europe/Berlin"),
    ("Madrid", "Spain", "Europe/Madrid"),
    ("Rome", "Italy", "Europe/Rome"),
    ("Amsterdam", "Netherlands", "Europe/Amsterdam"),
    ("Brussels", "Belgium", "Europe/Brussels"),
    ("Vienna", "Austria", "Europe/Vienna"),
    ("Zurich", "Switzerland", "Europe/Zurich"),
    ("Stockholm", "Sweden", "Europe/Stockholm"),
    ("Oslo", "Norway", "Europe/Oslo"),
    ("Copenhagen", "Denmark", "Europe/Copenhagen"),
    ("Helsinki", "Finland", "Europe/Helsinki"),
    ("Warsaw", "Poland", "Europe/Warsaw"),
    ("Prague", "Czechia", "Europe/Prague"),
    ("Budapest", "Hungary", "Europe/Budapest"),
    ("Athens", "Greece", "Europe/Athens"),
    ("Bucharest", "Romania", "Europe/Bucharest"),
    ("Kyiv", "Ukraine", "Europe/Kyiv"),
    ("Moscow", "Russia", "Europe/Moscow"),
    ("Dublin", "Ireland", "Europe/Dublin"),
    ("Lisbon", "Portugal", "Europe/Lisbon"),
    ("Los Angeles", "United States", "America/Los_Angeles"),
    ("San Francisco", "United States", "America/Los_Angeles"),
    ("Chicago", "United States", "America/Chicago"),
    ("Denver", "United States", "America/Denver"),
    ("Phoenix", "United States", "America/Phoenix"),
    ("Boston", "United States", "America/New_York"),
    ("Miami", "United States", "America/New_York"),
    ("Seattle", "United States", "America/Los_Angeles"),
    ("Honolulu", "United States", "Pacific/Honolulu"),
    ("Anchorage", "United States", "America/Anchorage"),
    ("Toronto", "Canada", "America/Toronto"),
    ("Montreal", "Canada", "America/Toronto"),
    ("Vancouver", "Canada", "America/Vancouver"),
    ("Mexico City", "Mexico", "America/Mexico_City"),
    ("Sao Paulo", "Brazil", "America/Sao_Paulo"),
    (
        "Buenos Aires",
        "Argentina",
        "America/Argentina/Buenos_Aires",
    ),
    ("Santiago", "Chile", "America/Santiago"),
    ("Bogota", "Colombia", "America/Bogota"),
    ("Lima", "Peru", "America/Lima"),
    ("Cairo", "Egypt", "Africa/Cairo"),
    ("Casablanca", "Morocco", "Africa/Casablanca"),
    ("Lagos", "Nigeria", "Africa/Lagos"),
    ("Nairobi", "Kenya", "Africa/Nairobi"),
    ("Johannesburg", "South Africa", "Africa/Johannesburg"),
    ("Cape Town", "South Africa", "Africa/Johannesburg"),
    ("Addis Ababa", "Ethiopia", "Africa/Addis_Ababa"),
    ("Accra", "Ghana", "Africa/Accra"),
    ("Mumbai", "India", "Asia/Kolkata"),
    ("Delhi", "India", "Asia/Kolkata"),
    ("Bengaluru", "India", "Asia/Kolkata"),
    ("Karachi", "Pakistan", "Asia/Karachi"),
    ("Dhaka", "Bangladesh", "Asia/Dhaka"),
    ("Kathmandu", "Nepal", "Asia/Kathmandu"),
    ("Colombo", "Sri Lanka", "Asia/Colombo"),
    ("Singapore", "Singapore", "Asia/Singapore"),
    ("Hong Kong", "China", "Asia/Hong_Kong"),
    ("Beijing", "China", "Asia/Shanghai"),
    ("Shanghai", "China", "Asia/Shanghai"),
    ("Taipei", "Taiwan", "Asia/Taipei"),
    ("Seoul", "South Korea", "Asia/Seoul"),
    ("Bangkok", "Thailand", "Asia/Bangkok"),
    ("Jakarta", "Indonesia", "Asia/Jakarta"),
    ("Manila", "Philippines", "Asia/Manila"),
    ("Kuala Lumpur", "Malaysia", "Asia/Kuala_Lumpur"),
    ("Ho Chi Minh City", "Vietnam", "Asia/Ho_Chi_Minh"),
    ("Melbourne", "Australia", "Australia/Melbourne"),
    ("Brisbane", "Australia", "Australia/Brisbane"),
    ("Perth", "Australia", "Australia/Perth"),
    ("Adelaide", "Australia", "Australia/Adelaide"),
    ("Auckland", "New Zealand", "Pacific/Auckland"),
    ("Wellington", "New Zealand", "Pacific/Auckland"),
    ("Reykjavik", "Iceland", "Atlantic/Reykjavik"),
    ("UTC", "Coordinated Universal Time", "Etc/UTC"),
];

fn query_field(target: &ZoneTarget) -> FieldId {
    FieldId::Text(
        DocId::Studio,
        format!("/panel/draft/cities/{:x}", key(target)).into(),
    )
}

pub fn city_name(zone: &str) -> String {
    CITIES.iter().find(|(_, _, id)| *id == zone).map_or_else(
        || zone.rsplit('/').next().unwrap_or(zone).replace('_', " "),
        |(city, _, _)| (*city).into(),
    )
}

pub fn zones(app: &App, b: &mut B, c: &mut Col, target: ZoneTarget, caption: &str) {
    let id = query_field(&target);
    let query = app.text(&id).trim().to_lowercase();
    field(app, b, c, id, caption, "Search a city or country", "");
    if query.is_empty() && !matches!(target, ZoneTarget::Calendar) {
        return;
    }
    let existing: Vec<String> = match &target {
        ZoneTarget::Calendar => app
            .get(DocId::Bars, "/calendar/zones")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default()
            .iter()
            .filter_map(|v| v["zone"].as_str().map(str::to_owned))
            .collect(),
        ZoneTarget::AddRecord(path) => {
            let layout = crate::pages::widgets::layout(app);
            path.split('/')
                .nth(2)
                .and_then(|s| s.parse::<usize>().ok())
                .and_then(|i| layout.instances.get(i))
                .and_then(|instance| {
                    app.registry
                        .get(&instance.widget)
                        .map(|w| w.manifest.resolve_settings(&instance.settings))
                })
                .and_then(|values| values.get("zones").cloned())
                .and_then(|v| v.as_array().cloned())
                .unwrap_or_default()
                .iter()
                .filter_map(|v| v["tz"].as_str().map(str::to_owned))
                .collect()
        }
        _ => vec![],
    };
    let mut count = 0;
    for (i, (city, country, zone)) in CITIES
        .iter()
        .enumerate()
        .filter(|(_, (city, country, zone))| {
            format!("{city} {country}").to_lowercase().contains(&query)
                && (matches!(target, ZoneTarget::Calendar)
                    || !existing.iter().any(|old| old == zone))
        })
        .take(if query.is_empty() { 4 } else { 8 })
    {
        row(
            b,
            c,
            Some(crate::theme::glyph::CLOCK),
            city,
            "",
            if existing.iter().any(|old| old == zone) {
                "Added"
            } else if matches!(target, ZoneTarget::Calendar) {
                "Suggested"
            } else {
                country
            },
            Some(existing.iter().any(|old| old == zone)),
            if existing.iter().any(|old| old == zone) {
                Action::None
            } else {
                Action::Panel(Command::PickZone(target.clone(), i))
            },
        );
        count += 1;
    }
    if count == 0 {
        notice(
            b,
            c,
            "No matching city. Try another city in the same time zone.",
        );
    }
}

pub fn choose_zone(app: &mut App, target: &ZoneTarget, city: usize) {
    let Some((city, _, zone)) = CITIES.get(city) else {
        return;
    };
    match target {
        ZoneTarget::Calendar => {
            let mut zones = app
                .get(DocId::Bars, "/calendar/zones")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default();
            if zones
                .iter()
                .any(|value| value["zone"].as_str() == Some(zone))
            {
                return;
            }
            if zones.len() >= 4 {
                app.alert("The calendar can show four cities");
                return;
            }
            zones.push(json!({"zone": zone, "label": city}));
            app.set(
                &Edit::new(DocId::Bars, "/calendar/zones", "Add a city"),
                json!(zones),
            );
        }
        ZoneTarget::Record(path) => app.set(
            &Edit::new(DocId::Widgets, path.clone(), "City"),
            json!({"label": city, "tz": zone}),
        ),
        ZoneTarget::AddRecord(path) => {
            let layout = crate::pages::widgets::layout(app);
            let Some(instance) = path
                .split('/')
                .nth(2)
                .and_then(|s| s.parse::<usize>().ok())
                .and_then(|i| layout.instances.get(i))
            else {
                return;
            };
            let Some(manifest) = app.registry.get(&instance.widget).map(|w| &w.manifest) else {
                return;
            };
            let mut cities = manifest
                .resolve_settings(&instance.settings)
                .get("zones")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default();
            if cities
                .iter()
                .any(|value| value["tz"].as_str() == Some(zone))
            {
                return;
            }
            if cities.len() >= 8 {
                app.alert("The clock can show eight cities");
                return;
            }
            cities.push(json!({"label":city,"tz":zone}));
            app.set(
                &Edit::new(DocId::Widgets, path.clone(), "Add a city"),
                json!(cities),
            );
        }
    }
    app.texts.remove(&query_field(target));
}

#[cfg(test)]
mod font_tests {
    use super::*;

    #[test]
    fn font_rows_load_installed_families_only_when_opened_without_saving() {
        let mut ui = mypaper_ui::Ui::new().unwrap();
        let mut app = crate::goldens::app(true, &ui);
        app.panel = Some(crate::panel::State {
            page: crate::panel::Page::Look,
            ..Default::default()
        });
        app.nav.more.insert(key((
            "panel-more",
            key(("studio-panel", crate::panel::Page::Look)),
        )));
        let version = app.docs.version;
        let built = crate::panel::build(&app, &mut ui.text);
        assert!(app.panel.as_ref().unwrap().fonts.get().is_none());
        let target = built
            .targets
            .iter()
            .find(|target| matches!(target.action, Action::Panel(Command::PickFont(..))))
            .unwrap();
        app.input.focus = Some(target.key);
        let action = target.action.clone();
        app.frame.targets = built.targets;
        app.run(action);
        assert_eq!(app.docs.version, version);
        let (_, choices) = app.picking.as_ref().expect("the font chooser opened");
        assert!(choices.options.iter().any(|(_, name)| name == "Arial"));
        assert!(app.panel.as_ref().unwrap().fonts.get().is_some());
    }
}

/// Icon packs use the same unboxed, adaptive select as every panel setting.
pub fn icon_pack(app: &App, b: &mut B, c: &mut Col) {
    use mypaper_bars::icon_pack::IconPack;
    use mypaper_ui::{Rect, controls::Align};
    let current = app
        .get(DocId::Config, "/look/text/icon_pack")
        .cloned()
        .unwrap_or(Value::Null);
    let folder = mypaper_config::dir().ok().map(|dir| dir.join("icons"));
    let mut options = vec![(Value::Null, "The style's icons".to_owned())];
    if let Some(folder) = &folder {
        let mut names: Vec<_> = std::fs::read_dir(folder)
            .into_iter()
            .flatten()
            .flatten()
            .filter(|entry| entry.path().is_dir())
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        options.extend(names.into_iter().map(|name| (json!(name), name)));
    }
    crate::ui::anchor(app, "icon_pack", c.view, c.y);
    super::editors::select(
        b,
        c,
        "Icon pack",
        current.clone(),
        Edit::new(DocId::Config, "/look/text/icon_pack", "Icon pack"),
        options,
    );
    let pack = current
        .as_str()
        .zip(folder.as_ref())
        .and_then(|(name, folder)| {
            let dir = folder.join(name);
            IconPack::load(&dir).ok().map(|pack| (dir, pack))
        });
    if let Some((dir, pack)) = pack {
        for (i, entry) in pack.entries.iter().enumerate() {
            let rect = Rect::new(c.x, c.y, c.w, super::controls::height(b, 36.0));
            let icon = Rect::new(
                rect.left + b.px(2.0),
                rect.top + b.px(8.0),
                b.px(20.0),
                b.px(20.0),
            );
            if let Some(image) = app
                .images
                .borrow_mut()
                .get(&dir.join(&entry.icon), (64, 64))
            {
                crate::stage::image_cover(b, icon, &image, 1.0);
            } else {
                b.icon(icon, crate::theme::glyph::APP, 14.0, b.p.ink3);
            }
            let what = [&entry.path, &entry.umid, &entry.extension, &entry.key]
                .into_iter()
                .flatten()
                .next()
                .map_or("", String::as_str);
            super::controls::text(
                b,
                Rect::new(
                    rect.left + b.px(32.0),
                    rect.top,
                    rect.width() - b.px(68.0),
                    rect.height(),
                ),
                what,
                13.0,
                400,
                b.p.ink1,
                Align::Start,
            );
            super::controls::icon_button(
                b,
                Rect::new(
                    rect.right - b.px(30.0),
                    rect.top + b.px(4.0),
                    b.px(28.0),
                    b.px(28.0),
                ),
                crate::theme::glyph::DELETE,
                &format!("Remove {what}"),
                true,
                Action::IconPackRemove(i),
            );
            c.y = rect.bottom;
        }
        action_button(b, c, "An app's icon", Action::IconPackAdd);
    } else {
        action_button(b, c, "Start your own", Action::IconPackNew);
    }
}
