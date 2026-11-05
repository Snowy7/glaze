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
    tokio::runtime::Builder::new_current_thread().enable_all().build()?.block_on(async {
        let secret=credentials::read("t3")?;
        let mut request="ws://127.0.0.1:3773/ws?orchestrationProtocol=2".into_client_request()?;
        request.headers_mut().insert("authorization",format!("Bearer {}",secret.text()?).parse()?);
        let config=WebSocketConfig::default().max_message_size(Some(32*1024*1024)).max_frame_size(Some(32*1024*1024));
        let (mut socket,_) = tokio_tungstenite::connect_async_with_config(request,Some(config),true).await?;
        let args:Vec<_>=std::env::args().collect();
        let method=args.get(1).map(String::as_str).unwrap_or("orchestration.subscribeShell");
        let payload:Value=args.get(2).map(|s|serde_json::from_str(s)).transpose()?.unwrap_or_else(|| if method == "server.getUsageSummary" {json!({"sinceDay":"2026-11-05","untilDay":"2026-11-05","timeZone":"UTC"})}else{json!({})});
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
