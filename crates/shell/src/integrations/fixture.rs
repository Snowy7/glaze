//! The widget-only executable uses the same shared media adapter without linking bar/flyout UI.
#[path = "media.rs"]
pub mod media;
use mypaper_integrations::{Hub, Lease, Provider, config::Config};
use std::{
    cell::RefCell,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};
use windows::Win32::{
    Foundation::{LPARAM, WPARAM},
    System::Threading::GetCurrentThreadId,
    UI::WindowsAndMessaging::{PostThreadMessageW, WM_APP},
};
thread_local! {static STATE:RefCell<(Hub,Arc<AtomicBool>)>=RefCell::new({
    // SAFETY: the fixture UI thread owns this message queue.
    let thread=unsafe {GetCurrentThreadId()};
    let dirty=Arc::new(AtomicBool::new(false));let changed=dirty.clone();
    let hub=Hub::new(move |_| {
        if !changed.swap(true,Ordering::AcqRel) {
            crate::inbox::post(WM_APP+146,0);
            // SAFETY: a queue wake carries no pointers and tolerates an exited UI thread.
            let _=unsafe {PostThreadMessageW(thread,WM_APP+146,WPARAM(0),LPARAM(0))};
        }
    });(hub,dirty)
});}
pub fn acquire(provider: Provider) -> anyhow::Result<Lease> {
    STATE.with_borrow(|(hub, _)| hub.acquire(provider, &Config::default()))
}
pub fn process() {
    if STATE.with_borrow(|(_, dirty)| dirty.swap(false, Ordering::AcqRel)) {
        media::changed(false);
    }
}
