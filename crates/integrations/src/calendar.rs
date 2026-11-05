//! Read-only calendars: Windows' local AppointmentStore plus Google Calendar and Microsoft Graph.
use anyhow::{Result, ensure};
use chrono::{DateTime, Duration, Local, NaiveDate, SecondsFormat, Utc};
use serde_json::Value;
use std::{collections::BTreeMap, sync::Arc};
use tokio::sync::{Notify, mpsc};
use windows::ApplicationModel::Appointments::*;
use super::{Action, Availability, Data, Publish, calendar_http as http, calendar_oauth::{self as oauth, Service}};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Event {
    pub id: String,
    pub calendar_id: String,
    pub calendar: String,
    pub title: String,
    pub start_ms: i64,
    pub end_ms: i64,
    pub all_day: bool,
    pub color: [u8; 3],
    /// All-day dates use the provider's exclusive end date, independent of machine timezone.
    pub start_date: Option<String>,
    pub end_date: Option<String>,
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Snapshot {
    pub events: Vec<Event>,
    pub sources: BTreeMap<String, String>,
}
const COLORS: [[u8;3];6] = [[10,132,255],[48,209,88],[255,159,10],[191,90,242],[255,55,95],[255,214,10]];
pub fn color(value: &str, index: usize) -> [u8;3] {
    let Some(hex) = value.strip_prefix('#').filter(|s| s.len() == 6 && s.is_ascii()) else { return COLORS[index % COLORS.len()]; };
    match (u8::from_str_radix(&hex[..2],16),u8::from_str_radix(&hex[2..4],16),u8::from_str_radix(&hex[4..],16)) {
        (Ok(r),Ok(g),Ok(b)) => [r,g,b], _ => COLORS[index % COLORS.len()],
    }
}
fn timestamp(value: &str) -> Result<i64> {
    // Graph's response is requested in UTC; it omits the suffix on dateTimeTimeZone.dateTime.
    let value = if value.len() > 10 && !value[10..].contains(['+', '-', 'Z']) { format!("{value}Z") } else { value.to_owned() };
    Ok(DateTime::parse_from_rfc3339(&value)?.timestamp_millis())
}
fn midnight(date: &str) -> Result<i64> { Ok(NaiveDate::parse_from_str(date,"%Y-%m-%d")?.and_hms_opt(0,0,0).unwrap().and_utc().timestamp_millis()) }
fn text(value: &Value, key: &str) -> String { value[key].as_str().unwrap_or_default().chars().take(256).collect() }

/// Converts every event occurrence to the existing calendar view's date/time shape, including multi-day spans.
pub fn view_events(snapshot: &Snapshot) -> Vec<Value> {
    let mut events = Vec::new();
    for event in &snapshot.events {
        let Some(start) = DateTime::from_timestamp_millis(event.start_ms) else { continue; };
        let Some(end) = DateTime::from_timestamp_millis(event.end_ms.max(event.start_ms + 1)) else { continue; };
        let first = if event.all_day { event.start_date.as_deref().and_then(|d| NaiveDate::parse_from_str(d,"%Y-%m-%d").ok()).unwrap_or(start.date_naive()) } else { start.with_timezone(&Local).date_naive() };
        let last = if event.all_day { event.end_date.as_deref().and_then(|d| NaiveDate::parse_from_str(d,"%Y-%m-%d").ok()).map(|d| d - Duration::days(1)).unwrap_or(first) } else { (end - Duration::milliseconds(1)).with_timezone(&Local).date_naive() };
        for day in 0..=(last-first).num_days().clamp(0,365) {
            let date = first + Duration::days(day);
            let time = if event.all_day || day > 0 { String::new() } else { start.with_timezone(&Local).format("%H:%M").to_string() };
            let index = COLORS.iter().position(|c| c == &event.color).unwrap_or_else(|| {
                COLORS.iter().enumerate().min_by_key(|(_,c)| c.iter().zip(event.color).map(|(a,b)| (i32::from(*a)-i32::from(b)).pow(2)).sum::<i32>()).map_or(0,|(i,_)|i)
            });
            events.push(serde_json::json!({"id":format!("remote:{}:{}:{}",event.calendar_id,event.id,date),"title":event.title,"date":date.to_string(),"time":time,"color":index,"calendarColor":event.color,"calendar":event.calendar,"readOnly":true,"source":"calendar"}));
        }
    }
    events.sort_by(|a,b| (a["date"].as_str(),a["time"].as_str(),a["id"].as_str()).cmp(&(b["date"].as_str(),b["time"].as_str(),b["id"].as_str())));
    events
}

pub async fn windows_store() -> Result<AppointmentStore> {
    AppointmentManager::RequestStoreAsync(AppointmentStoreAccessType::AllCalendarsReadOnly)?.await.map_err(Into::into)
}
async fn windows_events(store: &AppointmentStore, start: DateTime<Utc>, end: DateTime<Utc>) -> Result<Vec<Event>> {
    let calendars = store.FindAppointmentCalendarsAsync()?.await?;
    let mut result = Vec::new();
    for calendar in calendars.into_iter().take(32) {
        let name = calendar.DisplayName()?.to_string();
        let id = calendar.LocalId()?.to_string();
        let color = calendar.DisplayColor()?;
        let options = FindAppointmentsOptions::new()?;
        options.SetMaxCount(1000)?;
        // Request the fields used below; WinRT's minimal default does not include every field.
        let properties = options.FetchProperties()?;
        for property in [AppointmentProperties::Subject()?,AppointmentProperties::StartTime()?,AppointmentProperties::Duration()?,AppointmentProperties::AllDay()?] { properties.Append(&property)?; }
        let appointments = calendar.FindAppointmentsAsyncWithOptions(windows::Foundation::DateTime { UniversalTime: (start.timestamp_millis() + 11_644_473_600_000) * 10_000 }, windows::Foundation::TimeSpan { Duration: (end-start).num_milliseconds() * 10_000 }, &options)?.await?;
        for appointment in appointments {
            let start_ms = appointment.StartTime()?.UniversalTime / 10_000 - 11_644_473_600_000;
            let end_ms = start_ms + appointment.Duration()?.Duration / 10_000;
            let all_day = appointment.AllDay()?;
            result.push(Event { id: format!("{}:{start_ms}",appointment.LocalId()?), calendar_id: format!("windows:{id}"), calendar: name.clone(), title: appointment.Subject()?.to_string(), start_ms, end_ms, all_day, color: [color.R,color.G,color.B], start_date: all_day.then(|| DateTime::from_timestamp_millis(start_ms).unwrap_or(start).with_timezone(&Local).date_naive().to_string()), end_date: all_day.then(|| DateTime::from_timestamp_millis(end_ms).unwrap_or(end).with_timezone(&Local).date_naive().to_string()) });
            if result.len() >= 1000 { return Ok(result); }
        }
    }
    Ok(result)
}

fn google(config: &super::config::Calendar, start: DateTime<Utc>, end: DateTime<Utc>) -> Result<Vec<Event>> {
    let token = oauth::access_token(Service::Google, &config.google_client_id)?;
    let mut calendars = Vec::new(); let mut page = String::new();
    for _ in 0..8 {
        let data = http::json(&format!("https://www.googleapis.com/calendar/v3/users/me/calendarList?maxResults=250&pageToken={}",http::encode(&page)),&token)?;
        if let Some(items) = data["items"].as_array() { calendars.extend(items.iter().filter(|c| c["selected"].as_bool().unwrap_or(true) && !c["deleted"].as_bool().unwrap_or(false)).cloned()); }
        page = text(&data,"nextPageToken"); if page.is_empty() { break; }
    }
    let mut events = Vec::new();
    for (index, calendar) in calendars.iter().take(32).enumerate() {
        let id = text(calendar,"id"); let mut page = String::new();
        for _ in 0..8 {
            let data = http::json(&format!("https://www.googleapis.com/calendar/v3/calendars/{}/events?singleEvents=true&orderBy=startTime&maxResults=250&timeMin={}&timeMax={}&pageToken={}", http::encode(&id),http::encode(&start.to_rfc3339_opts(SecondsFormat::Secs,true)),http::encode(&end.to_rfc3339_opts(SecondsFormat::Secs,true)),http::encode(&page)),&token)?;
            for item in data["items"].as_array().into_iter().flatten().filter(|v| v["status"] != "cancelled") {
                let all_day = item["start"]["date"].is_string();
                let start_date = item["start"]["date"].as_str(); let end_date = item["end"]["date"].as_str();
                let start_ms = if let Some(date) = start_date { midnight(date)? } else { timestamp(item["start"]["dateTime"].as_str().unwrap_or(""))? };
                let end_ms = if let Some(date) = end_date { midnight(date)? } else { timestamp(item["end"]["dateTime"].as_str().unwrap_or(""))? };
                events.push(Event { id: text(item,"id"), calendar_id:format!("google:{id}"), calendar:text(calendar,"summary"), title:text(item,"summary"), start_ms,end_ms,all_day,color:color(calendar["backgroundColor"].as_str().unwrap_or(""),index),start_date:start_date.map(str::to_owned),end_date:end_date.map(str::to_owned) });
                if events.len() >= 1000 { return Ok(events); }
            }
            page = text(&data,"nextPageToken"); if page.is_empty() { break; }
        }
    }
    Ok(events)
}
fn graph_pages(url: &str, token: &str) -> Result<Vec<Value>> {
    let mut url = url.to_owned(); let mut items = Vec::new();
    for _ in 0..16 {
        ensure!(url.starts_with("https://graph.microsoft.com/v1.0/"), "Graph pagination changed origin");
        let auth = format!("Bearer {token}");
        let data: Value = serde_json::from_slice(&http::request("GET",&url,&[("Authorization",&auth),("Prefer","outlook.timezone=\"UTC\"")],&[],2*1024*1024)?)?;
        if let Some(values) = data["value"].as_array() { items.extend(values.iter().cloned()); }
        let Some(next) = data["@odata.nextLink"].as_str() else { break; }; url = next.into();
        if items.len() >= 1000 { break; }
    }
    Ok(items)
}
fn microsoft(config: &super::config::Calendar, start: DateTime<Utc>, end: DateTime<Utc>) -> Result<Vec<Event>> {
    let token = oauth::access_token(Service::Microsoft,&config.microsoft_client_id)?;
    let calendars = graph_pages("https://graph.microsoft.com/v1.0/me/calendars?$top=100",&token)?;
    let mut events = Vec::new();
    for (index,calendar) in calendars.iter().take(32).enumerate() {
        let id = text(calendar,"id");
        let url = format!("https://graph.microsoft.com/v1.0/me/calendars/{}/calendarView?startDateTime={}&endDateTime={}&$top=250&$select=id,subject,start,end,isAllDay,isCancelled",http::encode(&id),http::encode(&start.to_rfc3339()),http::encode(&end.to_rfc3339()));
        for item in graph_pages(&url,&token)?.iter().filter(|v| !v["isCancelled"].as_bool().unwrap_or(false)) {
            let first = item["start"]["dateTime"].as_str().unwrap_or(""); let last = item["end"]["dateTime"].as_str().unwrap_or("");
            let all_day = item["isAllDay"].as_bool().unwrap_or(false);
            events.push(Event { id:text(item,"id"),calendar_id:format!("microsoft:{id}"),calendar:text(calendar,"name"),title:text(item,"subject"),start_ms:timestamp(first)?,end_ms:timestamp(last)?,all_day,color:color(calendar["hexColor"].as_str().unwrap_or(""),index),start_date:all_day.then(||first.get(..10).unwrap_or("").into()),end_date:all_day.then(||last.get(..10).unwrap_or("").into()) });
            if events.len() >= 1000 { return Ok(events); }
        }
    }
    Ok(events)
}
async fn cloud(config: &super::config::Calendar, snapshot: &mut Snapshot, start: DateTime<Utc>, end: DateTime<Utc>) {
    for (service,name,fetch) in [(Service::Google,"Google",google as fn(&super::config::Calendar,DateTime<Utc>,DateTime<Utc>)->Result<Vec<Event>>),(Service::Microsoft,"Microsoft",microsoft)] {
        if !oauth::connected(service) { snapshot.sources.insert(name.into(),"Not connected".into()); continue; }
        let config = config.clone();
        match tokio::task::spawn_blocking(move || fetch(&config,start,end)).await {
            Ok(Ok(events)) => { snapshot.sources.insert(name.into(),format!("{} events · snapshot",events.len())); snapshot.events.extend(events); }
            Ok(Err(e)) => { snapshot.sources.insert(name.into(),e.to_string()); }
            Err(_) => { snapshot.sources.insert(name.into(),"Calendar worker stopped".into()); }
        }
    }
}

/// Loads once on demand and on Windows StoreChanged. Cloud refresh is explicitly requested by the user.
pub(crate) async fn run(config: &super::config::Calendar, publish: &Publish, _commands: &mut mpsc::Receiver<Action>) -> Result<()> {
    use windows::Win32::System::Com::{CoInitializeEx, CoUninitialize, COINIT_MULTITHREADED};
    // SAFETY: this provider owns its lane; the guard balances COM even when cancelled.
    unsafe { CoInitializeEx(None, COINIT_MULTITHREADED).ok()?; }
    struct Apartment;
    impl Drop for Apartment { fn drop(&mut self) { /* SAFETY: balances this lane's COM initialization. */ unsafe { CoUninitialize(); } } }
    let _apartment = Apartment;
    let start = Utc::now()-Duration::days(180); let end = Utc::now()+Duration::days(365);
    let mut snapshot = Snapshot::default();
    let store = if config.windows {
        match windows_store().await { Ok(store) => Some(store), Err(e) => { snapshot.sources.insert("Windows".into(),format!("Unavailable: {e}")); None } }
    } else { None };
    let changed = Arc::new(Notify::new());
    let registration = store.as_ref().and_then(|store| {
        let notify = changed.clone();
        store.StoreChanged(&windows::Foundation::TypedEventHandler::new(move |_,_| { notify.notify_one(); Ok(()) })).ok()
    });
    struct Guard(Option<AppointmentStore>,Option<i64>);
    impl Drop for Guard { fn drop(&mut self) { if let (Some(store),Some(token))=(&self.0,self.1) { let _=store.RemoveStoreChanged(token); } } }
    let guard = Guard(store,registration);
    cloud(config,&mut snapshot,start,end).await;
    loop {
        if let Some(store) = &guard.0 {
            match windows_events(store,start,end).await {
                Ok(events) => { snapshot.events.retain(|e| !e.calendar_id.starts_with("windows:")); snapshot.sources.insert("Windows".into(),format!("{} events",events.len())); snapshot.events.extend(events); }
                Err(e) => { snapshot.sources.insert("Windows".into(),format!("Unavailable: {e}")); }
            }
        }
        snapshot.events.sort_by_key(|e| e.start_ms);
        publish.set(Availability::Cached,Some(Data::Calendar(snapshot.clone())));
        changed.notified().await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test] fn all_day_spans_are_exclusive_and_keep_calendar_color() {
        let snapshot=Snapshot { events:vec![Event{id:"id".into(),calendar_id:"google:one".into(),calendar:"Personal".into(),title:"Trip".into(),start_ms:midnight("2026-11-05").unwrap(),end_ms:midnight("2026-11-07").unwrap(),all_day:true,color:[4,5,6],start_date:Some("2026-11-05".into()),end_date:Some("2026-11-07".into())}],..Default::default() };
        let events=view_events(&snapshot); assert_eq!(events.len(),2); assert_eq!(events[1]["date"],"2026-11-06"); assert_eq!(events[0]["calendarColor"],serde_json::json!([4,5,6])); assert_eq!(events[0]["readOnly"],true);
    }
    #[test] fn parses_graph_utc_and_rejects_broken_dates() {
        assert_eq!(timestamp("2026-11-05T12:00:00.0000000").unwrap(),timestamp("2026-11-05T15:00:00+03:00").unwrap());
        assert!(timestamp("bad").is_err()); assert_eq!(color("#12abEF",0),[18,171,239]);
    }
}
