//! Read-only live qualification. Never requests consent, sends a message, or changes vendor state.
#![windows_subsystem = "windows"]
use glaze_integrations::{Availability, Data, Hub, Provider, config::Config};
use serde_json::json;
use std::time::Duration;
fn main() -> anyhow::Result<()> {
    let output = std::env::args()
        .nth(1)
        .ok_or_else(|| anyhow::anyhow!("expected an evidence JSON path"))?;
    let config_path = std::path::PathBuf::from(std::env::var_os("APPDATA").unwrap_or_default())
        .join("glaze/integrations.json");
    let mut config = std::fs::read_to_string(config_path)
        .ok()
        .and_then(|s| Config::parse(&s).ok())
        .unwrap_or_default();
    config.calendar.enabled = true;
    let (changed, events) = std::sync::mpsc::channel();
    let hub = Hub::new(move |provider| {
        let _ = changed.send(provider);
    });
    let calendar = hub.acquire(Provider::Calendar, &config)?;
    let notifications = if config.notifications.enabled {
        hub.acquire(Provider::Notifications, &config).ok()
    } else {
        None
    };
    let deadline = std::time::Instant::now() + Duration::from_secs(12);
    while std::time::Instant::now() < deadline {
        if calendar.snapshot().data.is_some()
            && notifications
                .as_ref()
                .is_none_or(|l| l.snapshot().availability != Availability::Connecting)
        {
            break;
        }
        let _ = events.recv_timeout(Duration::from_millis(100));
    }
    let calendar = calendar.snapshot();
    let report = json!({
        "at":chrono::Utc::now().to_rfc3339(),"packaged":windows::ApplicationModel::Package::Current().is_ok(),
        "calendar":{"availability":format!("{:?}",calendar.availability),"events":match &calendar.data{Some(Data::Calendar(s))=>s.events.len(),_=>0},"sources":match &calendar.data{Some(Data::Calendar(s))=>json!(s.sources),_=>json!({})}},
        "discordNotifications":notifications.as_ref().and_then(|l|match l.snapshot().data{Some(Data::Notifications(s))=>Some(s.cards.iter().filter(|card|glaze_integrations::notifications::is_discord_app_id(&card.app_id)).count()),_=>None})
    });
    if let Some(parent) = std::path::Path::new(&output).parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(output, serde_json::to_vec_pretty(&report)?)?;
    hub.stop_all();
    Ok(())
}
