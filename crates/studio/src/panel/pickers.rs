use super::{
    Command,
    controls::{notice, row},
    editors::{field, select},
};
use crate::{
    actions::{Action, Edit, FieldId},
    app::App,
    docs::DocId,
    ui::{B, Col},
};
use mypaper_ui::controls::key;
use serde_json::{Value, json};

pub fn font(app: &App, b: &mut B, c: &mut Col, edit: Edit, current: &str) {
    let fonts = app
        .panel
        .as_ref()
        .unwrap()
        .fonts
        .get_or_init(|| b.text.families());
    let mut options = vec![(
        Value::Null,
        if edit.doc == DocId::Config {
            "From wallpaper"
        } else {
            "From Look"
        }
        .into(),
    )];
    options.extend(fonts.iter().map(|font| (json!(font), font.clone())));
    select(
        b,
        c,
        &edit.label.clone(),
        if current.is_empty() {
            Value::Null
        } else {
            json!(current)
        },
        edit,
        options,
    );
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
    let mut count = 0;
    for (i, (city, country, _)) in CITIES
        .iter()
        .enumerate()
        .filter(|(_, (city, country, _))| {
            format!("{city} {country}").to_lowercase().contains(&query)
        })
        .take(if query.is_empty() { 4 } else { 8 })
    {
        row(
            b,
            c,
            Some(crate::theme::glyph::CLOCK),
            city,
            "",
            country,
            Some(false),
            Action::Panel(Command::PickZone(target.clone(), i)),
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
