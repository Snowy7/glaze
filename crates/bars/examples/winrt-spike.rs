//! Spike (docs/bars.md, "Flyouts" → "Notifications"): which WinRT APIs answer an unpackaged
//! desktop process on this build? Prints what each call returns and changes nothing, except with
//! `--toast`: that registers a throwaway app id (`HKCU\Software\Classes\AppUserModelId\
//! mypaper.spike`), posts one silent toast from it (no banner), removes it through the listener,
//! and deletes the app id again.
//!
//! ```text
//! cargo run -p mypaper-bars --example winrt-spike [-- --toast]
//! ```

use windows::ApplicationModel::DataTransfer::Clipboard;
use windows::Data::Xml::Dom::XmlDocument;
use windows::Devices::Radios::Radio;
use windows::UI::Notifications::Management::UserNotificationListener;
use windows::UI::Notifications::{
    NotificationKinds, ToastNotification, ToastNotificationManager,
};
use windows::UI::Shell::FocusSessionManager;
use windows::Win32::System::Com::{COINIT_APARTMENTTHREADED, CoInitializeEx};
use windows::Win32::System::Registry::{
    HKEY_CURRENT_USER, REG_SZ, RegDeleteTreeW, RegSetKeyValueW,
};
use windows::core::{HSTRING, w};

const SPIKE_APP: &str = "mypaper.spike";

/// Posts a silent toast from a throwaway app id, removes it through the listener, and reports
/// whether removing worked.
fn toast_round_trip(listener: &UserNotificationListener) -> windows::core::Result<()> {
    let key = w!(r"Software\Classes\AppUserModelId\mypaper.spike");
    let name: Vec<u16> = "mypaper spike\0".encode_utf16().collect();
    // SAFETY: a value under our own throwaway key; the data pointer and size describe `name`.
    unsafe {
        RegSetKeyValueW(
            HKEY_CURRENT_USER,
            key,
            w!("DisplayName"),
            REG_SZ.0,
            Some(name.as_ptr().cast()),
            (name.len() * 2) as u32,
        )
        .ok()?;
    }
    let result = (|| {
        let xml = XmlDocument::new()?;
        xml.LoadXml(&HSTRING::from(
            "<toast><visual><binding template=\"ToastGeneric\"><text>mypaper spike</text><text>removed right away</text></binding></visual></toast>",
        ))?;
        let toast = ToastNotification::CreateToastNotification(&xml)?;
        toast.SetSuppressPopup(true)?;
        ToastNotificationManager::CreateToastNotifierWithId(&HSTRING::from(SPIKE_APP))?
            .Show(&toast)?;
        std::thread::sleep(std::time::Duration::from_millis(800));
        let ours = || -> windows::core::Result<Vec<u32>> {
            Ok(listener
                .GetNotificationsAsync(NotificationKinds::Toast)?
                .join()?
                .into_iter()
                .filter(|n| {
                    n.AppInfo()
                        .and_then(|a| a.DisplayInfo())
                        .and_then(|d| d.DisplayName())
                        .is_ok_and(|name| name == "mypaper spike")
                })
                .filter_map(|n| n.Id().ok())
                .collect())
        };
        let before = ours()?;
        println!("spike toasts listed: {before:?}");
        for id in &before {
            println!("RemoveNotification({id}): {:?}", listener.RemoveNotification(*id));
        }
        std::thread::sleep(std::time::Duration::from_millis(300));
        let after = ours()?;
        println!("spike toasts after removing: {after:?}");
        Ok(())
    })();
    // SAFETY: deletes only our throwaway key.
    let _ = unsafe { RegDeleteTreeW(HKEY_CURRENT_USER, key) };
    result
}

fn main() {
    // SAFETY: COM for this thread, once, before any WinRT call.
    let _ = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) };

    if std::env::args().any(|a| a == "--toast") {
        match UserNotificationListener::Current() {
            Ok(listener) => println!("toast round trip: {:?}", toast_round_trip(&listener)),
            Err(e) => println!("Current(): {e:?}"),
        }
        return;
    }

    println!("--- UserNotificationListener");
    match UserNotificationListener::Current() {
        Err(e) => println!("Current(): {e:?}"),
        Ok(listener) => {
            println!("GetAccessStatus: {:?}", listener.GetAccessStatus());
            match listener.RequestAccessAsync() {
                Err(e) => println!("RequestAccessAsync(): {e:?}"),
                Ok(op) => println!("RequestAccessAsync → {:?}", op.join()),
            }
            match listener.GetNotificationsAsync(NotificationKinds::Toast) {
                Err(e) => println!("GetNotificationsAsync(): {e:?}"),
                Ok(op) => match op.join() {
                    Err(e) => println!("GetNotificationsAsync → {e:?}"),
                    Ok(list) => {
                        println!(
                            "GetNotificationsAsync → {} toasts",
                            list.Size().unwrap_or(0)
                        );
                        for n in list.into_iter().take(5) {
                            let app = n
                                .AppInfo()
                                .and_then(|a| a.DisplayInfo())
                                .and_then(|d| d.DisplayName());
                            println!("  id {:?} app {:?}", n.Id(), app);
                        }
                    }
                },
            }
            let handler = windows::Foundation::TypedEventHandler::new(|_, _| Ok(()));
            match listener.NotificationChanged(&handler) {
                Ok(token) => {
                    println!("NotificationChanged: registered");
                    let _ = listener.RemoveNotificationChanged(token);
                }
                Err(e) => println!("NotificationChanged: {e:?}"),
            }
        }
    }

    println!("--- Do not disturb / focus");
    println!(
        "NotificationMode: {:?}",
        ToastNotificationManager::GetDefault().and_then(|m| m.NotificationMode())
    );
    println!(
        "FocusSessionManager.IsSupported: {:?}",
        FocusSessionManager::IsSupported()
    );
    println!(
        "IsFocusActive: {:?}",
        FocusSessionManager::GetDefault().and_then(|m| m.IsFocusActive())
    );

    println!("--- Clipboard history");
    println!("IsHistoryEnabled: {:?}", Clipboard::IsHistoryEnabled());
    match Clipboard::GetHistoryItemsAsync() {
        Err(e) => println!("GetHistoryItemsAsync(): {e:?}"),
        Ok(op) => match op.join() {
            Err(e) => println!("GetHistoryItemsAsync → {e:?}"),
            Ok(result) => {
                println!("status {:?}", result.Status());
                if let Ok(items) = result.Items() {
                    println!("items: {}", items.Size().unwrap_or(0));
                    for item in items.into_iter().take(3) {
                        let formats = item
                            .Content()
                            .and_then(|c| c.AvailableFormats())
                            .map(|f| f.into_iter().map(|s| s.to_string()).collect::<Vec<_>>());
                        println!("  formats {formats:?}");
                    }
                }
            }
        },
    }

    println!("--- Radios");
    match Radio::RequestAccessAsync() {
        Err(e) => println!("RequestAccessAsync(): {e:?}"),
        Ok(op) => println!("RequestAccessAsync → {:?}", op.join()),
    }
    match Radio::GetRadiosAsync() {
        Err(e) => println!("GetRadiosAsync(): {e:?}"),
        Ok(op) => match op.join() {
            Err(e) => println!("GetRadiosAsync → {e:?}"),
            Ok(list) => {
                for r in list {
                    println!("  {:?} {:?} {:?}", r.Name(), r.Kind(), r.State());
                }
            }
        },
    }
}
