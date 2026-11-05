//! Discord widget state combines existing Windows notification demand with optional RPC/bot data.
use crate::widgets::model::Model as WidgetModel;
use mypaper_integrations::{Action, Data, Lease, Provider, notifications};
use mypaper_ui::HitId;
pub const MUTE: HitId = 7300;
pub const DEAFEN: HitId = 7301;
pub const SEND: HitId = 7302;
pub const BOT: HitId = 7303;
pub const VOICE: HitId = 7304;
pub const MESSAGES: HitId = 7305;
pub const SETUP: HitId = 7306;
pub const CHANNEL: HitId = 7350;
#[derive(Default)]
pub struct Model {
    pub(super) notifications: Option<Lease>,
    pub cards: Vec<notifications::Card>,
    pub voice: bool,
    pub setup: bool,
    pub channel: usize,
    pub reply_to: Option<String>,
    pub channels: Vec<String>,
    pub can_send: bool,
    pub notifications_enabled: bool,
    checked_generation: Option<u64>,
    task: Option<super::Task<()>>,
    sent_text: Option<String>,
    pub notice: Option<String>,
}
impl Model {
    pub fn busy(&self) -> bool {
        self.task.is_some()
    }
}
pub fn sync(widget: &mut WidgetModel) {
    let configured_id = widget.str("appId", "com.squirrel.Discord.Discord");
    let state = &mut widget.integration;
    let config = super::config();
    if state.discord.checked_generation != Some(super::generation()) {
        state.discord.channels = config.discord.channel_ids.clone();
        state.discord.can_send = !state.discord.channels.is_empty()
            && mypaper_integrations::credentials::read("discord/bot").is_ok();
        state.discord.notifications_enabled = config.notifications.enabled;
        state.discord.checked_generation = Some(super::generation());
    }
    if state.discord.notifications.is_none() && config.notifications.enabled {
        state.discord.notifications = super::acquire(Provider::Notifications).ok();
    }
    if let Some(lease) = &state.discord.notifications {
        let snapshot = lease.snapshot();
        if let Some(Data::Notifications(notifications)) = snapshot.data {
            let cards = notifications
                .cards
                .into_iter()
                .filter(|card| {
                    mypaper_integrations::discord::is_discord_card(card)
                        || card.app_id.eq_ignore_ascii_case(&configured_id)
                })
                .collect::<Vec<_>>();
            if state.discord.cards != cards {
                state.discord.cards = cards;
                widget.dirty = true;
            }
        }
    }
    if let Some(result) = state.discord.task.as_ref().and_then(super::Task::take) {
        state.discord.notice = match result {
            Ok(()) => {
                if let Some(sent) = state.discord.sent_text.take() {
                    if let Some(field) = &widget.field
                        && field.text().is_ok_and(|text| text == sent)
                    {
                        let _ = field.set_text("");
                    }
                    state.discord.reply_to = None;
                    Some("Sent as your bot".into())
                } else {
                    None
                }
            }
            Err(e) => {
                state.discord.sent_text = None;
                Some(e)
            }
        };
        state.discord.task = None;
        if let Some(lease) = &state.lease {
            lease.retry();
        } else {
            state.generation = None;
        }
        widget.dirty = true;
    }
}
fn open(url: &str) -> anyhow::Result<()> {
    mypaper_integrations::launcher::open(&mypaper_integrations::launcher::Recent {
        title: String::new(),
        target: url.into(),
        arguments: String::new(),
    })
}
pub fn action(widget: &mut WidgetModel, hit: HitId) -> bool {
    use super::widgets::{CONNECT, ITEM, PRIMARY, REFRESH};
    let state = &mut widget.integration;
    state.discord.notice = None;
    let config = super::config();
    let result = (|| -> anyhow::Result<()> {
        match hit {
            CONNECT => {
                let mut settings = super::load()?;
                settings.discord.enabled = true;
                settings.notifications.enabled = true;
                settings.notifications.discord_previews = true;
                mypaper_widgets::files::write_atomic(
                    &super::config_path(),
                    &serde_json::to_vec_pretty(&settings)?,
                )?;
                state.generation = None;
            }
            REFRESH => {
                if let Some(lease) = &state.lease {
                    lease.retry();
                } else {
                    state.generation = None;
                }
                if let Some(lease) = &state.discord.notifications {
                    lease.retry();
                }
            }
            PRIMARY => open("https://discord.com/channels/@me")?,
            VOICE => state.discord.voice = true,
            MESSAGES => state.discord.voice = false,
            SETUP => state.discord.setup = !state.discord.setup,
            BOT if !state.discord.busy() => {
                let id = config.discord.client_id.clone();
                state.discord.task = Some(super::Task::spawn(move || {
                    let id = if id.is_empty()
                        || mypaper_integrations::credentials::read("discord/bot").is_err()
                    {
                        let Some(id) = mypaper_integrations::discord::configure_bot(&id)? else {
                            return Ok(());
                        };
                        id
                    } else {
                        id
                    };
                    let mut config = super::load()?;
                    config.discord.client_id = id.clone();
                    config.discord.enabled = true;
                    mypaper_widgets::files::write_atomic(
                        &super::config_path(),
                        &serde_json::to_vec_pretty(&config)?,
                    )?;
                    open(&mypaper_integrations::discord::invite_url(&id)?)
                }));
            }
            MUTE | DEAFEN => {
                let call = match &state.snapshot.data {
                    Some(Data::Discord(s)) => s.call.as_ref(),
                    _ => None,
                }
                .ok_or_else(|| anyhow::anyhow!("No active Discord call"))?;
                state
                    .lease
                    .as_ref()
                    .ok_or_else(|| anyhow::anyhow!("Discord is disconnected"))?
                    .action(Action::DiscordVoice {
                        muted: (hit == MUTE).then_some(!call.muted),
                        deafened: (hit == DEAFEN).then_some(!call.deafened),
                    })?;
            }
            SEND if !state.discord.busy() => {
                let channel = config
                    .discord
                    .channel_ids
                    .get(state.discord.channel)
                    .cloned()
                    .ok_or_else(|| {
                        anyhow::anyhow!("Choose a bot channel in integrations.json first")
                    })?;
                let text = widget
                    .field
                    .as_ref()
                    .ok_or_else(|| anyhow::anyhow!("Enter a message first"))?
                    .text()?;
                let reply = state.discord.reply_to.clone();
                state.discord.sent_text = Some(text.clone());
                state.discord.task = Some(super::Task::spawn(move || {
                    mypaper_integrations::discord::send(&channel, &text, reply.as_deref())
                }));
            }
            hit if (CHANNEL..CHANNEL + 8).contains(&hit) => {
                state.discord.channel = (hit - CHANNEL) as usize;
                state.discord.reply_to = None;
            }
            hit if (ITEM..ITEM + 512).contains(&hit) => {
                let index = (hit - ITEM) as usize;
                if let Some(Data::Discord(s)) = &state.snapshot.data
                    && let Some(message) = s.messages.get(index)
                {
                    if let Some(channel) = config
                        .discord
                        .channel_ids
                        .iter()
                        .position(|id| id == &message.channel_id)
                    {
                        state.discord.channel = channel;
                        state.discord.reply_to = Some(message.id.clone());
                    }
                } else {
                    open("https://discord.com/channels/@me")?;
                }
            }
            hit if hit == super::widgets::SECONDARY && !state.discord.busy() => {
                let mut discord = config.discord.clone();
                discord.rpc = true;
                state.discord.task = Some(super::Task::spawn(move || {
                    let Some(id) =
                        mypaper_integrations::discord::configure_voice(&discord.client_id)?
                    else {
                        return Ok(());
                    };
                    discord.client_id = id.clone();
                    let mut config = super::load()?;
                    config.discord.client_id = id;
                    mypaper_widgets::files::write_atomic(
                        &super::config_path(),
                        &serde_json::to_vec_pretty(&config)?,
                    )?;
                    mypaper_integrations::discord::connect(&discord)?;
                    let mut config = super::load()?;
                    config.discord.enabled = true;
                    config.discord.rpc = true;
                    mypaper_widgets::files::write_atomic(
                        &super::config_path(),
                        &serde_json::to_vec_pretty(&config)?,
                    )
                }));
            }
            _ => {}
        }
        Ok(())
    })();
    if let Err(e) = result {
        state.discord.notice = Some(e.to_string());
    }
    true
}
