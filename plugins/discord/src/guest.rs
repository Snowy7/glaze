use crate::{
    faces,
    model::{self, Call, Config, Decoder, Message, Saved},
};
use glaze_plugin_sdk::guest::{
    self, Event, FaceOpen, Plugin, SizeClass, events, http, pipes, secrets, timers, ui,
};
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Clone)]
enum Request {
    ValidateBot,
    History(String),
    Send(String),
    Exchange,
}
#[derive(Clone, PartialEq)]
enum Rpc {
    Authorize,
    Authenticate,
    Channel,
    Settings,
    Control,
    Subscribe,
}
#[derive(Default)]
struct Discord {
    config: Config,
    saved: Saved,
    faces: BTreeMap<String, SizeClass>,
    pipe: Option<u64>,
    decoder: Decoder,
    nonce: u64,
    rpc: BTreeMap<String, Rpc>,
    requests: BTreeMap<u64, Request>,
    bot_verified: bool,
    call: Option<Call>,
    channel: Value,
    voice_settings: Value,
    can_control: bool,
    connected: bool,
    authorizing: bool,
    messages: Vec<Message>,
    notifications: Vec<Message>,
    notice: Option<String>,
    voice_status: String,
}
fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
fn secret(key: &str) -> Result<String, String> {
    secrets::get(key)
        .map_err(|_| "Credential broker unavailable".to_string())?
        .filter(|s| !s.is_empty())
        .ok_or_else(|| format!("Store your Discord {key} in setup first"))
}
impl Discord {
    fn draw(&self) {
        for (instance, size) in &self.faces {
            let choice = self.saved.faces.get(instance).cloned().unwrap_or_default();
            let messages = if self.bot_verified {
                &self.messages
            } else {
                &self.notifications
            };
            let f = faces::face(
                faces::Context {
                    choice: &choice,
                    call: self.call.as_ref(),
                    messages,
                    channels: &self.config.channels,
                    bot: self.bot_verified,
                    connected: self.connected,
                    busy: self
                        .requests
                        .values()
                        .any(|r| matches!(r, Request::Send(_))),
                    notice: self.notice.as_deref(),
                    voice_status: &self.voice_status,
                },
                matches!(size, SizeClass::Small),
            );
            ui::set_view(instance, f);
        }
    }
    fn open(&mut self, open: FaceOpen) -> Result<(), String> {
        let config = Config::parse(&guest::settings(&open))?;
        let changed = config != self.config;
        let first = self.faces.is_empty();
        self.faces.insert(open.instance.clone(), open.size);
        let choice = self.saved.faces.entry(open.instance).or_default();
        if !config.channels.contains(&choice.channel) {
            choice.channel = config.channels.first().cloned().unwrap_or_default();
            choice.reply = None;
        }
        if changed {
            self.disconnect();
            self.requests.clear();
            self.messages.clear();
            self.bot_verified = false;
            self.config = config;
        }
        if first || changed {
            self.refresh_bot()?;
            if self.config.rpc {
                self.connect(false)?;
            }
        }
        Ok(())
    }
    fn fetch(
        &mut self,
        kind: Request,
        method: http::Method,
        url: String,
        headers: Vec<http::Header>,
        body: Option<Vec<u8>>,
    ) -> Result<(), String> {
        if self.requests.len() >= 16 {
            return Err("Discord is busy; try again shortly".into());
        }
        let id = http::fetch(&http::Request {
            method,
            url,
            headers,
            body,
            timeout_ms: Some(15000),
        })
        .map_err(|_| "Discord request could not start".to_string())?;
        self.requests.insert(id, kind);
        Ok(())
    }
    fn bot_request(
        &mut self,
        kind: Request,
        method: http::Method,
        path: &str,
        body: Option<Value>,
    ) -> Result<(), String> {
        let token = secret("bot")?;
        let headers = vec![
            http::Header {
                name: "Authorization".into(),
                value: format!("Bot {token}"),
            },
            http::Header {
                name: "Content-Type".into(),
                value: "application/json".into(),
            },
        ];
        self.fetch(
            kind,
            method,
            format!("https://discord.com/api/v10/{path}"),
            headers,
            body.map(|b| b.to_string().into_bytes()),
        )
    }
    fn refresh_bot(&mut self) -> Result<(), String> {
        if self.config.channels.is_empty() || secrets::get("bot").ok().flatten().is_none() {
            return Ok(());
        }
        if self
            .requests
            .values()
            .any(|r| matches!(r, Request::ValidateBot | Request::History(_)))
        {
            return Ok(());
        }
        self.bot_verified = false;
        self.bot_request(Request::ValidateBot, http::Method::Get, "users/@me", None)
    }
    fn disconnect(&mut self) {
        if let Some(id) = self.pipe.take() {
            pipes::close(id);
        }
        for nonce in self.rpc.keys() {
            timers::cancel(&format!("rpc:{nonce}"));
        }
        self.rpc.clear();
        self.decoder = Decoder::default();
        self.call = None;
        self.channel = Value::Null;
        self.voice_settings = Value::Null;
        self.connected = false;
        self.can_control = false;
        timers::cancel("ready");
        timers::cancel("renew");
    }
    fn connect(&mut self, authorize: bool) -> Result<(), String> {
        model::snowflake(&self.config.client_id).map_err(|_| {
            "Set your registered Discord application ID in widget settings".to_string()
        })?;
        if authorize {
            secret("client-secret")?;
        }
        self.disconnect();
        self.authorizing = authorize;
        self.voice_status = "Connecting to Discord RPC…".into();
        let pipe = pipes::open("discord-ipc-0")
            .map_err(|_| "Discord IPC unavailable; open Discord and refresh".to_string())?;
        self.pipe = Some(pipe);
        self.write(
            0,
            &serde_json::to_vec(&json!({"v":1,"client_id":self.config.client_id})).unwrap(),
        )?;
        timers::after("ready", 5000);
        Ok(())
    }
    fn write(&self, op: u32, payload: &[u8]) -> Result<(), String> {
        let id = self.pipe.ok_or("Discord voice is disconnected")?;
        pipes::write(id, &model::frame(op, payload)?)
            .map_err(|_| "Discord pipe write failed".into())
    }
    fn request(
        &mut self,
        kind: Rpc,
        command: &str,
        args: Value,
        event: Option<&str>,
    ) -> Result<(), String> {
        if self.rpc.len() >= 32 {
            return Err("Discord RPC is busy".into());
        }
        self.nonce += 1;
        let nonce = self.nonce.to_string();
        let mut payload = json!({"cmd":command,"args":args,"nonce":nonce});
        if let Some(event) = event {
            payload["evt"] = json!(event);
        }
        self.write(1, &serde_json::to_vec(&payload).unwrap())?;
        timers::after(
            &format!("rpc:{nonce}"),
            if kind == Rpc::Authorize {
                180000
            } else {
                10000
            },
        );
        self.rpc.insert(nonce, kind);
        Ok(())
    }
    fn exchange(&mut self, code: Option<&str>) -> Result<(), String> {
        let client_secret = secret("client-secret")?;
        let refresh = if code.is_none() {
            Some(secret("rpc-refresh")?)
        } else {
            None
        };
        let mut fields = vec![
            ("client_id", self.config.client_id.as_str()),
            ("client_secret", client_secret.as_str()),
        ];
        if let Some(code) = code {
            fields.extend([("grant_type", "authorization_code"), ("code", code)]);
        }
        if let Some(refresh) = &refresh {
            fields.extend([
                ("grant_type", "refresh_token"),
                ("refresh_token", refresh.as_str()),
            ]);
        }
        self.fetch(
            Request::Exchange,
            http::Method::Post,
            "https://discord.com/api/oauth2/token".into(),
            vec![http::Header {
                name: "Content-Type".into(),
                value: "application/x-www-form-urlencoded".into(),
            }],
            Some(model::form(&fields)),
        )
    }
    fn authenticate(&mut self) -> Result<(), String> {
        self.request(
            Rpc::Authenticate,
            "AUTHENTICATE",
            json!({"access_token":secret("rpc-access")?}),
            None,
        )
    }
    fn subscribe(&mut self, event: &str, channel: Option<&str>, on: bool) -> Result<(), String> {
        self.request(
            Rpc::Subscribe,
            if on { "SUBSCRIBE" } else { "UNSUBSCRIBE" },
            channel.map_or(json!({}), |id| json!({"channel_id":id})),
            Some(event),
        )
    }
    fn refresh_call(&mut self) -> Result<(), String> {
        if !self.connected {
            return Ok(());
        }
        if !self
            .rpc
            .values()
            .any(|r| *r == Rpc::Channel || *r == Rpc::Settings)
        {
            self.request(Rpc::Channel, "GET_SELECTED_VOICE_CHANNEL", json!({}), None)?;
            self.request(Rpc::Settings, "GET_VOICE_SETTINGS", json!({}), None)?;
        }
        Ok(())
    }
    fn update_call(&mut self) -> Result<(), String> {
        let call = model::parse_call(
            &self.channel,
            &self.voice_settings,
            self.can_control,
            self.call.as_ref(),
        );
        let old = self.call.as_ref().map(|c| c.channel_id.clone());
        let new = call.as_ref().map(|c| c.channel_id.clone());
        if old != new {
            for event in [
                "VOICE_STATE_CREATE",
                "VOICE_STATE_UPDATE",
                "VOICE_STATE_DELETE",
                "SPEAKING_START",
                "SPEAKING_STOP",
            ] {
                if let Some(old) = &old {
                    self.subscribe(event, Some(old), false)?;
                }
                if let Some(new) = &new {
                    self.subscribe(event, Some(new), true)?;
                }
            }
        }
        self.call = call;
        Ok(())
    }
    fn rpc_frame(&mut self, value: Value) -> Result<(), String> {
        if value["evt"] == "READY" {
            timers::cancel("ready");
            if self.authorizing {
                let mut scopes = vec!["rpc", "rpc.voice.read"];
                if self.config.voice_controls {
                    scopes.push("rpc.voice.write");
                }
                return self.request(
                    Rpc::Authorize,
                    "AUTHORIZE",
                    json!({"client_id":self.config.client_id,"scopes":scopes}),
                    None,
                );
            }
            return if self.saved.expiry <= now() + 60 {
                self.exchange(None)
            } else {
                self.authenticate()
            };
        }
        if let Some(nonce) = value["nonce"].as_str() {
            if let Some(kind) = self.rpc.remove(nonce) {
                timers::cancel(&format!("rpc:{nonce}"));
                if value["evt"] == "ERROR" {
                    return Err(format!(
                        "Discord rejected RPC (code {}); check tester access and approved scopes",
                        value["data"]["code"]
                    ));
                }
                let data = &value["data"];
                match kind {
                    Rpc::Authorize => self.exchange(Some(
                        data["code"]
                            .as_str()
                            .ok_or("Discord did not issue an authorization code")?,
                    ))?,
                    Rpc::Authenticate => {
                        self.connected = true;
                        self.authorizing = false;
                        self.voice_status = "Connected to Discord RPC".into();
                        self.can_control = self.config.voice_controls
                            && data["scopes"].as_array().is_some_and(|a| {
                                a.iter().any(|s| s == "rpc.voice.write" || s == "rpc")
                            });
                        for event in ["VOICE_CHANNEL_SELECT", "VOICE_SETTINGS_UPDATE"] {
                            self.subscribe(event, None, true)?;
                        }
                        let delay = self.saved.expiry.saturating_sub(now() + 60).clamp(1, 3600)
                            as u32
                            * 1000;
                        timers::after("renew", delay);
                        self.refresh_call()?;
                    }
                    Rpc::Channel => {
                        self.channel = data.clone();
                        self.update_call()?;
                    }
                    Rpc::Settings => {
                        self.voice_settings = data.clone();
                        self.update_call()?;
                    }
                    Rpc::Control => {
                        self.refresh_call()?;
                    }
                    Rpc::Subscribe => {}
                }
            }
            return Ok(());
        }
        match value["evt"].as_str().unwrap_or("") {
            "SPEAKING_START" | "SPEAKING_STOP" => {
                if let Some(call) = &mut self.call {
                    for p in &mut call.participants {
                        if p.id == value["data"]["user_id"] {
                            p.speaking = value["evt"] == "SPEAKING_START";
                        }
                    }
                }
            }
            "VOICE_CHANNEL_SELECT"
            | "VOICE_STATE_CREATE"
            | "VOICE_STATE_UPDATE"
            | "VOICE_STATE_DELETE"
            | "VOICE_SETTINGS_UPDATE" => self.refresh_call()?,
            _ => {}
        }
        Ok(())
    }
    fn pipe_event(&mut self, event: pipes::PipeEvent) -> Result<(), String> {
        match event {
            pipes::PipeEvent::Data((id, bytes)) if self.pipe == Some(id) => {
                for (op, payload) in self.decoder.push(&bytes)? {
                    match op {
                        1 => self.rpc_frame(
                            serde_json::from_slice(&payload)
                                .map_err(|_| "Invalid Discord RPC JSON")?,
                        )?,
                        2 => {
                            self.disconnect();
                            return Err(
                                "Discord closed RPC; check registered app and tester access".into(),
                            );
                        }
                        3 => self.write(4, &payload)?,
                        4 => {}
                        _ => return Err("Unsupported Discord IPC frame".into()),
                    }
                }
            }
            pipes::PipeEvent::Closed(id) if self.pipe == Some(id) => {
                self.disconnect();
                return Err("Discord IPC disconnected; refresh to reconnect".into());
            }
            _ => {}
        }
        Ok(())
    }
    fn http_reply(&mut self, reply: http::Reply) -> Result<(), String> {
        let Some(kind) = self.requests.remove(&reply.id) else {
            return Ok(());
        };
        let response = reply
            .outcome
            .map_err(|_| "Discord network request failed".to_string())?;
        if !(200..300).contains(&response.status) {
            if matches!(kind, Request::ValidateBot) {
                self.bot_verified = false;
            }
            return Err(if response.status == 429 {
                "Discord rate limit; wait before refreshing".into()
            } else {
                format!(
                    "Discord HTTP {} · check bot membership, permissions or RPC credentials",
                    response.status
                )
            });
        }
        if response.body.len() > model::MAX_FRAME {
            return Err("Discord response exceeds 512 KiB".into());
        }
        let value: Value =
            serde_json::from_slice(&response.body).map_err(|_| "Discord returned invalid JSON")?;
        match kind {
            Request::ValidateBot => {
                if value["bot"].as_bool() != Some(true) {
                    return Err("Use a bot token; personal account tokens are not supported".into());
                }
                self.bot_verified = true;
                self.messages.clear();
                for channel in self.config.channels.clone() {
                    self.bot_request(
                        Request::History(channel.clone()),
                        http::Method::Get,
                        &format!("channels/{channel}/messages?limit=30"),
                        None,
                    )?;
                }
            }
            Request::History(channel) => {
                self.messages.retain(|m| m.channel_id != channel);
                self.messages.extend(model::messages(&channel, &value)?);
                self.messages.sort_by(|a, b| b.timestamp.cmp(&a.timestamp));
                self.messages.truncate(100);
            }
            Request::Send(instance) => {
                if let Some(choice) = self.saved.faces.get_mut(&instance) {
                    choice.reply = None;
                }
                self.notice = Some("Sent as bot".into());
                self.refresh_bot()?;
            }
            Request::Exchange => {
                let token = value["access_token"]
                    .as_str()
                    .ok_or("Discord did not issue an access token")?;
                secrets::set("rpc-access", token)
                    .map_err(|_| "Could not store RPC access token")?;
                if let Some(refresh) = value["refresh_token"].as_str() {
                    secrets::set("rpc-refresh", refresh)
                        .map_err(|_| "Could not store RPC refresh token")?;
                }
                self.saved.expiry =
                    now() + value["expires_in"].as_u64().unwrap_or(3600).min(30 * 86400);
                self.authenticate()?;
            }
        }
        Ok(())
    }
    fn action(&mut self, action: guest::Action) -> Result<(), String> {
        if !self.faces.contains_key(&action.instance) {
            return Ok(());
        }
        self.notice = None;
        let choice = self.saved.faces.entry(action.instance.clone()).or_default();
        match action.action.as_str() {
            "messages" => {
                choice.voice = false;
                choice.setup = false;
            }
            "voice" => {
                choice.voice = true;
                choice.setup = false;
            }
            "setup" => choice.setup = !choice.setup,
            "channel" => {
                let id = action.value.ok_or("Choose a channel")?;
                if !self.config.channels.contains(&id) {
                    return Err("Channel is not configured".into());
                }
                choice.channel = id;
                choice.reply = None;
            }
            "cancel-reply" => choice.reply = None,
            "send" => {
                if !self.bot_verified {
                    return Err("Verify a bot token first".into());
                }
                if self
                    .requests
                    .values()
                    .any(|r| matches!(r, Request::Send(_)))
                {
                    return Err("A message is already sending".into());
                }
                if !self.config.channels.contains(&choice.channel) {
                    return Err("Choose a configured channel".into());
                }
                let channel = choice.channel.clone();
                let body = model::send_body(
                    &channel,
                    action.value.as_deref().unwrap_or(""),
                    choice.reply.as_deref(),
                )?;
                self.bot_request(
                    Request::Send(action.instance),
                    http::Method::Post,
                    &format!("channels/{channel}/messages"),
                    Some(body),
                )?;
            }
            "bot-token" | "client-secret" => {
                let key = if action.action == "bot-token" {
                    "bot"
                } else {
                    "client-secret"
                };
                let token = action
                    .value
                    .filter(|s| !s.trim().is_empty())
                    .ok_or("No credential entered")?;
                secrets::set(key, token.trim())
                    .map_err(|_| "Could not store Discord credential")?;
                self.notice = Some("Credential stored in Windows Credential Manager".into());
                if key == "bot" {
                    self.requests.clear();
                    self.refresh_bot()?;
                }
            }
            "authorize" => {
                choice.setup = false;
                choice.voice = true;
                self.connect(true)?;
            }
            "forget" => {
                self.disconnect();
                self.requests.clear();
                self.bot_verified = false;
                self.messages.clear();
                self.saved.expiry = 0;
                for key in ["bot", "client-secret", "rpc-access", "rpc-refresh"] {
                    secrets::remove(key).map_err(|_| "Could not remove Discord credential")?;
                }
                self.notice = Some("Discord credentials removed".into());
            }
            "refresh" => {
                self.refresh_bot()?;
                if self.config.rpc || self.pipe.is_some() {
                    self.connect(false)?;
                }
            }
            "mute" | "deafen" => {
                let call = self
                    .call
                    .as_ref()
                    .filter(|c| c.can_control)
                    .ok_or("Discord did not grant voice controls")?;
                if self.rpc.values().any(|r| *r == Rpc::Control) {
                    return Err("Voice control is pending".into());
                }
                let args = if action.action == "mute" {
                    json!({"mute":!call.muted})
                } else {
                    json!({"deaf":!call.deafened})
                };
                self.request(Rpc::Control, "SET_VOICE_SETTINGS", args, None)?;
            }
            other => {
                if let Some(id) = other.strip_prefix("reply:") {
                    if !self
                        .messages
                        .iter()
                        .any(|m| m.id == id && m.channel_id == choice.channel)
                    {
                        return Err("Message is no longer in this channel".into());
                    }
                    choice.reply = Some(id.into());
                }
            }
        }
        Ok(())
    }
    fn notification(&mut self, payload: &str) {
        let Ok(value) = serde_json::from_str::<Value>(payload) else {
            return;
        };
        if value["kind"] == "removed" {
            let id = format!("toast:{}", value["id"]);
            self.notifications.retain(|m| m.id != id);
            return;
        }
        let single = if value["kind"] == "added" {
            Some(vec![value["card"].clone()])
        } else {
            None
        };
        let cards = single
            .as_ref()
            .or_else(|| value["cards"].as_array())
            .or_else(|| value.as_array());
        if let Some(cards) = cards {
            if single.is_none() {
                self.notifications.clear();
            }
            let incoming: Vec<_> = cards
                .iter()
                .filter(|c| {
                    let id = c["app_id"]
                        .as_str()
                        .or(c["appId"].as_str())
                        .unwrap_or("")
                        .to_ascii_lowercase();
                    matches!(
                        id.as_str(),
                        "com.squirrel.discord.discord"
                            | "com.squirrel.discordptb.discordptb"
                            | "com.squirrel.discordcanary.discordcanary"
                    ) || id.starts_with("discordinc.") && id.contains("!discord")
                })
                .take(30)
                .map(|c| Message {
                    id: format!("toast:{}", c["id"]),
                    author: model::text(c, "title"),
                    content: model::text(c, "body"),
                    ..Default::default()
                })
                .collect();
            for message in incoming {
                self.notifications.retain(|m| m.id != message.id);
                self.notifications.insert(0, message);
            }
            self.notifications.truncate(30);
        }
    }
}
impl Plugin for Discord {
    fn init(&mut self, _info: &guest::InitInfo, state: Option<&[u8]>) -> Result<(), String> {
        self.saved = state
            .and_then(|b| serde_json::from_slice(b).ok())
            .unwrap_or_default();
        self.voice_status = "Connect a registered Discord RPC app".into();
        let _ = events::subscribe(&[events::Topic::Notifications]);
        Ok(())
    }
    fn on_event(&mut self, event: Event) {
        let result = match event {
            Event::Open(open) | Event::Resize(open) => self.open(open),
            Event::Close(instance) => {
                self.faces.remove(&instance);
                if self.faces.is_empty() {
                    self.disconnect();
                }
                Ok(())
            }
            Event::Action(action) => self.action(action),
            Event::Http(reply) => self.http_reply(reply),
            Event::Pipe(event) => {
                let result = self.pipe_event(event);
                if result.is_err() {
                    self.disconnect();
                }
                result
            }
            Event::Notification(payload) => {
                self.notification(&payload);
                Ok(())
            }
            Event::Timer(id) if id == "ready" || id.starts_with("rpc:") => {
                let pending = id == "ready" && self.pipe.is_some() && !self.connected
                    || id
                        .strip_prefix("rpc:")
                        .is_some_and(|n| self.rpc.contains_key(n));
                if pending {
                    self.disconnect();
                    Err("Discord RPC timed out; refresh to reconnect".into())
                } else {
                    Ok(())
                }
            }
            Event::Timer(id) if id == "renew" => self.exchange(None),
            Event::Suspend => {
                self.disconnect();
                Ok(())
            }
            Event::Resume if self.config.rpc && !self.faces.is_empty() => self.connect(false),
            _ => Ok(()),
        };
        if let Err(error) = result {
            self.notice = Some(error);
        }
        self.draw();
    }
    fn save(&mut self) -> Vec<u8> {
        serde_json::to_vec(&self.saved).unwrap_or_default()
    }
}
glaze_plugin_sdk::export!(Discord);
