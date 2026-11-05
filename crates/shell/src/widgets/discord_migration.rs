//! One-time compatibility for Discord's old widget settings and Credential Manager targets.
use mypaper_integrations::credentials;
use mypaper_widgets::{faces, layout::Layout};
use serde_json::json;

pub(super) fn migrate(layout: &mut Layout) -> bool {
    if !faces::face_installed("discord--main") { return false; }
    let config = crate::integrations::config().discord;
    let mut changed = false;
    for instance in &mut layout.instances {
        if instance.widget != "discord" {continue;}
        for (key,value) in [
            ("clientId",json!(config.client_id)),
            ("channelIds",json!(config.channel_ids.join(","))),
            ("rpc",json!(config.rpc)),
            ("voiceControls",json!(config.voice_controls)),
        ] {
            if let serde_json::map::Entry::Vacant(entry) = instance.settings.entry(key) {
                entry.insert(value);changed=true;
            }
        }
    }
    // Never overwrite a credential the user has already set in the new plugin. Verify every
    // copy by reading it back before deleting its old target. Errors retain the original.
    for key in ["bot","client-secret","rpc-access","rpc-refresh"] {
        if let Err(error) = move_secret(key) {log::warn!("Discord credential migration ({key}): {error}");}
    }
    changed
}
fn move_secret(key: &str) -> anyhow::Result<()> {
    let old = format!("discord/{key}");
    let new = format!("plugin/discord/{key}");
    let Ok(source) = credentials::read(&old) else {return Ok(())};
    if let Ok(existing) = credentials::read(&new) {
        // A different new value wins; retain the original for recovery.
        if existing.text()? != source.text()? {return Ok(());}
    } else {
        credentials::write(&new,source.text()?)?;
    }
    anyhow::ensure!(credentials::read(&new)?.text()?==source.text()?,"broker credential verification failed");
    credentials::remove(&old)
}
