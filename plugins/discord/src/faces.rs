use crate::model::{Call, Choice, Message};
use mypaper_plugin_sdk::{Face, Role, Tone, View, view::SelectOption};

pub struct Context<'a> {
    pub choice: &'a Choice,
    pub call: Option<&'a Call>,
    pub messages: &'a [Message],
    pub channels: &'a [String],
    pub bot: bool,
    pub connected: bool,
    pub busy: bool,
    pub notice: Option<&'a str>,
    pub voice_status: &'a str,
}
fn control(label: &str, action: &str, icon: &str, enabled: bool) -> View {
    View::IconButton {
        label: label.into(),
        action: action.into(),
        icon: icon.into(),
        enabled,
    }
}
pub fn face(c: Context<'_>, small: bool) -> Face {
    let choice = c.choice;
    let tabs = View::Grid {
        columns: 3,
        gap: Some(4.0),
        children: vec![
            View::Button {
                label: "Chat".into(),
                action: "messages".into(),
                icon: None,
                primary: !choice.voice && !choice.setup,
            },
            View::Button {
                label: "Call".into(),
                action: "voice".into(),
                icon: None,
                primary: choice.voice && !choice.setup,
            },
            control("Discord setup", "setup", "info", true),
        ],
    };
    let mut nodes = vec![tabs];
    let footer;
    if choice.setup {
        nodes.extend([
            View::Field {
                placeholder: "Store bot token…".into(),
                action: "bot-token".into(),
            },
            View::Field {
                placeholder: "Store RPC client secret…".into(),
                action: "client-secret".into(),
            },
        ]);
        if !small {
            nodes.extend([
                View::tinted("Set application and channel IDs in widget settings. Bot messages are sent as your bot. Calls require a registered app and tester access.", Role::Caption,Tone::Secondary),
                View::row([View::button("Connect call", "authorize"),View::button("Forget credentials", "forget")]),
            ]);
        }
        footer = "Credentials stay in Windows Credential Manager";
    } else if choice.voice {
        if let Some(call) = c.call {
            nodes.push(View::text(
                &call.channel,
                if small { Role::Caption } else { Role::Headline },
            ));
            nodes.push(View::Grid {
                columns: 2,
                gap: Some(4.0),
                children: vec![
                    View::row([
                        control(
                            if call.muted { "Unmute" } else { "Mute" },
                            "mute",
                            "mic",
                            call.can_control && !c.busy,
                        ),
                        View::text(if call.muted { "Unmute" } else { "Mute" }, Role::Caption),
                    ]),
                    View::row([
                        control(
                            if call.deafened { "Undeafen" } else { "Deafen" },
                            "deafen",
                            "headphones",
                            call.can_control && !c.busy,
                        ),
                        View::text(
                            if call.deafened { "Undeafen" } else { "Deafen" },
                            Role::Caption,
                        ),
                    ]),
                ],
            });
            nodes.push(View::List {
                gap: Some(4.0),
                children: call
                    .participants
                    .iter()
                    .map(|p| {
                        View::row([
                            View::Icon {
                                icon: "user".into(),
                                size: Some(16.0),
                                color: Some(if p.speaking {
                                    Tone::Positive
                                } else {
                                    Tone::Tertiary
                                }),
                            },
                            View::tinted(
                                &p.name,
                                Role::Caption,
                                if p.speaking {
                                    Tone::Positive
                                } else {
                                    Tone::Label
                                },
                            ),
                            View::tinted(
                                if p.deafened {
                                    "Deafened"
                                } else if p.muted {
                                    "Muted"
                                } else if p.speaking {
                                    "Speaking"
                                } else {
                                    ""
                                },
                                Role::Caption,
                                Tone::Secondary,
                            ),
                        ])
                    })
                    .collect(),
            });
            footer = if call.can_control {
                "Connected · call controls"
            } else {
                "Read only · rpc.voice.write required"
            };
        } else {
            nodes.push(View::text(
                if c.connected {
                    "No active call"
                } else {
                    "Connect your current call"
                },
                Role::Callout,
            ));
            nodes.push(View::Spacer);
            nodes.push(View::button("Connect voice", "authorize"));
            footer = c.voice_status;
        }
    } else {
        let list = View::List {
            gap: Some(2.0),
            children: c
                .messages
                .iter()
                .filter(|m| {
                    choice.channel.is_empty()
                        || m.channel_id.is_empty()
                        || m.channel_id == choice.channel
                })
                .take(30)
                .map(|m| {
                    if !c.bot {
                        View::Stack {
                            gap: Some(0.0),
                            children: vec![
                                View::text(&m.author, Role::Caption),
                                View::tinted(&m.content, Role::Caption, Tone::Secondary),
                            ],
                        }
                    } else {
                        View::ListRow {
                            title: m.author.clone(),
                            detail: (!small).then(|| {
                                if m.content.is_empty() {
                                    "Message content unavailable · check bot intent".into()
                                } else {
                                    m.content.clone()
                                }
                            }),
                            icon: Some("message".into()),
                            trailing: None,
                            action: (c.bot && !m.id.is_empty()).then(|| format!("reply:{}", m.id)),
                        }
                    }
                })
                .collect(),
        };
        if c.bot && !c.channels.is_empty() {
            let mut composer = Vec::new();
            if c.channels.len() > 1 {
                composer.push(View::Select {
                    label: "Channel".into(),
                    action: "channel".into(),
                    options: c
                        .channels
                        .iter()
                        .map(|id| SelectOption {
                            label: format!("#{id}"),
                            value: id.clone(),
                        })
                        .collect(),
                    selected: Some(choice.channel.clone()),
                });
            }
            if let Some(reply) = &choice.reply {
                if !small {
                    nodes.push(View::row([
                        View::tinted(format!("Reply to {reply}"), Role::Caption, Tone::Accent),
                        control("Cancel reply", "cancel-reply", "close", true),
                    ]));
                }
            }
            composer.push(View::Field {
                placeholder: if c.busy {
                    "Sending…"
                } else if choice.reply.is_some() {
                    "Reply as bot…"
                } else {
                    "Send as bot…"
                }
                .into(),
                action: "send".into(),
            });
            nodes.push(View::Grid {
                columns: composer.len() as u8,
                gap: Some(4.0),
                children: composer,
            });
            nodes.push(list);
            footer = "Sends as bot · mentions disabled";
        } else {
            if c.messages.is_empty() {
                nodes.push(View::tinted(
                    "No recent messages",
                    Role::Caption,
                    Tone::Secondary,
                ));
            } else {
                nodes.push(list);
            }
            nodes.push(View::button("Set up Discord", "setup"));
            footer = "Windows notifications · not unread counts";
        }
    }
    let mut result = View::Stack {
        children: nodes,
        gap: Some(2.0),
    }
    .face();
    result.footer = Some(c.notice.unwrap_or(footer).into());
    result.refresh = true;
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn write_golden_faces_and_native_controls() {
        let messages = vec![
            Message {
                id: "100".into(),
                channel_id: "42".into(),
                author: "Ada · Design room".into(),
                content: "The native widget layouts are ready.".into(),
                ..Default::default()
            },
            Message {
                id: "101".into(),
                channel_id: "42".into(),
                author: "Lin · Desktop".into(),
                content: "I left notes on the calendar.".into(),
                ..Default::default()
            },
        ];
        let channels = vec!["42".into(), "43".into()];
        let call = Call {
            channel: "Design room".into(),
            can_control: true,
            participants: vec![
                crate::model::Participant {
                    name: "Alex".into(),
                    speaking: true,
                    ..Default::default()
                },
                crate::model::Participant {
                    name: "Ada".into(),
                    muted: true,
                    ..Default::default()
                },
                crate::model::Participant {
                    name: "Lin".into(),
                    ..Default::default()
                },
            ],
            ..Default::default()
        };
        let mut output = serde_json::Map::new();
        for mode in ["messages", "voice", "setup", "empty", "bot", "readonly"] {
            let choice = Choice {
                voice: mode == "voice" || mode == "readonly",
                setup: mode == "setup",
                channel: "42".into(),
                ..Default::default()
            };
            let mut sizes = serde_json::Map::new();
            for (name, small) in [("small", true), ("medium", false), ("large", false)] {
                let readonly = Call {
                    can_control: false,
                    ..call.clone()
                };
                let f = face(
                    Context {
                        choice: &choice,
                        call: if mode == "readonly" {
                            Some(&readonly)
                        } else {
                            Some(&call)
                        },
                        messages: if mode == "empty" { &[] } else { &messages },
                        channels: &channels,
                        bot: mode == "bot",
                        connected: true,
                        busy: false,
                        notice: None,
                        voice_status: "Connected to Discord RPC",
                    },
                    small,
                );
                let encoded = serde_json::to_string(&f).unwrap();
                if mode == "bot" {
                    assert!(
                        encoded.contains("\"kind\":\"field\"")
                            && encoded.contains("\"kind\":\"select\"")
                    );
                }
                sizes.insert(name.into(), serde_json::to_value(f).unwrap());
            }
            output.insert(mode.into(), serde_json::Value::Object(sizes));
        }
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("golden/faces.json");
        if std::env::var_os("UPDATE_GOLDENS").is_some() {
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(&path, serde_json::to_vec_pretty(&output).unwrap()).unwrap();
        } else {
            let expected: serde_json::Value =
                serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
            assert_eq!(serde_json::Value::Object(output), expected);
        }
    }
}
