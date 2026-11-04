//! Spike (docs/bars.md, "Flyouts" → "Notifications"): which WinRT APIs answer an unpackaged
//! desktop process on this build? Prints what each call returns; changes nothing.
//!
//! ```text
//! cargo run -p mypaper-bars --example winrt-spike
//! ```

use windows::ApplicationModel::DataTransfer::Clipboard;
use windows::Devices::Radios::Radio;
use windows::UI::Notifications::Management::UserNotificationListener;
use windows::UI::Notifications::{NotificationKinds, ToastNotificationManager};
use windows::UI::Shell::FocusSessionManager;
use windows::Win32::System::Com::{COINIT_APARTMENTTHREADED, CoInitializeEx};

fn main() {
    // SAFETY: COM for this thread, once, before any WinRT call.
    let _ = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) };

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
