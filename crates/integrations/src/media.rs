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
    session: Option<(Session, [i64; 2])>,
    artwork: Option<(String, Option<String>)>,
}
impl Subscription {
    fn follow(&mut self, changed: &Arc<Notify>, publish: &Publish) -> Result<()> {
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
            self.session = Some((session, [a, b]));
        }
        Ok(())
    }
    fn unfollow(&mut self) {
        if let Some((session, [a, b])) = self.session.take() {
            let _ = session.RemoveMediaPropertiesChanged(a);
            let _ = session.RemovePlaybackInfoChanged(b);
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
            "{}\n{}\n{}",
            session.SourceAppUserModelId()?,
            props.Title()?,
            props.Artist()?
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
        Ok(Snapshot {
            title: props.Title()?.to_string(),
            artist: props.Artist()?.to_string(),
            album: props.AlbumTitle()?.to_string(),
            app_id: session.SourceAppUserModelId()?.to_string(),
            playing: playback.PlaybackStatus()? == Status::Playing,
            toggle: controls.IsPlayPauseToggleEnabled()?,
            previous: controls.IsPreviousEnabled()?,
            next: controls.IsNextEnabled()?,
            artwork: self.artwork.as_ref().and_then(|(_, image)| image.clone()),
        })
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
    subscription.follow(&changed, publish)?;
    publish.set(Availability::Live, Some(Data::Media(subscription.read()?)));
    loop {
        tokio::select! {
            _ = changed.notified() => { subscription.follow(&changed,publish)?; publish.set(Availability::Live, Some(Data::Media(subscription.read()?))); }
            Some(action) = commands.recv() => if let crate::Action::Media(action) = action { publish.action_result(subscription.control(action)); }
        }
    }
}
