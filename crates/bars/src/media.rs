//! Now playing and transport controls through Windows' System Media Transport Controls (SMTC).
//!
//! SMTC is WinRT with blocking async calls and callbacks on thread-pool threads, so it lives on
//! its own MTA worker thread, which exists only while a media module is on screen.

use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::time::Duration;

use serde::Serialize;
use windows::Foundation::TypedEventHandler;
use windows::Media::Control::{
    GlobalSystemMediaTransportControlsSession as Session,
    GlobalSystemMediaTransportControlsSessionManager as Manager,
    GlobalSystemMediaTransportControlsSessionPlaybackStatus as Status,
};
use windows::Win32::System::Com::{COINIT_MULTITHREADED, CoInitializeEx, CoUninitialize};

use crate::bridge::MediaAction;
use crate::wake::{Msg, Waker};

/// Coalesces the burst of SMTC events one track change produces.
const SETTLE: Duration = Duration::from_millis(80);

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct MediaInfo {
    pub title: String,
    pub artist: String,
    pub app: String,
    pub playing: bool,
}

enum Cmd {
    Changed,
    Control(MediaAction),
    Stop,
}

/// The worker while it runs.
pub struct Media {
    worker: Option<Sender<Cmd>>,
    waker: Waker,
}

impl Media {
    pub fn new(waker: Waker) -> Self {
        Self {
            worker: None,
            waker,
        }
    }

    /// Runs the worker while `on`; it reports the current state at once, then every change.
    pub fn set_active(&mut self, on: bool) {
        match (on, self.worker.is_some()) {
            (true, false) => {
                let (tx, rx) = mpsc::channel();
                let (waker, changed) = (self.waker.clone(), tx.clone());
                let spawned = std::thread::Builder::new()
                    .name("media".into())
                    .spawn(move || run(rx, changed, waker));
                match spawned {
                    Ok(_) => self.worker = Some(tx),
                    Err(e) => log::warn!("no media worker: {e}"),
                }
            }
            (false, true) => {
                if let Some(worker) = self.worker.take() {
                    let _ = worker.send(Cmd::Stop);
                }
            }
            _ => {}
        }
    }

    pub fn control(&self, action: MediaAction) {
        if let Some(worker) = &self.worker {
            let _ = worker.send(Cmd::Control(action));
        }
    }
}

impl Drop for Media {
    fn drop(&mut self) {
        self.set_active(false);
    }
}

fn run(rx: Receiver<Cmd>, changed: Sender<Cmd>, waker: Waker) {
    // SAFETY: COM for this thread; balanced by CoUninitialize below.
    let com = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) }.is_ok();
    match Manager::RequestAsync().and_then(|op| op.join()) {
        Ok(manager) => serve(&manager, &rx, &changed, &waker),
        Err(e) => {
            log::warn!("media controls unavailable: {e}");
            waker.send(Msg::Media(None));
        }
    }
    if com {
        // SAFETY: balances the CoInitializeEx above, on the same thread.
        unsafe { CoUninitialize() };
    }
}

fn notify<S: windows::core::RuntimeType + 'static, A: windows::core::RuntimeType + 'static>(
    changed: &Sender<Cmd>,
) -> TypedEventHandler<S, A> {
    let changed = changed.clone();
    TypedEventHandler::new(move |_, _| {
        let _ = changed.send(Cmd::Changed);
        Ok(())
    })
}

/// Listens to the manager and the current session until told to stop.
fn serve(manager: &Manager, rx: &Receiver<Cmd>, changed: &Sender<Cmd>, waker: &Waker) {
    let manager_tokens = [
        manager
            .CurrentSessionChanged(&notify(changed))
            .unwrap_or_default(),
        manager
            .SessionsChanged(&notify(changed))
            .unwrap_or_default(),
    ];
    let mut session: Option<(Session, [i64; 2])> = None;
    let follow = |session: &mut Option<(Session, [i64; 2])>| {
        let current = manager.GetCurrentSession().ok();
        if current.as_ref() == session.as_ref().map(|(s, _)| s) {
            return;
        }
        if let Some((old, [a, b])) = session.take() {
            let _ = old.RemoveMediaPropertiesChanged(a);
            let _ = old.RemovePlaybackInfoChanged(b);
        }
        *session = current.map(|s| {
            let tokens = [
                s.MediaPropertiesChanged(&notify(changed))
                    .unwrap_or_default(),
                s.PlaybackInfoChanged(&notify(changed)).unwrap_or_default(),
            ];
            (s, tokens)
        });
    };
    follow(&mut session);
    let mut sent = state(manager);
    waker.send(Msg::Media(sent.clone()));
    let mut dirty = false;
    loop {
        let command = if dirty {
            match rx.recv_timeout(SETTLE) {
                Ok(command) => command,
                Err(RecvTimeoutError::Timeout) => {
                    dirty = false;
                    follow(&mut session);
                    let now = state(manager);
                    if now != sent {
                        sent = now;
                        waker.send(Msg::Media(sent.clone()));
                    }
                    continue;
                }
                Err(RecvTimeoutError::Disconnected) => break,
            }
        } else {
            match rx.recv() {
                Ok(command) => command,
                Err(_) => break,
            }
        };
        match command {
            Cmd::Changed => dirty = true,
            Cmd::Control(action) => {
                if let Err(e) = control(manager, action) {
                    log::info!("media {action:?}: {e}");
                }
            }
            Cmd::Stop => break,
        }
    }
    let [current, sessions] = manager_tokens;
    let _ = manager.RemoveCurrentSessionChanged(current);
    let _ = manager.RemoveSessionsChanged(sessions);
    if let Some((old, [a, b])) = session {
        let _ = old.RemoveMediaPropertiesChanged(a);
        let _ = old.RemovePlaybackInfoChanged(b);
    }
}

fn control(manager: &Manager, action: MediaAction) -> Result<(), String> {
    let session = manager
        .GetCurrentSession()
        .map_err(|_| "nothing is playing".to_owned())?;
    let operation = match action {
        MediaAction::Toggle => session.TryTogglePlayPauseAsync(),
        MediaAction::Next => session.TrySkipNextAsync(),
        MediaAction::Previous => session.TrySkipPreviousAsync(),
    };
    match operation.and_then(|op| op.join()) {
        Ok(true) => Ok(()),
        Ok(false) => Err("the player refused".into()),
        Err(e) => Err(e.to_string()),
    }
}

fn state(manager: &Manager) -> Option<MediaInfo> {
    let session = manager.GetCurrentSession().ok()?;
    let properties = session
        .TryGetMediaPropertiesAsync()
        .and_then(|op| op.join())
        .ok()?;
    let text = |value: windows::core::Result<windows::core::HSTRING>| {
        value.map(|s| s.to_string()).unwrap_or_default()
    };
    let title = text(properties.Title());
    let artist = text(properties.Artist());
    if title.is_empty() && artist.is_empty() {
        return None;
    }
    let playing = session
        .GetPlaybackInfo()
        .and_then(|info| info.PlaybackStatus())
        .is_ok_and(|status| status == Status::Playing);
    Some(MediaInfo {
        title,
        artist,
        app: app_name(&text(session.SourceAppUserModelId())),
        playing,
    })
}

/// A readable app name from an AppUserModelID: `Spotify.exe` → `Spotify`,
/// `SpotifyAB.SpotifyMusic_zpdnekdrzrea0!Spotify` → `Spotify`.
fn app_name(aumid: &str) -> String {
    let name = aumid.rsplit('!').next().unwrap_or(aumid);
    let name = match name.len().checked_sub(4) {
        Some(at) if name[at..].eq_ignore_ascii_case(".exe") => &name[..at],
        _ => name,
    };
    name.rsplit('.').next().unwrap_or(name).to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn app_names_are_readable() {
        assert_eq!(app_name("Spotify.exe"), "Spotify");
        assert_eq!(
            app_name("SpotifyAB.SpotifyMusic_zpdnekdrzrea0!Spotify"),
            "Spotify"
        );
        assert_eq!(app_name("MSEdge"), "MSEdge");
    }
}
