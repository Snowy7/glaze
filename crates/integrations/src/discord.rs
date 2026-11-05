//! Official Discord IPC for authorized testers/approved apps; bot REST never uses a user token.
use super::{Action, Availability, Data, Publish, calendar_http as http, credentials};
use anyhow::{Context, Result, bail, ensure};
use serde_json::{Value, json};
use std::collections::VecDeque;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::windows::named_pipe::{ClientOptions, NamedPipeClient},
    sync::mpsc,
    time::{Duration, timeout},
};

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Participant {
    pub id: String,
    pub name: String,
    pub speaking: bool,
    pub muted: bool,
    pub deafened: bool,
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Call {
    pub channel_id: String,
    pub channel: String,
    pub participants: Vec<Participant>,
    pub muted: bool,
    pub deafened: bool,
    pub can_control: bool,
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Message {
    pub id: String,
    pub channel_id: String,
    pub author: String,
    pub content: String,
    pub timestamp: String,
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Snapshot {
    pub call: Option<Call>,
    pub messages: Vec<Message>,
    pub voice_status: String,
    pub bot_status: String,
}

/// AUMIDs differ between Discord's stable, PTB, Canary and Store distributions.
pub fn is_discord_card(card: &super::notifications::Card) -> bool {
    is_discord_app_id(&card.app_id)
}
pub fn is_discord_app_id(app_id: &str) -> bool {
    let id = app_id.to_ascii_lowercase();
    matches!(
        id.as_str(),
        "com.squirrel.discord.discord"
            | "com.squirrel.discordptb.discordptb"
            | "com.squirrel.discordcanary.discordcanary"
    ) || (id.starts_with("discordinc.") && id.contains("!discord"))
}

fn string(v: &Value, key: &str) -> String {
    v[key]
        .as_str()
        .unwrap_or_default()
        .chars()
        .take(2000)
        .collect()
}
fn snowflake(id: &str) -> Result<()> {
    ensure!(
        !id.is_empty() && id.len() <= 20 && id.bytes().all(|b| b.is_ascii_digit()),
        "Expected a Discord channel ID"
    );
    Ok(())
}
pub fn invite_url(client_id: &str) -> Result<String> {
    snowflake(client_id)?;
    // VIEW_CHANNEL | SEND_MESSAGES | READ_MESSAGE_HISTORY. No administrator permission.
    Ok(format!(
        "https://discord.com/oauth2/authorize?client_id={client_id}&scope=bot&permissions=68608&integration_type=0"
    ))
}

pub fn configure_bot(client_id: &str) -> Result<Option<String>> {
    let id = super::calendar_setup::prompt(
        "Set up your Discord bot",
        "Create a Discord application and bot in the Developer Portal. Paste the application ID in User name and the BOT token in Password. Never enter a personal account token. Discord's Add bot page opens next.",
        client_id,
        Some("discord/bot"),
    )?;
    if let Some(id) = &id {
        snowflake(id)?;
    }
    Ok(id)
}
pub fn configure_voice(client_id: &str) -> Result<Option<String>> {
    if !client_id.is_empty() && credentials::read("discord/client-secret").is_ok() {
        return Ok(Some(client_id.into()));
    }
    let id = super::calendar_setup::prompt(
        "Set up Discord current call",
        "Register an application in Discord's Developer Portal, set an RPC Origin, and add yourself as a tester. Paste the application ID in User name and application client secret in Password. Discord's consent prompt opens next.",
        client_id,
        Some("discord/client-secret"),
    )?;
    if let Some(id) = &id {
        snowflake(id)?;
    }
    Ok(id)
}
fn bot_token() -> Result<credentials::Secret> {
    let token = credentials::read("discord/bot")?;
    let mut authorization = format!("Bot {}", token.text()?);
    let result = http::request(
        "GET",
        "https://discord.com/api/v10/users/@me",
        &[("Authorization", &authorization)],
        &[],
        64 * 1024,
    )
    .and_then(|bytes| Ok(serde_json::from_slice::<Value>(&bytes)?));
    // SAFETY: replacing UTF-8 bytes with zero preserves UTF-8 validity.
    unsafe {
        authorization.as_bytes_mut().fill(0);
    }
    ensure!(
        result?["bot"].as_bool() == Some(true),
        "Discord requires a bot token; personal account tokens are not supported"
    );
    Ok(token)
}
fn bot_request(
    token: &credentials::Secret,
    method: &str,
    channel: &str,
    suffix: &str,
    body: &[u8],
) -> Result<Value> {
    snowflake(channel)?;
    let mut authorization = format!("Bot {}", token.text()?);
    let result = http::request(
        method,
        &format!("https://discord.com/api/v10/channels/{channel}/{suffix}"),
        &[
            ("Authorization", &authorization),
            ("Content-Type", "application/json"),
        ],
        body,
        512 * 1024,
    )
    .and_then(|bytes| Ok(serde_json::from_slice(&bytes)?));
    // SAFETY: replacing bytes by zero preserves UTF-8.
    unsafe {
        authorization.as_bytes_mut().fill(0);
    }
    result
}
pub fn history(channels: &[String]) -> Result<Vec<Message>> {
    let mut messages = Vec::new();
    let token = bot_token()?;
    for channel in channels.iter().take(8) {
        let data = bot_request(&token, "GET", channel, "messages?limit=30", &[])?;
        for item in data
            .as_array()
            .context("Discord did not return channel messages")?
        {
            messages.push(Message {
                id: string(item, "id"),
                channel_id: channel.clone(),
                author: item["author"]["global_name"]
                    .as_str()
                    .or(item["author"]["username"].as_str())
                    .unwrap_or("Discord")
                    .into(),
                content: string(item, "content"),
                timestamp: string(item, "timestamp"),
            });
        }
    }
    messages.sort_by(|a, b| b.timestamp.cmp(&a.timestamp));
    messages.truncate(100);
    Ok(messages)
}
/// User clicks Send. Discord displays the bot as sender. Never impersonates the signed-in user.
pub fn send(channel: &str, content: &str, reply_to: Option<&str>) -> Result<()> {
    ensure!(
        !content.trim().is_empty() && content.chars().count() <= 2000,
        "Message must contain 1–2000 characters"
    );
    let mut body = json!({"content":content,"allowed_mentions":{"parse":[],"replied_user":false}});
    if let Some(id) = reply_to {
        snowflake(id)?;
        body["message_reference"] =
            json!({"message_id":id,"channel_id":channel,"fail_if_not_exists":true});
    }
    bot_request(
        &bot_token()?,
        "POST",
        channel,
        "messages",
        &serde_json::to_vec(&body)?,
    )?;
    Ok(())
}

struct Rpc {
    pipe: NamedPipeClient,
    nonce: u64,
    events: VecDeque<Value>,
    buffer: Vec<u8>,
}
impl Rpc {
    async fn open(config: &super::config::Discord) -> Result<Self> {
        snowflake(&config.client_id)
            .context("Register a Discord application and configure its client ID")?;
        let pipe = if config.pipe.is_empty() {
            r"\\.\pipe\discord-ipc-0"
        } else {
            &config.pipe
        };
        let mut rpc = Self {
            pipe: ClientOptions::new()
                .open(pipe)
                .context("Discord IPC is unavailable; open Discord and retry")?,
            nonce: 0,
            events: VecDeque::new(),
            buffer: Vec::new(),
        };
        rpc.write(0, &json!({"v":1,"client_id":config.client_id}))
            .await?;
        let ready = timeout(Duration::from_secs(5), rpc.receive()).await??;
        ensure!(
            ready["evt"] == "READY",
            "Discord did not accept the registered application"
        );
        Ok(rpc)
    }
    async fn write(&mut self, opcode: u32, value: &Value) -> Result<()> {
        let bytes = serde_json::to_vec(value)?;
        ensure!(bytes.len() <= 512 * 1024, "Discord frame exceeds 512 KiB");
        self.pipe.write_all(&opcode.to_le_bytes()).await?;
        self.pipe
            .write_all(&(bytes.len() as u32).to_le_bytes())
            .await?;
        self.pipe.write_all(&bytes).await?;
        Ok(())
    }
    async fn receive(&mut self) -> Result<Value> {
        loop {
            if self.buffer.len() < 8 {
                ensure!(
                    self.pipe.read_buf(&mut self.buffer).await? > 0,
                    "Discord IPC disconnected"
                );
                continue;
            }
            let opcode = u32::from_le_bytes(self.buffer[..4].try_into().unwrap());
            let size = u32::from_le_bytes(self.buffer[4..8].try_into().unwrap()) as usize;
            ensure!(size <= 512 * 1024, "Discord frame exceeds 512 KiB");
            if self.buffer.len() < size + 8 {
                // read_buf is cancellation safe: a button interrupt cannot lose a partial IPC frame.
                ensure!(
                    self.pipe.read_buf(&mut self.buffer).await? > 0,
                    "Discord IPC disconnected"
                );
                continue;
            }
            let value: Value = serde_json::from_slice(&self.buffer[8..8 + size])?;
            self.buffer.drain(..8 + size);
            match opcode {
                1 => return Ok(value),
                2 => bail!("Discord closed RPC; verify tester membership and approved scopes"),
                3 => self.write(4, &value).await?,
                4 => {}
                _ => bail!("Unsupported Discord IPC frame"),
            }
        }
    }
    async fn request(&mut self, command: &str, args: Value, event: Option<&str>) -> Result<Value> {
        self.nonce += 1;
        let nonce = self.nonce.to_string();
        let mut payload = json!({"cmd":command,"args":args,"nonce":nonce});
        if let Some(event) = event {
            payload["evt"] = json!(event);
        }
        self.write(1, &payload).await?;
        timeout(Duration::from_secs(if command=="AUTHORIZE" {180}else{5}),async {
            loop { let value=self.receive().await?;
                if value["nonce"].as_str()==Some(&nonce) {
                    ensure!(value["evt"]!="ERROR","Discord rejected {command}; registered app/tester or scope approval is required (code {})",value["data"]["code"]);
                    return Ok(value["data"].clone());
                }
                ensure!(self.events.len()<128,"Discord events overflowed; refresh for a new snapshot");self.events.push_back(value);
            }
        }).await?
    }
    async fn next(&mut self) -> Result<Value> {
        if let Some(value) = self.events.pop_front() {
            Ok(value)
        } else {
            self.receive().await
        }
    }
    async fn subscribe(&mut self, event: &str, channel: Option<&str>, on: bool) -> Result<()> {
        self.request(
            if on { "SUBSCRIBE" } else { "UNSUBSCRIBE" },
            channel.map_or(json!({}), |id| json!({"channel_id":id})),
            Some(event),
        )
        .await?;
        Ok(())
    }
}
fn exchange(config: &super::config::Discord, code: Option<&str>) -> Result<String> {
    let secret = credentials::read("discord/client-secret")
        .context("Store this personal Discord application's client secret in Credential Manager")?;
    let refresh = if code.is_none() {
        Some(credentials::read("discord/rpc-refresh")?)
    } else {
        None
    };
    let mut fields = vec![
        ("client_id", config.client_id.as_str()),
        ("client_secret", secret.text()?),
    ];
    if let Some(code) = code {
        fields.extend([("grant_type", "authorization_code"), ("code", code)]);
    } else {
        fields.extend([
            ("grant_type", "refresh_token"),
            ("refresh_token", refresh.as_ref().unwrap().text()?),
        ]);
    }
    let mut body = http::form(&fields);
    let result = (|| -> Result<String> {
        let mut bytes = http::request(
            "POST",
            "https://discord.com/api/oauth2/token",
            &[("Content-Type", "application/x-www-form-urlencoded")],
            body.as_bytes(),
            32 * 1024,
        )?;
        let response: Value = serde_json::from_slice(&bytes)?;
        bytes.fill(0);
        let access = response["access_token"]
            .as_str()
            .context("Discord did not issue an access token")?;
        credentials::write("discord/rpc-access", access)?;
        if let Some(refresh) = response["refresh_token"].as_str() {
            credentials::write("discord/rpc-refresh", refresh)?;
        }
        credentials::write(
            "discord/rpc-expiry",
            &(chrono::Utc::now().timestamp() + response["expires_in"].as_i64().unwrap_or(3600))
                .to_string(),
        )?;
        Ok(access.into())
    })();
    // SAFETY: zero bytes preserve UTF-8.
    unsafe {
        body.as_bytes_mut().fill(0);
    }
    result
}
/// An explicit Connect click opens Discord's own consent modal. Testers may use a personal app.
pub fn connect(config: &super::config::Discord) -> Result<()> {
    credentials::read("discord/client-secret")
        .context("Register a personal Discord app and store its client secret first")?;
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?
        .block_on(async {
            let mut rpc = Rpc::open(config).await?;
            let mut scopes = vec!["rpc", "rpc.voice.read"];
            if config.voice_controls {
                scopes.push("rpc.voice.write");
            }
            let response = rpc
                .request(
                    "AUTHORIZE",
                    json!({"client_id":config.client_id,"scopes":scopes}),
                    None,
                )
                .await?;
            let code = response["code"]
                .as_str()
                .context("Discord did not issue an authorization code")?;
            exchange(config, Some(code))?;
            Ok(())
        })
}
fn parse_call(channel: &Value, settings: &Value, can_control: bool) -> Option<Call> {
    if channel.is_null() {
        return None;
    }
    let participants = channel["voice_states"]
        .as_array()
        .into_iter()
        .flatten()
        .take(100)
        .map(|p| {
            let state = &p["voice_state"];
            Participant {
                id: string(&p["user"], "id"),
                name: p["nick"]
                    .as_str()
                    .or(p["user"]["global_name"].as_str())
                    .or(p["user"]["username"].as_str())
                    .unwrap_or("Discord user")
                    .into(),
                speaking: false,
                muted: state["mute"].as_bool().unwrap_or(false)
                    || state["self_mute"].as_bool().unwrap_or(false)
                    || p["mute"].as_bool().unwrap_or(false),
                deafened: state["deaf"].as_bool().unwrap_or(false)
                    || state["self_deaf"].as_bool().unwrap_or(false),
            }
        })
        .collect();
    Some(Call {
        channel_id: string(channel, "id"),
        channel: string(channel, "name"),
        participants,
        muted: settings["mute"].as_bool().unwrap_or(false),
        deafened: settings["deaf"].as_bool().unwrap_or(false),
        can_control,
    })
}
async fn refresh_call(
    rpc: &mut Rpc,
    previous: &Option<Call>,
    can_control: bool,
) -> Result<Option<Call>> {
    let channel = rpc
        .request("GET_SELECTED_VOICE_CHANNEL", json!({}), None)
        .await?;
    let settings = rpc.request("GET_VOICE_SETTINGS", json!({}), None).await?;
    let mut call = parse_call(&channel, &settings, can_control);
    if let (Some(call), Some(old)) = (&mut call, previous) {
        for p in &mut call.participants {
            p.speaking = old.participants.iter().any(|v| v.id == p.id && v.speaking);
        }
    }
    let old = previous.as_ref().map(|c| c.channel_id.as_str());
    let new = call.as_ref().map(|c| c.channel_id.as_str());
    if old != new {
        for event in [
            "VOICE_STATE_CREATE",
            "VOICE_STATE_UPDATE",
            "VOICE_STATE_DELETE",
            "SPEAKING_START",
            "SPEAKING_STOP",
        ] {
            if let Some(id) = old {
                rpc.subscribe(event, Some(id), false).await?;
            }
            if let Some(id) = new {
                rpc.subscribe(event, Some(id), true).await?;
            }
        }
    }
    Ok(call)
}
pub(crate) async fn run(
    config: &super::config::Discord,
    publish: &Publish,
    commands: &mut mpsc::Receiver<Action>,
) -> Result<()> {
    let mut snapshot = Snapshot {
        voice_status: "Connect a registered Discord RPC app (personal testers supported)".into(),
        bot_status: "Add a bot to your server for history and sending".into(),
        ..Default::default()
    };
    if !config.channel_ids.is_empty() {
        let ids = config.channel_ids.clone();
        match tokio::task::spawn_blocking(move || history(&ids)).await {
            Ok(Ok(messages)) => {
                snapshot.messages = messages;
                snapshot.bot_status = "Bot history · refresh to update · sends as bot".into();
            }
            Ok(Err(e)) => snapshot.bot_status = e.to_string(),
            Err(_) => snapshot.bot_status = "Discord history worker stopped".into(),
        }
    }
    let mut rpc = None;
    let mut can_control = false;
    if config.rpc {
        let result = async {
            let expiry = credentials::read("discord/rpc-expiry")
                .ok()
                .and_then(|s| s.text().ok()?.parse::<i64>().ok())
                .unwrap_or(0);
            let token = if expiry <= chrono::Utc::now().timestamp() + 60 {
                exchange(config, None)?
            } else {
                credentials::read("discord/rpc-access")?.text()?.to_owned()
            };
            let mut client = Rpc::open(config).await?;
            let auth = client
                .request("AUTHENTICATE", json!({"access_token":token}), None)
                .await?;
            can_control = config.voice_controls
                && auth["scopes"]
                    .as_array()
                    .is_some_and(|a| a.iter().any(|s| s == "rpc.voice.write" || s == "rpc"));
            for event in ["VOICE_CHANNEL_SELECT", "VOICE_SETTINGS_UPDATE"] {
                client.subscribe(event, None, true).await?;
            }
            snapshot.call = refresh_call(&mut client, &None, can_control).await?;
            Ok::<_, anyhow::Error>(client)
        }
        .await;
        match result {
            Ok(client) => {
                rpc = Some(client);
                snapshot.voice_status = "Connected to Discord RPC".into();
            }
            Err(e) => snapshot.voice_status = format!("Voice unavailable: {e}"),
        }
    }
    loop {
        publish.set(
            if rpc.is_some() {
                Availability::Live
            } else {
                Availability::Cached
            },
            Some(Data::Discord(snapshot.clone())),
        );
        tokio::select! {
            value=async { match &mut rpc{Some(client)=>client.next().await,None=>std::future::pending().await} }=>{
                let value=value?;
                match value["evt"].as_str().unwrap_or("") {
                    "SPEAKING_START"|"SPEAKING_STOP"=>{if let Some(call)=&mut snapshot.call{for participant in &mut call.participants{if participant.id==value["data"]["user_id"]{participant.speaking=value["evt"]=="SPEAKING_START";}}}}
                    "VOICE_CHANNEL_SELECT"|"VOICE_STATE_CREATE"|"VOICE_STATE_UPDATE"|"VOICE_STATE_DELETE"|"VOICE_SETTINGS_UPDATE"=>snapshot.call=refresh_call(rpc.as_mut().unwrap(),&snapshot.call,can_control).await?,
                    _=>{},
                }
            }
            Some(action)=commands.recv()=>{
                let result=match action{
                    Action::DiscordVoice{muted,deafened}=>{
                        if !can_control {Err(anyhow::anyhow!("Discord did not grant voice controls"))}else if let Some(client)=&mut rpc {
                            let mut args=json!({});
                            if let Some(muted)=muted{args["mute"]=json!(muted);}
                            if let Some(deafened)=deafened{args["deaf"]=json!(deafened);}
                            let response=client.request("SET_VOICE_SETTINGS",args,None).await;
                            if response.is_ok(){snapshot.call=refresh_call(client,&snapshot.call,can_control).await?;}response.map(|_|())
                        }else{Err(anyhow::anyhow!("Discord voice is disconnected"))}
                    }
                    _=>Ok(()),
                };publish.action_result(result);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn bot_invite_is_scoped_and_call_keeps_voice_states() {
        assert!(invite_url("1&scope=rpc").is_err());
        assert!(invite_url("12345").unwrap().contains("permissions=68608"));
        let call=parse_call(&json!({"id":"42","name":"General","voice_states":[{"user":{"id":"7","username":"Alex"},"voice_state":{"self_mute":true,"deaf":false}}]}),&json!({"mute":false,"deaf":true}),false).unwrap();
        assert_eq!(call.channel, "General");
        assert!(call.participants[0].muted);
        assert!(call.deafened);
        assert!(!call.can_control);
    }

    #[test]
    fn ipc_uses_registered_app_and_keeps_partial_frames_across_button_interrupts() {
        tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap().block_on(async {
            use tokio::net::windows::named_pipe::ServerOptions;
            async fn read(pipe:&mut tokio::net::windows::named_pipe::NamedPipeServer)->(u32,Value){
                let mut head=[0;8];pipe.read_exact(&mut head).await.unwrap();let size=u32::from_le_bytes(head[4..].try_into().unwrap());let mut bytes=vec![0;size as usize];pipe.read_exact(&mut bytes).await.unwrap();(u32::from_le_bytes(head[..4].try_into().unwrap()),serde_json::from_slice(&bytes).unwrap())
            }
            async fn send(pipe:&mut tokio::net::windows::named_pipe::NamedPipeServer,value:Value){
                let bytes=serde_json::to_vec(&value).unwrap();pipe.write_all(&1_u32.to_le_bytes()).await.unwrap();pipe.write_all(&(bytes.len()as u32).to_le_bytes()).await.unwrap();pipe.write_all(&bytes).await.unwrap();
            }
            let name=format!(r"\\.\pipe\mypaper-discord-fixture-{}",std::process::id());
            let mut server=ServerOptions::new().first_pipe_instance(true).create(&name).unwrap();
            let job=tokio::spawn(async move{
                server.connect().await.unwrap();let(opcode,handshake)=read(&mut server).await;assert_eq!(opcode,0);assert_eq!(handshake,json!({"v":1,"client_id":"12345"}));
                send(&mut server,json!({"cmd":"DISPATCH","evt":"READY","data":{}})).await;
                let(_,auth)=read(&mut server).await;assert_eq!(auth["cmd"],"AUTHENTICATE");
                send(&mut server,json!({"cmd":"AUTHENTICATE","data":{"scopes":["rpc.voice.read"]},"nonce":auth["nonce"]})).await;
                let bytes=serde_json::to_vec(&json!({"cmd":"DISPATCH","evt":"SPEAKING_START","data":{"user_id":"7"}})).unwrap();
                server.write_all(&1_u32.to_le_bytes()).await.unwrap();server.write_all(&(bytes.len()as u32).to_le_bytes()).await.unwrap();server.write_all(&bytes[..3]).await.unwrap();
                tokio::time::sleep(Duration::from_millis(50)).await;server.write_all(&bytes[3..]).await.unwrap();tokio::time::sleep(Duration::from_millis(50)).await;
            });
            let mut rpc=Rpc::open(&super::super::config::Discord{client_id:"12345".into(),pipe:name,..Default::default()}).await.unwrap();
            let auth=rpc.request("AUTHENTICATE",json!({"access_token":"fixture-only"}),None).await.unwrap();assert_eq!(auth["scopes"],json!(["rpc.voice.read"]));
            assert!(timeout(Duration::from_millis(5),rpc.next()).await.is_err());
            let event=rpc.next().await.unwrap();assert_eq!(event["evt"],"SPEAKING_START");assert_eq!(event["data"]["user_id"],"7");job.await.unwrap();
        });
    }
}
