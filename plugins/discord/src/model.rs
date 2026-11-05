use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

pub const MAX_FRAME: usize = 512 * 1024;

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Participant {
    pub id: String,
    pub name: String,
    pub speaking: bool,
    pub muted: bool,
    pub deafened: bool,
}
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Call {
    pub channel_id: String,
    pub channel: String,
    pub participants: Vec<Participant>,
    pub muted: bool,
    pub deafened: bool,
    pub can_control: bool,
}
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Message {
    pub id: String,
    pub channel_id: String,
    pub author: String,
    pub content: String,
    pub timestamp: String,
}
#[derive(Default, Clone, Serialize, Deserialize)]
pub struct Saved {
    pub expiry: u64,
    pub faces: std::collections::BTreeMap<String, Choice>,
}
#[derive(Default, Clone, Serialize, Deserialize)]
pub struct Choice {
    pub voice: bool,
    pub channel: String,
    pub reply: Option<String>,
    #[serde(skip)]
    pub setup: bool,
}
#[derive(Default, Clone, PartialEq, Eq)]
pub struct Config {
    pub client_id: String,
    pub channels: Vec<String>,
    pub rpc: bool,
    pub voice_controls: bool,
}
impl Config {
    pub fn parse(value: &Value) -> Result<Self, String> {
        let client_id = text(value, "clientId");
        if !client_id.is_empty() {
            snowflake(&client_id)?;
        }
        let channels = match &value["channelIds"] {
            Value::Array(ids) => ids
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_owned)
                .collect(),
            Value::String(ids) => ids
                .split([',', ' ', '\n', ';'])
                .filter(|s| !s.is_empty())
                .map(str::to_owned)
                .collect(),
            _ => Vec::new(),
        };
        if channels.len() > 8 {
            return Err("Choose at most eight Discord channels".into());
        }
        for id in &channels {
            snowflake(id)?;
        }
        Ok(Self {
            client_id,
            channels,
            rpc: value["rpc"].as_bool().unwrap_or(false),
            voice_controls: value["voiceControls"].as_bool().unwrap_or(false),
        })
    }
}
pub fn text(value: &Value, key: &str) -> String {
    value[key]
        .as_str()
        .unwrap_or_default()
        .chars()
        .take(2000)
        .collect()
}
pub fn snowflake(id: &str) -> Result<(), String> {
    if !id.is_empty() && id.len() <= 20 && id.bytes().all(|b| b.is_ascii_digit()) {
        Ok(())
    } else {
        Err("Expected a numeric Discord ID".into())
    }
}
pub fn frame(op: u32, payload: &[u8]) -> Result<Vec<u8>, String> {
    if payload.len() > MAX_FRAME {
        return Err("Discord frame exceeds 512 KiB".into());
    }
    let mut bytes = Vec::with_capacity(payload.len() + 8);
    bytes.extend(op.to_le_bytes());
    bytes.extend((payload.len() as u32).to_le_bytes());
    bytes.extend(payload);
    Ok(bytes)
}
#[derive(Default)]
pub struct Decoder {
    buffer: Vec<u8>,
}
impl Decoder {
    /// Keeps incomplete headers/bodies between pipe events, and emits coalesced frames in order.
    pub fn push(&mut self, bytes: &[u8]) -> Result<Vec<(u32, Vec<u8>)>, String> {
        // Host chunks are bounded; a partial maximum frame plus one chunk is sufficient.
        if self.buffer.len() + bytes.len() > MAX_FRAME * 2 + 16 {
            return Err("Discord receive buffer exceeded".into());
        }
        self.buffer.extend_from_slice(bytes);
        let mut output = Vec::new();
        while self.buffer.len() >= 8 {
            let op = u32::from_le_bytes(self.buffer[..4].try_into().unwrap());
            let len = u32::from_le_bytes(self.buffer[4..8].try_into().unwrap()) as usize;
            if len > MAX_FRAME {
                return Err("Discord frame exceeds 512 KiB".into());
            }
            if self.buffer.len() < len + 8 {
                break;
            }
            output.push((op, self.buffer[8..8 + len].to_vec()));
            self.buffer.drain(..8 + len);
        }
        Ok(output)
    }
}
pub fn parse_call(
    channel: &Value,
    settings: &Value,
    can_control: bool,
    previous: Option<&Call>,
) -> Option<Call> {
    if channel.is_null() {
        return None;
    }
    let participants = channel["voice_states"]
        .as_array()
        .into_iter()
        .flatten()
        .take(100)
        .map(|p| {
            let id = text(&p["user"], "id");
            Participant {
                speaking: previous
                    .is_some_and(|c| c.participants.iter().any(|v| v.id == id && v.speaking)),
                id,
                name: p["nick"]
                    .as_str()
                    .or(p["user"]["global_name"].as_str())
                    .or(p["user"]["username"].as_str())
                    .unwrap_or("Discord user")
                    .chars()
                    .take(200)
                    .collect(),
                muted: p["voice_state"]["mute"].as_bool().unwrap_or(false)
                    || p["voice_state"]["self_mute"].as_bool().unwrap_or(false)
                    || p["mute"].as_bool().unwrap_or(false),
                deafened: p["voice_state"]["deaf"].as_bool().unwrap_or(false)
                    || p["voice_state"]["self_deaf"].as_bool().unwrap_or(false),
            }
        })
        .collect();
    Some(Call {
        channel_id: text(channel, "id"),
        channel: text(channel, "name"),
        participants,
        muted: settings["mute"].as_bool().unwrap_or(false),
        deafened: settings["deaf"].as_bool().unwrap_or(false),
        can_control,
    })
}
pub fn messages(channel: &str, value: &Value) -> Result<Vec<Message>, String> {
    let items = value
        .as_array()
        .ok_or("Discord did not return channel messages")?;
    Ok(items
        .iter()
        .take(30)
        .map(|m| Message {
            id: text(m, "id"),
            channel_id: channel.into(),
            author: m["author"]["global_name"]
                .as_str()
                .or(m["author"]["username"].as_str())
                .unwrap_or("Discord")
                .chars()
                .take(200)
                .collect(),
            content: text(m, "content"),
            timestamp: text(m, "timestamp"),
        })
        .collect())
}
pub fn send_body(channel: &str, content: &str, reply: Option<&str>) -> Result<Value, String> {
    snowflake(channel)?;
    if content.trim().is_empty() || content.chars().count() > 2000 {
        return Err("Message must contain 1–2000 characters".into());
    }
    let mut body =
        json!({"content": content, "allowed_mentions": {"parse": [], "replied_user": false}});
    if let Some(id) = reply {
        snowflake(id)?;
        body["message_reference"] =
            json!({"message_id":id,"channel_id":channel,"fail_if_not_exists":true});
    }
    Ok(body)
}
pub fn form(fields: &[(&str, &str)]) -> Vec<u8> {
    fn encode(s: &str) -> String {
        s.bytes()
            .map(|b| {
                if b.is_ascii_alphanumeric() || b"-._~".contains(&b) {
                    (b as char).to_string()
                } else {
                    format!("%{b:02X}")
                }
            })
            .collect()
    }
    fields
        .iter()
        .map(|(k, v)| format!("{}={}", encode(k), encode(v)))
        .collect::<Vec<_>>()
        .join("&")
        .into_bytes()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn fragmented_and_coalesced_frames_and_oversize() {
        let a = frame(1, br#"{"evt":"READY"}"#).unwrap();
        let b = frame(3, b"ping").unwrap();
        let mut decoder = Decoder::default();
        assert!(decoder.push(&a[..3]).unwrap().is_empty());
        assert!(decoder.push(&a[3..10]).unwrap().is_empty());
        let mut rest = a[10..].to_vec();
        rest.extend(b);
        assert_eq!(
            decoder.push(&rest).unwrap(),
            vec![(1, br#"{"evt":"READY"}"#.to_vec()), (3, b"ping".to_vec())]
        );
        let mut invalid = vec![0; 4];
        invalid.extend(((MAX_FRAME + 1) as u32).to_le_bytes());
        assert!(decoder.push(&invalid).is_err());
    }
    #[test]
    fn safe_bot_sending_and_ids() {
        assert!(send_body("42/evil", "hi", None).is_err());
        assert!(send_body("42", " ", None).is_err());
        assert!(send_body("42", &"a".repeat(2001), None).is_err());
        let body = send_body("42", "@everyone hello", Some("99")).unwrap();
        assert_eq!(body["allowed_mentions"]["parse"], json!([]));
        assert_eq!(body["allowed_mentions"]["replied_user"], false);
        assert_eq!(body["message_reference"]["fail_if_not_exists"], true);
        assert_eq!(
            form(&[("secret", "a&b=+ /é")]),
            b"secret=a%26b%3D%2B%20%2F%C3%A9"
        );
    }
    #[test]
    fn voice_states_keep_speaking_and_control_grants() {
        let channel = json!({"id":"42","name":"Design room","voice_states":[{"user":{"id":"7","username":"Alex"},"voice_state":{"self_mute":true,"deaf":false}}]});
        let old = Call {
            participants: vec![Participant {
                id: "7".into(),
                speaking: true,
                ..Default::default()
            }],
            ..Default::default()
        };
        let call = parse_call(&channel, &json!({"deaf":true}), false, Some(&old)).unwrap();
        assert!(call.participants[0].speaking && call.participants[0].muted && call.deafened);
        assert!(!call.can_control);
        assert!(parse_call(&Value::Null, &json!({}), true, None).is_none());
    }
    #[test]
    fn saved_choices_exclude_credentials_and_setup() {
        let saved = Saved {
            expiry: 123,
            faces: [(
                "x".into(),
                Choice {
                    setup: true,
                    ..Default::default()
                },
            )]
            .into(),
        };
        let value = serde_json::to_value(saved).unwrap();
        assert!(value["faces"]["x"].get("setup").is_none());
        assert_eq!(value.as_object().unwrap().len(), 2);
    }
}
