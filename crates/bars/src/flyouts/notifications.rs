//! Notifications (docs/bars.md, "Flyouts" → "Notifications"), on the `notifications` lane, through
//! `UserNotificationListener`. Measured on build 26300 with the unpackaged `mypaper-bars`
//! (`examples/winrt-spike.rs`): access is `Allowed`, reading every toast works, and so does
//! removing one; the `NotificationChanged` event refuses to register without package identity
//! (`0x80070490`, "Element not found"). So the list is read when the flyout opens (and every few
//! seconds while it's open), and the bar's badge is re-counted on a timer while a `notifications`
//! module is on screen.

use std::collections::HashMap;
use std::path::PathBuf;

use serde::Serialize;
use windows::Foundation::Size;
use windows::Storage::Streams::DataReader;
use windows::UI::Notifications::Management::{
    UserNotificationListener, UserNotificationListenerAccessStatus,
};
use windows::UI::Notifications::{KnownNotificationBindings, NotificationKinds, UserNotification};
use windows::Win32::System::Com::{COINIT_MULTITHREADED, CoInitializeEx};

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Notification {
    pub id: u32,
    pub app: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>,
    pub title: String,
    pub body: String,
    /// When it arrived, ms since the epoch.
    pub at: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct NotificationList {
    pub notifications: Vec<Notification>,
    /// Set when they can't be read: `denied` (turned off in Settings) or `unavailable`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<&'static str>,
}

/// `DateTime.UniversalTime` (100 ns since 1601) → ms since the epoch.
pub fn epoch_ms(universal: i64) -> u64 {
    const EPOCH: i64 = 116_444_736_000_000_000;
    (universal.saturating_sub(EPOCH).max(0) / 10_000) as u64
}

/// A toast's title and body from its text elements: the first line is the title.
pub fn title_and_body(texts: &[String]) -> (String, String) {
    let mut lines = texts.iter().map(|t| t.trim()).filter(|t| !t.is_empty());
    let title = lines.next().unwrap_or_default().to_owned();
    let body = lines.collect::<Vec<_>>().join("\n");
    (title, body)
}

/// Newest first.
pub fn order(list: &mut [Notification]) {
    list.sort_by(|a, b| b.at.cmp(&a.at).then(b.id.cmp(&a.id)));
}

pub struct NotificationsLane {
    listener: Option<UserNotificationListener>,
    /// App → its logo's URL in the icon cache.
    logos: HashMap<String, Option<String>>,
    icons: PathBuf,
}

impl NotificationsLane {
    pub fn new(icons: PathBuf) -> Self {
        // SAFETY: COM for this thread (MTA), kept for its life.
        let _ = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
        Self {
            listener: UserNotificationListener::Current()
                .inspect_err(|e| log::info!("notifications: no listener: {e}"))
                .ok(),
            logos: HashMap::new(),
            icons,
        }
    }

    fn allowed(&self) -> Option<&UserNotificationListener> {
        let listener = self.listener.as_ref()?;
        let status = match listener.GetAccessStatus() {
            Ok(UserNotificationListenerAccessStatus::Allowed) => return Some(listener),
            Ok(UserNotificationListenerAccessStatus::Unspecified) => {
                listener.RequestAccessAsync().and_then(|op| op.join()).ok()
            }
            other => other.ok(),
        };
        (status == Some(UserNotificationListenerAccessStatus::Allowed)).then_some(listener)
    }

    fn toasts(&self) -> Option<Vec<UserNotification>> {
        self.allowed()?
            .GetNotificationsAsync(NotificationKinds::Toast)
            .and_then(|op| op.join())
            .ok()
            .map(|list| list.into_iter().collect())
    }

    pub fn count(&self) -> Option<usize> {
        self.toasts().map(|t| t.len())
    }

    pub fn list(&mut self) -> NotificationList {
        if self.listener.is_some() && self.allowed().is_none() {
            return NotificationList {
                notifications: Vec::new(),
                error: Some("denied"),
            };
        }
        let Some(toasts) = self.toasts() else {
            return NotificationList {
                notifications: Vec::new(),
                error: Some("unavailable"),
            };
        };
        let mut notifications: Vec<Notification> = toasts
            .iter()
            .filter_map(|toast| {
                let info = toast.AppInfo().ok();
                let display = info.as_ref().and_then(|i| i.DisplayInfo().ok());
                let app = display
                    .as_ref()
                    .and_then(|d| d.DisplayName().ok())
                    .map(|n| n.to_string())
                    .unwrap_or_default();
                let key = info
                    .as_ref()
                    .and_then(|i| i.AppUserModelId().ok())
                    .map(|i| i.to_string())
                    .unwrap_or_else(|| app.clone());
                let icon = match self.logos.get(&key) {
                    Some(icon) => icon.clone(),
                    None => {
                        let icon = display.as_ref().and_then(|d| self.logo(d));
                        self.logos.insert(key, icon.clone());
                        icon
                    }
                };
                let texts: Vec<String> = toast
                    .Notification()
                    .and_then(|n| n.Visual())
                    .and_then(|v| v.GetBinding(&KnownNotificationBindings::ToastGeneric()?))
                    .and_then(|b| b.GetTextElements())
                    .map(|elements| {
                        elements
                            .into_iter()
                            .filter_map(|e| e.Text().ok().map(|t| t.to_string()))
                            .collect()
                    })
                    .unwrap_or_default();
                let (title, body) = title_and_body(&texts);
                Some(Notification {
                    id: toast.Id().ok()?,
                    app,
                    icon,
                    title,
                    body,
                    at: toast
                        .CreationTime()
                        .map(|t| epoch_ms(t.UniversalTime))
                        .unwrap_or(0),
                })
            })
            .collect();
        order(&mut notifications);
        NotificationList {
            notifications,
            error: None,
        }
    }

    /// The app's logo, stored in the icon cache.
    fn logo(&self, display: &windows::ApplicationModel::AppDisplayInfo) -> Option<String> {
        let reference = display
            .GetLogo(Size {
                Width: 64.0,
                Height: 64.0,
            })
            .ok()?;
        let stream = reference.OpenReadAsync().ok()?.join().ok()?;
        let size = stream.Size().ok()?;
        if size == 0 || size > 1 << 20 {
            return None;
        }
        let reader = DataReader::CreateDataReader(&stream).ok()?;
        reader.LoadAsync(size as u32).ok()?.join().ok()?;
        let mut bytes = vec![0u8; size as usize];
        reader.ReadBytes(&mut bytes).ok()?;
        bytes
            .starts_with(b"\x89PNG")
            .then(|| crate::icons::store_png(&self.icons, &bytes))
            .flatten()
    }

    pub fn dismiss(&self, id: u32) -> Result<(), String> {
        let listener = self.allowed().ok_or("notification access is off")?;
        listener.RemoveNotification(id).map_err(|e| e.to_string())
    }

    /// Clears them all; one by one when the listener won't clear without package identity.
    pub fn clear(&self) -> Result<(), String> {
        let listener = self.allowed().ok_or("notification access is off")?;
        if listener.ClearNotifications().is_ok() {
            return Ok(());
        }
        for toast in self.toasts().unwrap_or_default() {
            if let Ok(id) = toast.Id() {
                listener.RemoveNotification(id).map_err(|e| e.to_string())?;
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn toast_texts_become_a_title_and_a_body() {
        let texts = ["Maya", "  ", "Are we still on for lunch?", "See you"].map(String::from);
        assert_eq!(
            title_and_body(&texts),
            ("Maya".into(), "Are we still on for lunch?\nSee you".into())
        );
        assert_eq!(title_and_body(&[]), (String::new(), String::new()));
    }

    #[test]
    fn times_and_order() {
        // 2026-01-01T00:00:00Z.
        assert_eq!(epoch_ms(134_116_992_000_000_000), 1_767_225_600_000);
        assert_eq!(epoch_ms(0), 0);
        let n = |id, at| Notification {
            id,
            app: String::new(),
            icon: None,
            title: String::new(),
            body: String::new(),
            at,
        };
        let mut list = vec![n(1, 10), n(2, 30), n(3, 20)];
        order(&mut list);
        assert_eq!(list.iter().map(|n| n.id).collect::<Vec<_>>(), [2, 3, 1]);
    }
}
