//! Retained Windows toasts, grouped by AUMID. These are never called unread messages.
use crate::{Availability, Data, Publish};
use anyhow::{Result, ensure};
use std::{
    collections::BTreeMap,
    sync::{Arc, Once},
};
use tokio::sync::{Notify, mpsc};
use windows::{
    Foundation::TypedEventHandler,
    UI::Notifications::Management::{
        UserNotificationListener, UserNotificationListenerAccessStatus,
    },
    UI::Notifications::{KnownNotificationBindings, NotificationKinds},
};

/// Identity queries must not retain a WinRT factory from an apartment that can be torn down
/// (Studio creates previews on successive test threads). This Win32 query needs no apartment.
/// Stable, PTB, Canary and Store identities used by the Windows notification preview filter.
pub fn is_discord_app_id(app_id: &str) -> bool {
    let id = app_id.to_ascii_lowercase();
    matches!(id.as_str(), "com.squirrel.discord.discord" | "com.squirrel.discordptb.discordptb" | "com.squirrel.discordcanary.discordcanary")
        || (id.starts_with("discordinc.") && id.contains("!discord"))
}
fn package_family() -> Option<String> {
    use windows::Win32::{
        Foundation::ERROR_INSUFFICIENT_BUFFER,
        Storage::Packaging::Appx::GetCurrentPackageFamilyName,
    };
    let mut len = 0;
    // SAFETY: the first call asks for the size; the second owns a buffer of that many UTF-16 units.
    unsafe {
        if GetCurrentPackageFamilyName(&mut len, None) != ERROR_INSUFFICIENT_BUFFER || len == 0 {
            return None;
        }
        let mut text = vec![0; len as usize];
        GetCurrentPackageFamilyName(&mut len, Some(windows::core::PWSTR(text.as_mut_ptr())))
            .ok()
            .ok()?;
        String::from_utf16(&text[..len.saturating_sub(1) as usize]).ok()
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Card {
    pub id: u32,
    pub app_id: String,
    pub app: String,
    pub title: String,
    pub body: String,
    pub at: i64,
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Snapshot {
    pub cards: Vec<Card>,
    pub counts: BTreeMap<String, usize>,
}
/// Bound UTF-8 storage, including emoji, without splitting a scalar value.
fn text(value: &str, bytes: usize) -> String {
    let mut end = value.len().min(bytes);
    while !value.is_char_boundary(end) {
        end -= 1;
    }
    value[..end].to_owned()
}

/// Call on the UI thread after Enable/Grant access, or during the enabled integration's first run.
pub fn request_access()
-> windows::core::Result<windows_future::IAsyncOperation<UserNotificationListenerAccessStatus>> {
    UserNotificationListener::Current()?.RequestAccessAsync()
}

/// Request first-run consent on the UI thread; completion invalidates consumers without blocking it.
/// Windows persists Allowed/Denied. A per-package marker also remembers a dismissed prompt.
pub fn request_access_once(completed: impl Fn() + Send + Sync + 'static) -> Result<()> {
    let Some(family) = package_family() else {
        log_snapshot_mode("package identity is absent");
        return Ok(());
    };
    let listener = UserNotificationListener::Current()?;
    if listener.GetAccessStatus()? != UserNotificationListenerAccessStatus::Unspecified {
        return Ok(());
    }
    let directory = std::path::PathBuf::from(
        std::env::var_os("LOCALAPPDATA")
            .ok_or_else(|| anyhow::anyhow!("LOCALAPPDATA is missing"))?,
    )
    .join("mypaper/notification-consent");
    std::fs::create_dir_all(&directory)?;
    let marker = directory.join(family);
    match std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&marker)
    {
        Ok(_) => {}
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => return Ok(()),
        Err(e) => return Err(e.into()),
    }
    let operation = match request_access() {
        Ok(operation) => operation,
        Err(e) => {
            let _ = std::fs::remove_file(&marker);
            return Err(e.into());
        }
    };
    // The finite consent waiter owns the operation until completion. The shell keeps pumping UI.
    std::thread::Builder::new().name("notification-consent".into()).spawn(move || {
        match operation.join() {
            Ok(UserNotificationListenerAccessStatus::Allowed) => log::info!("notification access allowed"),
            Ok(UserNotificationListenerAccessStatus::Denied) => log::info!("notification access denied; enable it in Windows Settings > Privacy & security > Notifications"),
            Ok(_) => log::info!("notification access not granted; snapshots remain unavailable until access is granted"),
            Err(e) => {
                let _ = std::fs::remove_file(marker);
                log::warn!("notification consent: {e}");
            }
        }
        completed();
    })?;
    Ok(())
}

fn log_snapshot_mode(reason: &str) {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| log::info!("notifications: snapshot mode ({reason}); no polling"));
}
pub fn read(listener: &UserNotificationListener, show_text: bool) -> Result<Snapshot> {
    read_scoped(listener,show_text,false)
}
fn read_scoped(listener:&UserNotificationListener,show_text:bool,discord_previews:bool)->Result<Snapshot>{
    ensure!(
        listener.GetAccessStatus()? == UserNotificationListenerAccessStatus::Allowed,
        "notification access is off; allow mypaper in Windows Settings > Privacy & security > Notifications"
    );
    let list = listener
        .GetNotificationsAsync(NotificationKinds::Toast)?
        .join()?;
    ensure!(
        list.Size()? <= 512,
        "notification history exceeds the 512-card bound"
    );
    let mut state = Snapshot::default();
    for notification in list {
        let info = notification.AppInfo()?;
        let app_id = info.AppUserModelId()?.to_string();
        *state.counts.entry(app_id.clone()).or_default() += 1;
        let mut texts = Vec::new();
        if (show_text || discord_previews && is_discord_app_id(&app_id))
            && let Ok(binding) = notification
                .Notification()?
                .Visual()?
                .GetBinding(&KnownNotificationBindings::ToastGeneric()?)
        {
            for element in binding.GetTextElements()?.into_iter().take(3) {
                texts.push(text(&element.Text()?.to_string(), 256));
            }
        }
        state.cards.push(Card {
            id: notification.Id()?,
            app_id,
            app: text(&info.DisplayInfo()?.DisplayName()?.to_string(), 128),
            title: texts
                .first()
                .cloned()
                .unwrap_or_else(|| "Notification".into()),
            body: texts.get(1..).unwrap_or_default().join("\n"),
            at: notification.CreationTime()?.UniversalTime,
        });
    }
    state.cards.sort_by_key(|c| (std::cmp::Reverse(c.at), c.id));
    Ok(state)
}

struct Subscription {
    listener: UserNotificationListener,
    token: Option<i64>,
}
impl Drop for Subscription {
    fn drop(&mut self) {
        if let Some(token) = self.token {
            let _ = self.listener.RemoveNotificationChanged(token);
        }
    }
}

pub(crate) async fn run(
    config: &crate::config::Notifications,
    publish: &Publish,
    commands: &mut mpsc::Receiver<crate::Action>,
) -> Result<()> {
    let _com = crate::media::Com::new()?;
    let packaged = package_family().is_some();
    if !packaged {
        log_snapshot_mode("package identity is absent");
    }
    let listener = UserNotificationListener::Current()?;
    ensure!(
        listener.GetAccessStatus()? == UserNotificationListenerAccessStatus::Allowed,
        "notification access is off; allow mypaper in Windows Settings > Privacy & security > Notifications"
    );
    let changed = Arc::new(Notify::new());
    let notify = changed.clone();
    let events = publish.clone();
    let token = if !packaged {
        None
    } else if config.live_verified {
        match listener.NotificationChanged(&TypedEventHandler::new(move |_, _| {
            events.delivered();
            notify.notify_one();
            Ok(())
        })) {
            Ok(token) => Some(token),
            Err(e) => {
                log_snapshot_mode(&format!("event registration failed: {e}"));
                None
            }
        }
    } else {
        None
    };
    let subscription = Subscription { listener, token };
    let availability = if token.is_some() {
        Availability::Live
    } else {
        Availability::Cached
    };
    publish.set(
        availability.clone(),
        Some(Data::Notifications(read_scoped(
            &subscription.listener,
            config.show_text,
            config.discord_previews,
        )?)),
    );
    loop {
        tokio::select! {
            _ = changed.notified(), if token.is_some() => {
                publish.set(availability.clone(), Some(Data::Notifications(read_scoped(&subscription.listener, config.show_text,config.discord_previews)?)));
            }
            Some(action) = commands.recv() => if matches!(action, crate::Action::Dismiss(_) | crate::Action::RefreshNotifications) {
                if let crate::Action::Dismiss(ids) = action {
                    // Attempt every card, even if one toast disappeared between read and click.
                    let mut error = None;
                    for id in ids {
                        if let Err(e) = subscription.listener.RemoveNotification(id) {
                            error.get_or_insert(anyhow::Error::from(e));
                        }
                    }
                    publish.action_result(error.map_or(Ok(()), Err));
                }
                publish.set(availability.clone(), Some(Data::Notifications(read_scoped(&subscription.listener, config.show_text,config.discord_previews)?)));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn notification_text_limits_bound_bytes_and_keep_unicode_valid() {
        assert_eq!(super::text("مرحبا", 5), "مر");
        assert_eq!(super::text("🙂hello", 3), "");
        assert_eq!(super::text(&"🙂".repeat(1024), 256).len(), 256);
    }
}
