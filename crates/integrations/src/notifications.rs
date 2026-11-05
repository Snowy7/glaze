//! Retained Windows toasts, grouped by AUMID. These are never called unread messages.
use crate::{Availability, Data, Publish};
use anyhow::{Result, ensure};
use std::{collections::BTreeMap, sync::Arc};
use tokio::sync::{Notify, mpsc};
use windows::{
    Foundation::TypedEventHandler,
    UI::Notifications::Management::{
        UserNotificationListener, UserNotificationListenerAccessStatus,
    },
    UI::Notifications::{KnownNotificationBindings, NotificationKinds},
};

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

/// Call on the UI thread, in response to the user's Enable/Grant access action. Never prompt at startup.
pub fn request_access()
-> windows::core::Result<windows_future::IAsyncOperation<UserNotificationListenerAccessStatus>> {
    UserNotificationListener::Current()?.RequestAccessAsync()
}
pub fn read(listener: &UserNotificationListener, show_text: bool) -> Result<Snapshot> {
    ensure!(
        listener.GetAccessStatus()? == UserNotificationListenerAccessStatus::Allowed,
        "notification access is off; grant access from Studio"
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
        if show_text
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
    let listener = UserNotificationListener::Current()?;
    ensure!(
        listener.GetAccessStatus()? == UserNotificationListenerAccessStatus::Allowed,
        "grant Windows notification access in Studio"
    );
    let changed = Arc::new(Notify::new());
    let notify = changed.clone();
    let events = publish.clone();
    let token = if config.live_verified {
        Some(
            listener.NotificationChanged(&TypedEventHandler::new(move |_, _| {
                events.delivered();
                notify.notify_one();
                Ok(())
            }))?,
        )
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
        Some(Data::Notifications(read(
            &subscription.listener,
            config.show_text,
        )?)),
    );
    loop {
        tokio::select! {
            _ = changed.notified(), if token.is_some() => {
                publish.set(availability.clone(), Some(Data::Notifications(read(&subscription.listener, config.show_text)?)));
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
                publish.set(availability.clone(), Some(Data::Notifications(read(&subscription.listener, config.show_text)?)));
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
