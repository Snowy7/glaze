//! A shared SMTC manager, selected-session events, and advertised transport capabilities.
use crate::{Availability, Data, Publish};
use anyhow::{Result, ensure};
use base64::{Engine, engine::general_purpose::STANDARD};
use std::sync::Arc;
use tokio::sync::{Notify, mpsc};
use windows::{
    Foundation::TypedEventHandler,
    Media::Control::{
        GlobalSystemMediaTransportControlsSession as Session,
        GlobalSystemMediaTransportControlsSessionManager as Manager,
        GlobalSystemMediaTransportControlsSessionPlaybackStatus as Status,
    },
    Win32::System::Com::{COINIT_MULTITHREADED, CoInitializeEx, CoUninitialize},
};

pub(crate) struct Com;
impl Com {
    pub fn new() -> Result<Self> {
        // SAFETY: this provider owns its worker thread; balanced in Drop even on cancellation.
        unsafe {
            CoInitializeEx(None, COINIT_MULTITHREADED).ok()?;
        }
        Ok(Self)
    }
}
impl Drop for Com {
    fn drop(&mut self) {
        // SAFETY: balances this thread's successful initialization.
        unsafe {
            CoUninitialize();
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Snapshot {
    pub title: String,
    pub artist: String,
    pub album: String,
    pub app_id: String,
    pub playing: bool,
    pub toggle: bool,
    pub previous: bool,
    pub next: bool,
    pub artwork: Option<String>,
    /// Session-relative timeline in milliseconds; absent for a live/unsupported timeline.
    pub position_ms: Option<i64>,
    pub duration_ms: Option<i64>,
    /// Unix milliseconds at which `position_ms` was reported by SMTC.
    pub position_at_ms: Option<i64>,
}
#[derive(Clone, Copy, Debug)]
pub enum Action {
    Toggle,
    Play,
    Pause,
    Previous,
    Next,
}
fn notify<S: windows::core::RuntimeType + 'static, A: windows::core::RuntimeType + 'static>(
    changed: &Arc<Notify>,
    events: &Publish,
) -> TypedEventHandler<S, A> {
    let changed = changed.clone();
    let events = events.clone();
    TypedEventHandler::new(move |_, _| {
        events.delivered();
        changed.notify_one();
        Ok(())
    })
}
struct Subscription {
    manager: Manager,
    manager_tokens: [i64; 2],
    session: Option<(Session, [i64; 3])>,
    artwork: Option<(String, Option<String>)>,
}
impl Subscription {
    fn follow(
        &mut self,
        changed: &Arc<Notify>,
        timeline: &Arc<Notify>,
        publish: &Publish,
    ) -> Result<()> {
        let next = self.manager.GetCurrentSession().ok();
        if next.as_ref() == self.session.as_ref().map(|(s, _)| s) {
            return Ok(());
        }
        self.unfollow();
        if let Some(session) = next {
            let a = session.MediaPropertiesChanged(&notify(changed, publish))?;
            let b = match session.PlaybackInfoChanged(&notify(changed, publish)) {
                Ok(b) => b,
                Err(e) => {
                    let _ = session.RemoveMediaPropertiesChanged(a);
                    return Err(e.into());
                }
            };
            let c = match session.TimelinePropertiesChanged(&notify(timeline, publish)) {
                Ok(c) => c,
                Err(e) => {
                    let _ = session.RemoveMediaPropertiesChanged(a);
                    let _ = session.RemovePlaybackInfoChanged(b);
                    return Err(e.into());
                }
            };
            self.session = Some((session, [a, b, c]));
        }
        Ok(())
    }
    fn unfollow(&mut self) {
        if let Some((session, [a, b, c])) = self.session.take() {
            let _ = session.RemoveMediaPropertiesChanged(a);
            let _ = session.RemovePlaybackInfoChanged(b);
            let _ = session.RemoveTimelinePropertiesChanged(c);
        }
    }
    fn read(&mut self) -> Result<Snapshot> {
        let Some((session, _)) = &self.session else {
            return Ok(Snapshot::default());
        };
        let props = session.TryGetMediaPropertiesAsync()?.join()?;
        let playback = session.GetPlaybackInfo()?;
        let controls = playback.Controls()?;
        let key = format!(
            "{}\n{}\n{}\n{}",
            session.SourceAppUserModelId()?,
            props.Title()?,
            props.Artist()?,
            props.AlbumTitle()?
        );
        if self.artwork.as_ref().is_none_or(|(old, _)| old != &key) {
            let image = (|| -> Option<String> {
                let stream = props.Thumbnail().ok()?.OpenReadAsync().ok()?.join().ok()?;
                let size = stream.Size().ok()?;
                if size == 0 || size > 256 * 1024 {
                    return None;
                }
                let reader =
                    windows::Storage::Streams::DataReader::CreateDataReader(&stream).ok()?;
                let size = reader.LoadAsync(size as u32).ok()?.join().ok()?;
                let mut bytes = vec![0; size as usize];
                reader.ReadBytes(&mut bytes).ok()?;
                Some(format!(
                    "data:{};base64,{}",
                    stream.ContentType().ok()?,
                    STANDARD.encode(bytes)
                ))
            })();
            self.artwork = Some((key, image));
        }
        let mut snapshot = Snapshot {
            title: props.Title()?.to_string(),
            artist: props.Artist()?.to_string(),
            album: props.AlbumTitle()?.to_string(),
            app_id: session.SourceAppUserModelId()?.to_string(),
            playing: playback.PlaybackStatus()? == Status::Playing,
            toggle: controls.IsPlayPauseToggleEnabled()?,
            previous: controls.IsPreviousEnabled()?,
            next: controls.IsNextEnabled()?,
            artwork: self.artwork.as_ref().and_then(|(_, image)| image.clone()),
            ..Snapshot::default()
        };
        self.read_timeline(&mut snapshot);
        Ok(snapshot)
    }
    /// Seek/timeline notifications do not reread metadata or reopen the thumbnail stream.
    fn read_timeline(&self, snapshot: &mut Snapshot) {
        let timeline = self
            .session
            .as_ref()
            .and_then(|(session, _)| session.GetTimelineProperties().ok())
            .and_then(|timeline| {
                let start = timeline.StartTime().ok()?.Duration;
                let duration = timeline.EndTime().ok()?.Duration - start;
                if duration <= 0 {
                    return None;
                }
                Some((
                    (timeline.Position().ok()?.Duration - start).clamp(0, duration) / 10_000,
                    duration / 10_000,
                    timeline
                        .LastUpdatedTime()
                        .ok()
                        .map(|at| at.UniversalTime / 10_000 - 11_644_473_600_000),
                ))
            });
        snapshot.position_ms = timeline.as_ref().map(|(position, _, _)| *position);
        snapshot.duration_ms = timeline.as_ref().map(|(_, duration, _)| *duration);
        snapshot.position_at_ms = timeline.and_then(|(_, _, at)| at);
    }
    fn control(&self, action: Action) -> Result<()> {
        let Some((session, _)) = &self.session else {
            anyhow::bail!("nothing is playing");
        };
        let controls = session.GetPlaybackInfo()?.Controls()?;
        let operation = match action {
            Action::Toggle => {
                ensure!(
                    controls.IsPlayPauseToggleEnabled()?,
                    "player does not support play/pause"
                );
                session.TryTogglePlayPauseAsync()?
            }
            Action::Play => session.TryPlayAsync()?,
            Action::Pause => session.TryPauseAsync()?,
            Action::Next => {
                ensure!(controls.IsNextEnabled()?, "player does not support next");
                session.TrySkipNextAsync()?
            }
            Action::Previous => {
                ensure!(
                    controls.IsPreviousEnabled()?,
                    "player does not support previous"
                );
                session.TrySkipPreviousAsync()?
            }
        };
        ensure!(operation.join()?, "player refused the command");
        Ok(())
    }
}
impl Drop for Subscription {
    fn drop(&mut self) {
        self.unfollow();
        let _ = self
            .manager
            .RemoveCurrentSessionChanged(self.manager_tokens[0]);
        let _ = self.manager.RemoveSessionsChanged(self.manager_tokens[1]);
    }
}

pub(crate) async fn run(
    publish: &Publish,
    commands: &mut mpsc::Receiver<crate::Action>,
) -> Result<()> {
    let _com = Com::new()?;
    let manager = Manager::RequestAsync()?.join()?;
    let changed = Arc::new(Notify::new());
    let timeline = Arc::new(Notify::new());
    let a = manager.CurrentSessionChanged(&notify(&changed, publish))?;
    let b = match manager.SessionsChanged(&notify(&changed, publish)) {
        Ok(b) => b,
        Err(e) => {
            let _ = manager.RemoveCurrentSessionChanged(a);
            return Err(e.into());
        }
    };
    let mut subscription = Subscription {
        manager,
        manager_tokens: [a, b],
        session: None,
        artwork: None,
    };
    subscription.follow(&changed, &timeline, publish)?;
    let mut snapshot = subscription.read()?;
    publish.set(Availability::Live, Some(Data::Media(snapshot.clone())));
    loop {
        tokio::select! {
            _ = changed.notified() => {
                subscription.follow(&changed, &timeline, publish)?;
                snapshot = subscription.read()?;
                publish.set(Availability::Live, Some(Data::Media(snapshot.clone())));
            }
            _ = timeline.notified() => {
                subscription.read_timeline(&mut snapshot);
                publish.set(Availability::Live, Some(Data::Media(snapshot.clone())));
            }
            Some(action) = commands.recv() => if let crate::Action::Media(action) = action { publish.action_result(subscription.control(action)); }
        }
    }
}
