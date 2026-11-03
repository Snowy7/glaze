//! `media`: now playing and transport controls through Windows' System Media
//! Transport Controls (SMTC).
//!
//! SMTC is WinRT with blocking async calls and callbacks on thread-pool
//! threads, so it lives on its own MTA worker thread that exists only while a
//! widget is listening or waiting for an answer, and never while held.

use std::collections::{BTreeSet, HashMap};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::time::{Duration, Instant};

use serde::Serialize;
use serde_json::Value;
use windows::Foundation::TypedEventHandler;
use windows::Media::Control::{
    GlobalSystemMediaTransportControlsSession as Session,
    GlobalSystemMediaTransportControlsSessionManager as Manager,
    GlobalSystemMediaTransportControlsSessionMediaProperties as Properties,
    GlobalSystemMediaTransportControlsSessionPlaybackStatus as Status,
};
use windows::Storage::Streams::DataReader;
use windows::Win32::System::Com::{COINIT_MULTITHREADED, CoInitializeEx, CoUninitialize};
use windows::Win32::System::SystemInformation::GetSystemTimeAsFileTime;

use crate::bridge::{ErrorCode, MediaAction, Out, Target, WidgetError};
use crate::wake::{Msg, Waker};

/// Bigger thumbnails are left out.
const MAX_ARTWORK: u64 = 256 << 10;
/// Coalesces the burst of SMTC events one track change produces.
const SETTLE: Duration = Duration::from_millis(80);
/// A position this far from where it should be by now is a seek.
const SEEK: f64 = 2.0;

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct MediaState {
    pub title: String,
    pub artist: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub album: Option<String>,
    pub app: String,
    pub playing: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub position: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duration: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub artwork: Option<String>,
}

enum Cmd {
    Query(u64),
    Control(u64, MediaAction),
    /// Something changed in SMTC.
    Changed,
    Stop,
}

/// The host side: who listens, who waits, and the worker if one runs.
pub struct Media {
    worker: Option<Sender<Cmd>>,
    subs: BTreeSet<String>,
    pending: HashMap<u64, Target>,
    next_token: u64,
    held: bool,
    waker: Waker,
}

impl Media {
    pub fn new(waker: Waker) -> Self {
        Self {
            worker: None,
            subs: BTreeSet::new(),
            pending: HashMap::new(),
            next_token: 0,
            held: false,
            waker,
        }
    }

    fn worker(&mut self) -> &Sender<Cmd> {
        self.worker.get_or_insert_with(|| {
            let (tx, rx) = mpsc::channel();
            let waker = self.waker.clone();
            let changed = tx.clone();
            let spawned = std::thread::Builder::new()
                .name("media".into())
                .spawn(move || run(rx, changed, waker));
            if let Err(e) = spawned {
                log::warn!("no media worker: {e}");
            }
            tx
        })
    }

    fn ask(&mut self, target: Target, command: impl FnOnce(u64) -> Cmd) {
        self.next_token += 1;
        let token = self.next_token;
        self.pending.insert(token, target);
        let command = command(token);
        let _ = self.worker().send(command);
    }

    pub fn current(&mut self, target: Target) {
        self.ask(target, Cmd::Query);
    }

    pub fn control(&mut self, target: Target, action: MediaAction) {
        self.ask(target, |token| Cmd::Control(token, action));
    }

    pub fn subscribe(&mut self, instance: &str) {
        self.subs.insert(instance.to_owned());
        self.worker();
    }

    pub fn unsubscribe(&mut self, instance: &str) {
        self.subs.remove(instance);
        self.stop_if_idle();
    }

    pub fn forget(&mut self, instance: &str) {
        self.unsubscribe(instance);
    }

    /// The worker answered (`token`) or saw a change (`None`).
    pub fn on_result(&mut self, token: Option<u64>, result: Result<Value, String>) -> Vec<Out> {
        let out = match token {
            Some(token) => self
                .pending
                .remove(&token)
                .map(|target| {
                    let reply = result.map_err(|e| WidgetError::new(ErrorCode::Internal, e));
                    Out::Reply(target, reply)
                })
                .into_iter()
                .collect(),
            None => match result {
                Ok(state) if !self.held => self
                    .subs
                    .iter()
                    .map(|instance| Out::Event {
                        instance: instance.clone(),
                        event: "media",
                        data: state.clone(),
                    })
                    .collect(),
                Ok(_) => Vec::new(),
                Err(e) => {
                    log::warn!("media: {e}");
                    Vec::new()
                }
            },
        };
        self.stop_if_idle();
        out
    }

    /// Held: the worker goes away (and with it every SMTC callback).
    pub fn set_held(&mut self, held: bool) {
        self.held = held;
        if held {
            self.stop();
        } else if !self.subs.is_empty() {
            self.worker();
        }
    }

    fn stop_if_idle(&mut self) {
        if self.subs.is_empty() && self.pending.is_empty() {
            self.stop();
        }
    }

    fn stop(&mut self) {
        if let Some(worker) = self.worker.take() {
            let _ = worker.send(Cmd::Stop);
        }
    }
}

/// The worker: answers queries and reports changes until told to stop.
fn run(rx: Receiver<Cmd>, changed: Sender<Cmd>, waker: Waker) {
    // SAFETY: initializes COM for this thread; balanced by CoUninitialize below.
    let com = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) }.is_ok();
    let answer = |token, result| {
        waker.send(Msg::Media {
            token: Some(token),
            result,
        })
    };
    match Manager::RequestAsync().and_then(|op| op.join()) {
        Ok(manager) => Worker::new(manager, changed).serve(&rx, &waker),
        Err(e) => {
            // No SMTC: answer whatever comes in, then stop.
            for command in rx {
                match command {
                    Cmd::Query(token) | Cmd::Control(token, _) => {
                        answer(token, Err(format!("media controls unavailable: {e}")))
                    }
                    Cmd::Changed => {}
                    Cmd::Stop => break,
                }
            }
        }
    }
    if com {
        // SAFETY: balances the CoInitializeEx above, on the same thread.
        unsafe { CoUninitialize() };
    }
}

struct Worker {
    manager: Manager,
    changed: Sender<Cmd>,
    manager_tokens: [i64; 2],
    /// The session we listen to, with its three event tokens.
    session: Option<(Session, [i64; 3])>,
    /// What subscribers last got, and when.
    sent: Option<(Option<MediaState>, Instant)>,
    /// The artwork for the current track.
    artwork: Option<(String, Option<String>)>,
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

impl Worker {
    fn new(manager: Manager, changed: Sender<Cmd>) -> Self {
        let manager_tokens = [
            manager
                .CurrentSessionChanged(&notify(&changed))
                .unwrap_or_default(),
            manager
                .SessionsChanged(&notify(&changed))
                .unwrap_or_default(),
        ];
        let mut worker = Self {
            manager,
            changed,
            manager_tokens,
            session: None,
            sent: None,
            artwork: None,
        };
        worker.follow_current();
        worker
    }

    /// Listens to the current session's events (and not the previous one's).
    fn follow_current(&mut self) {
        let current = self.manager.GetCurrentSession().ok();
        if current.as_ref() == self.session.as_ref().map(|(s, _)| s) {
            return;
        }
        self.unfollow();
        self.session = current.map(|session| {
            let tokens = [
                session
                    .MediaPropertiesChanged(&notify(&self.changed))
                    .unwrap_or_default(),
                session
                    .PlaybackInfoChanged(&notify(&self.changed))
                    .unwrap_or_default(),
                session
                    .TimelinePropertiesChanged(&notify(&self.changed))
                    .unwrap_or_default(),
            ];
            (session, tokens)
        });
    }

    fn serve(mut self, rx: &Receiver<Cmd>, waker: &Waker) {
        let report = |token, result| waker.send(Msg::Media { token, result });
        let mut dirty = true; // subscribers get the state right away
        loop {
            let command = if dirty {
                match rx.recv_timeout(SETTLE) {
                    Ok(command) => command,
                    Err(RecvTimeoutError::Timeout) => {
                        dirty = false;
                        self.follow_current();
                        let state = self.state();
                        let now = Instant::now();
                        if worth_sending(self.sent.as_ref(), state.as_ref(), now) {
                            report(None, Ok(to_json(&state)));
                            self.sent = Some((state, now));
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
                Cmd::Query(token) => {
                    self.follow_current();
                    report(Some(token), Ok(to_json(&self.state())));
                }
                Cmd::Control(token, action) => {
                    report(Some(token), self.control(action).map(|()| Value::Null))
                }
                Cmd::Stop => break,
            }
        }
        let [current, sessions] = self.manager_tokens;
        let _ = self.manager.RemoveCurrentSessionChanged(current);
        let _ = self.manager.RemoveSessionsChanged(sessions);
        self.unfollow();
    }

    fn unfollow(&mut self) {
        if let Some((session, [a, b, c])) = self.session.take() {
            let _ = session.RemoveMediaPropertiesChanged(a);
            let _ = session.RemovePlaybackInfoChanged(b);
            let _ = session.RemoveTimelinePropertiesChanged(c);
        }
    }

    fn control(&self, action: MediaAction) -> Result<(), String> {
        let session = self
            .manager
            .GetCurrentSession()
            .map_err(|_| "nothing is playing".to_owned())?;
        let operation = match action {
            MediaAction::Play => session.TryPlayAsync(),
            MediaAction::Pause => session.TryPauseAsync(),
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

    fn state(&mut self) -> Option<MediaState> {
        let session = self.manager.GetCurrentSession().ok()?;
        let properties = session
            .TryGetMediaPropertiesAsync()
            .and_then(|op| op.join())
            .ok()?;
        let title = properties
            .Title()
            .map(|s| s.to_string())
            .unwrap_or_default();
        let artist = properties
            .Artist()
            .map(|s| s.to_string())
            .unwrap_or_default();
        if title.is_empty() && artist.is_empty() {
            return None;
        }
        let album = properties
            .AlbumTitle()
            .map(|s| s.to_string())
            .ok()
            .filter(|s| !s.is_empty());
        let app = app_name(
            &session
                .SourceAppUserModelId()
                .map(|s| s.to_string())
                .unwrap_or_default(),
        );
        let playing = session
            .GetPlaybackInfo()
            .and_then(|info| info.PlaybackStatus())
            .is_ok_and(|status| status == Status::Playing);
        let (position, duration) =
            session
                .GetTimelineProperties()
                .ok()
                .map_or((None, None), |timeline| {
                    let seconds = |ticks: i64| ticks as f64 / 1e7;
                    let start = timeline.StartTime().map_or(0, |t| t.Duration);
                    let end = timeline.EndTime().map_or(0, |t| t.Duration);
                    if end <= start {
                        return (None, None);
                    }
                    let mut position = timeline.Position().map_or(0, |t| t.Duration) - start;
                    if playing && let Ok(updated) = timeline.LastUpdatedTime() {
                        // SMTC reports the position as of its last update.
                        position += (now_filetime() - updated.UniversalTime).max(0);
                    }
                    (
                        Some(seconds(position.clamp(0, end - start))),
                        Some(seconds(end - start)),
                    )
                });
        let key = format!(
            "{app}\n{title}\n{artist}\n{}",
            album.as_deref().unwrap_or_default()
        );
        if self.artwork.as_ref().is_none_or(|(k, _)| *k != key) {
            self.artwork = Some((key, artwork(&properties)));
        }
        Some(MediaState {
            title,
            artist,
            album,
            app,
            playing,
            position,
            duration,
            artwork: self.artwork.as_ref().and_then(|(_, art)| art.clone()),
        })
    }
}

fn to_json(state: &Option<MediaState>) -> Value {
    serde_json::to_value(state).unwrap_or(Value::Null)
}

/// Now as a Windows `DateTime` (100 ns ticks since 1601).
fn now_filetime() -> i64 {
    // SAFETY: no preconditions.
    let time = unsafe { GetSystemTimeAsFileTime() };
    ((u64::from(time.dwHighDateTime) << 32) | u64::from(time.dwLowDateTime)) as i64
}

/// The thumbnail as a `data:` URL, if there is a small enough one.
fn artwork(properties: &Properties) -> Option<String> {
    let stream = properties
        .Thumbnail()
        .ok()?
        .OpenReadAsync()
        .ok()?
        .join()
        .ok()?;
    let size = stream.Size().ok()?;
    if size == 0 || size > MAX_ARTWORK {
        return None;
    }
    let kind = stream
        .ContentType()
        .map(|s| s.to_string())
        .ok()
        .filter(|t| t.starts_with("image/"));
    let reader = DataReader::CreateDataReader(&stream).ok()?;
    let loaded = reader.LoadAsync(size as u32).ok()?.join().ok()?;
    let mut bytes = vec![0u8; loaded as usize];
    reader.ReadBytes(&mut bytes).ok()?;
    Some(format!(
        "data:{};base64,{}",
        kind.as_deref().unwrap_or("image/png"),
        base64(&bytes)
    ))
}

/// A readable app name from an AppUserModelID: `Spotify.exe` → `Spotify`,
/// `Microsoft.ZuneMusic_8wekyb3d8bbwe!Microsoft.ZuneMusic` → `ZuneMusic`.
fn app_name(aumid: &str) -> String {
    let name = aumid.rsplit('!').next().unwrap_or(aumid);
    let name = match name.len().checked_sub(4) {
        Some(at) if name[at..].eq_ignore_ascii_case(".exe") => &name[..at],
        _ => name,
    };
    name.rsplit('.').next().unwrap_or(name).to_owned()
}

/// Whether subscribers need this state: anything changed but the position,
/// or the position jumped (a seek) rather than just advanced.
fn worth_sending(
    sent: Option<&(Option<MediaState>, Instant)>,
    next: Option<&MediaState>,
    now: Instant,
) -> bool {
    let Some((sent, at)) = sent else { return true };
    match (sent.as_ref(), next) {
        (None, None) => false,
        (Some(before), Some(after)) => {
            let same = MediaState {
                position: after.position,
                ..before.clone()
            } == *after;
            if !same {
                return true;
            }
            let elapsed = if before.playing {
                (now - *at).as_secs_f64()
            } else {
                0.0
            };
            match (before.position, after.position) {
                (Some(p0), Some(p1)) => (p1 - (p0 + elapsed)).abs() > SEEK,
                (a, b) => a.is_some() != b.is_some(),
            }
        }
        _ => true,
    }
}

fn base64(bytes: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b = [
            chunk[0],
            *chunk.get(1).unwrap_or(&0),
            *chunk.get(2).unwrap_or(&0),
        ];
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(TABLE[(n >> (18 - 6 * i) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_matches_rfc_4648() {
        for (input, encoded) in [
            ("", ""),
            ("f", "Zg=="),
            ("fo", "Zm8="),
            ("foo", "Zm9v"),
            ("foobar", "Zm9vYmFy"),
        ] {
            assert_eq!(base64(input.as_bytes()), encoded);
        }
    }

    #[test]
    fn app_names_are_readable() {
        assert_eq!(app_name("Spotify.exe"), "Spotify");
        assert_eq!(
            app_name("Microsoft.ZuneMusic_8wekyb3d8bbwe!Microsoft.ZuneMusic"),
            "ZuneMusic"
        );
        assert_eq!(app_name("MSEdge"), "MSEdge");
        assert_eq!(app_name("FIREFOX.EXE"), "FIREFOX");
    }

    fn song(position: f64, playing: bool) -> MediaState {
        MediaState {
            title: "Song".into(),
            artist: "Band".into(),
            album: None,
            app: "Spotify".into(),
            playing,
            position: Some(position),
            duration: Some(200.0),
            artwork: None,
        }
    }

    #[test]
    fn only_real_changes_and_seeks_are_sent() {
        let t = Instant::now();
        let sent = (Some(song(10.0, true)), t);
        let later = t + Duration::from_secs(5);
        assert!(
            !worth_sending(Some(&sent), Some(&song(15.2, true)), later),
            "just playing on"
        );
        assert!(
            worth_sending(Some(&sent), Some(&song(90.0, true)), later),
            "a seek"
        );
        assert!(
            worth_sending(Some(&sent), Some(&song(15.0, false)), later),
            "paused"
        );
        assert!(worth_sending(Some(&sent), None, later), "stopped");
        assert!(
            worth_sending(None, None, later),
            "the first state always goes out"
        );
        assert!(!worth_sending(Some(&(None, t)), None, later));
    }
}
