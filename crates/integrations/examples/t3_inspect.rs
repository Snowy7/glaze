//! Read-only inspection of mypaper's separately paired T3 session. No titles or secrets printed.
use anyhow::Result;
use futures_util::{SinkExt, StreamExt};
use mypaper_integrations::credentials;
use serde_json::{Value, json};
use tokio_tungstenite::tungstenite::{client::IntoClientRequest, protocol::WebSocketConfig, Message};

fn shape(value: &Value) -> Value {
    match value {
        Value::Object(map) => map.iter().map(|(key,value)| (key.clone(), shape(value))).collect(),
        Value::Array(items) => json!({"count":items.len(),"first":items.first().map(shape)}),
        Value::String(s) => if ["Success", "Failure", "Fail", "Die", "orchestration:read", "diagnostics:read", "orchestration:operate"].contains(&s.as_str()) {json!(s)} else {json!("string")},
        _ => value.clone(),
    }
}
fn main() -> Result<()> {
    if std::env::args().nth(1).as_deref() == Some("--limits") {
        return inspect_limits();
    }
    tokio::runtime::Builder::new_current_thread().enable_all().build()?.block_on(async {
        if std::env::args().nth(1).as_deref()==Some("--features") {
            let metadata=mypaper_integrations::t3_client::metadata("").await?;
            let permissions=mypaper_integrations::t3_client::permissions("").await?;
            println!("{}",json!({"omitOlderSettled":metadata.shell_older_settled,"environmentKnown":!metadata.environment.environment_id.is_empty(),"modelCount":metadata.providers.iter().map(|p|p.models.len()).sum::<usize>(),"permissions":permissions,"grok":format!("{:?}",mypaper_integrations::grok::discover())}));
            return Ok(());
        }
        let secret=credentials::read("t3")?;
        let mut request="ws://127.0.0.1:3773/ws?orchestrationProtocol=2".into_client_request()?;
        request.headers_mut().insert("authorization",format!("Bearer {}",secret.text()?).parse()?);
        let config=WebSocketConfig::default().max_message_size(Some(32*1024*1024)).max_frame_size(Some(32*1024*1024));
        let (mut socket,_) = tokio_tungstenite::connect_async_with_config(request,Some(config),true).await?;
        let args:Vec<_>=std::env::args().collect();
        let method=args.get(1).map(String::as_str).unwrap_or("orchestration.subscribeShell");
        let payload:Value=if args.get(2).is_some_and(|s|s=="--narrow"){json!({"omitOlderSettled":true,"omitIdleSubagents":true})}else{args.get(2).map(|s|serde_json::from_str(s)).transpose()?.unwrap_or_else(|| if method == "server.getUsageSummary" {let day=chrono::Utc::now().format("%Y-%m-%d").to_string();json!({"sinceDay":day,"untilDay":day,"timeZone":"UTC"})}else{json!({})})};
        socket.send(Message::Text(json!({"_tag":"Request","id":"1","tag":method,"payload":payload,"headers":[]}).to_string().into())).await?;
        for _ in 0..5 {
            let message=tokio::time::timeout(std::time::Duration::from_secs(10),socket.next()).await?.ok_or_else(||anyhow::anyhow!("closed"))??;
            let text=message.to_text()?;
            let value:Value=serde_json::from_str(text)?;
            println!("{}",json!({"bytes":text.len(),"shape":shape(&value)}));
            if text.contains("\"Chunk\"") {socket.send(Message::Text(json!({"_tag":"Ack","requestId":"1"}).to_string().into())).await?;}
            if text.contains("\"Exit\"") || text.contains("\"snapshot\"") {break;}
        }
        socket.close(None).await?;
        Ok(())
    })
}

/// Verify the production subscription without printing account identities, titles or secrets.
fn inspect_limits() -> Result<()> {
    use mypaper_integrations::{Availability, Data, Hub, Provider, config::Config};
    use std::{collections::{BTreeMap, BTreeSet}, sync::mpsc, time::{Duration, Instant}};
    let (tx, rx) = mpsc::channel();
    let hub = Hub::new(move |_| { let _ = tx.send(()); });
    let mut config = Config::default();
    config.t3.enabled = true;
    config.t3.live_verified = true;
    let lease = hub.acquire(Provider::T3, &config)?;
    let deadline = Instant::now() + Duration::from_secs(30);
    while rx.recv_timeout(deadline.saturating_duration_since(Instant::now())).is_ok() {
        let snapshot = lease.snapshot();
        anyhow::ensure!(!matches!(snapshot.availability, Availability::Stale(_) | Availability::Unavailable(_)), "T3 limits subscription failed: {:?}", snapshot.availability);
        let Some(Data::T3(state)) = snapshot.data else { continue };
        let mut accounts: BTreeMap<&str, BTreeSet<&str>> = BTreeMap::new();
        for limit in &state.limits {
            accounts.entry(&limit.driver).or_default().insert(&limit.instance);
        }
        if accounts.iter().any(|(driver, ids)| driver.contains("claude") && ids.len() >= 2) {
            let providers: BTreeMap<_, _> = accounts.iter().map(|(driver, ids)| (*driver, ids.len())).collect();
            println!("{}", json!({"availability":format!("{:?}",snapshot.availability),"providerAccounts":providers,"limitWindows":state.limits.len(),"usageDistinct":state.history.as_ref().map(|h| h.providers.iter().all(|p| p.sessions_distinct))}));
            return Ok(());
        }
    }
    anyhow::bail!("T3 did not report both Claude accounts before the read deadline")
}
