//! Runs the actual component and inspects its broker effects. No requests are dispatched to
//! Discord and no real pipe is opened; fixture secrets live in a unique test namespace.
use crate::{caps, runtime::{Effect, Manifest, Runtime}};
use crate::runtime::exports::glaze::plugin::lifecycle::{Action, Event, FaceOpen};
use crate::runtime::glaze::plugin::{http::{Reply,Response},pipes::PipeEvent,types::SizeClass};
use serde_json::{Value,json};

fn view(effects: &[Effect]) -> String {
    effects.iter().filter_map(|e|if let Effect::View(_,f)=e {Some(serde_json::to_string(f).unwrap())}else{None}).collect()
}
fn request(effects: &[Effect], suffix: &str) -> u64 {
    effects.iter().find_map(|e|if let Effect::Fetch{id,url,..}=e {url.ends_with(suffix).then_some(*id)} else {None}).expect("brokered request")
}
fn reply(id:u64,body:Value) -> Event {Event::Http(Reply{id,outcome:Ok(Response{status:200,headers:vec![],body:serde_json::to_vec(&body).unwrap()})})}
fn act(name:&str,value:Option<&str>) -> Event {Event::Action(Action{instance:"main".into(),action:name.into(),value:value.map(str::to_owned)})}
fn rpc(pipe:u64,value:Value) -> Event {
    let body=serde_json::to_vec(&value).unwrap();
    let mut bytes=1u32.to_le_bytes().to_vec();bytes.extend((body.len() as u32).to_le_bytes());bytes.extend(body);
    Event::Pipe(PipeEvent::Data((pipe,bytes)))
}
#[test]
fn discord_wasm_brokers_bot_rpc_notifications_and_secret_state() -> anyhow::Result<()> {
    let dir=std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../plugins/discord");
    let mut manifest=Manifest::load(&dir)?;
    manifest.id=format!("discord-test-{}",std::process::id());
    let scratch=std::env::temp_dir().join(&manifest.id);
    let runtime=Runtime::new(scratch.join("cache"),scratch.join("data"))?;
    let (component,_)=runtime.component(&manifest,&dir)?;
    let result=(|| -> anyhow::Result<()> {
        let mut instance=runtime.instantiate(&manifest,&component,None)?;
        let open=|settings:Value|Event::Open(FaceOpen{instance:"main".into(),face:"main".into(),size:SizeClass::Large,settings:settings.to_string()});
        instance.event(&open(json!({"clientId":"12345"})))?;
        instance.event(&act("bot-token",Some("fixture-bot")))?;
        instance.event(&act("client-secret",Some("fixture-secret&value")))?;
        assert_eq!(caps::secret_get(&manifest.id,"bot").map_err(anyhow::Error::msg)?,Some("fixture-bot".into()));
        let effects=instance.event(&open(json!({"clientId":"12345","channelIds":"42,43"})))?;
        let validate=request(&effects,"users/@me");
        let effects=instance.event(&reply(validate,json!({"bot":false})))?;
        assert!(view(&effects).contains("personal account tokens are not supported"));
        assert!(!effects.iter().any(|e|matches!(e,Effect::Fetch{..})));
        let effects=instance.event(&act("refresh",None))?;
        let effects=instance.event(&reply(request(&effects,"users/@me"),json!({"bot":true})))?;
        let history=request(&effects,"channels/43/messages?limit=30");
        instance.event(&reply(history,json!([{"id":"99","author":{"username":"Ada"},"content":"Fixture message","timestamp":"2026-11-06T00:00:00Z"}])))?;
        instance.event(&act("channel",Some("43")))?;
        instance.event(&act("reply:99",None))?;
        let effects=instance.event(&act("send",Some("@everyone fixture")))?;
        let (_,body)=effects.iter().find_map(|e|match e {Effect::Fetch{url,method,body,..} if *method=="POST"=>Some((url,body)),_=>None}).expect("bot send");
        let body:Value=serde_json::from_slice(body.as_ref().unwrap())?;
        assert_eq!(body["allowed_mentions"]["parse"],json!([]));
        assert_eq!(body["message_reference"]["channel_id"],"43");
        assert_eq!(body["message_reference"]["message_id"],"99");
        assert!(!instance.event(&act("send",Some("duplicate")))?.iter().any(|e|matches!(e,Effect::Fetch{..})));

        let effects=instance.event(&act("authorize",None))?;
        let pipe=effects.iter().find_map(|e|if let Effect::Pipe{id,name}=e {assert_eq!(name,"discord-ipc-0");Some(*id)}else{None}).expect("raw IPC broker");
        instance.event(&rpc(pipe,json!({"evt":"READY","data":{}})))?;
        let effects=instance.event(&rpc(pipe,json!({"nonce":"1","data":{"code":"fixture-code"}})))?;
        let exchange=request(&effects,"oauth2/token");
        let effects=instance.event(&reply(exchange,json!({"access_token":"fixture-access","refresh_token":"fixture-refresh","expires_in":3600})))?;
        assert!(effects.iter().all(|e|!matches!(e,Effect::Fetch{..})));
        instance.event(&rpc(pipe,json!({"nonce":"2","data":{"scopes":["rpc.voice.read"]}})))?;
        instance.event(&rpc(pipe,json!({"nonce":"5","data":{"id":"7","name":"Design room","voice_states":[]}})))?;
        let effects=instance.event(&act("mute",None))?;
        assert!(view(&effects).contains("did not grant voice controls"));
        let saved=instance.save()?;
        let saved=String::from_utf8(saved)?;
        for secret in ["fixture-bot","fixture-secret","fixture-access","fixture-refresh"] {assert!(!saved.contains(secret));}

        let mut previews=runtime.instantiate(&manifest,&component,None)?;
        previews.event(&open(json!({})))?;
        let effects=previews.event(&Event::Notification(json!({"kind":"added","id":7,"card":{"id":7,"appId":"com.squirrel.Discord.Discord","title":"Ada","body":"Preview fixture"}}).to_string()))?;
        assert!(view(&effects).contains("Preview fixture"));
        let effects=previews.event(&Event::Notification(json!({"kind":"removed","id":7}).to_string()))?;
        assert!(!view(&effects).contains("Preview fixture"));
        Ok(())
    })();
    for key in ["bot","client-secret","rpc-access","rpc-refresh"] {let _=caps::secret_remove(&manifest.id,key);}
    drop(component);drop(runtime);
    assert!(scratch.starts_with(std::env::temp_dir()) && scratch.file_name().unwrap().to_string_lossy().starts_with("discord-test-"));
    let _=std::fs::remove_dir_all(scratch);
    result
}
