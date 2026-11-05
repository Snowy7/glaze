//! The proof plugin (docs/plugins.md): a widget face and a bar module, a top-bar style patch, a
//! timer, actions, the network, and state that survives unloads. `spin` and `crash` exist to
//! test the host's budget and trap isolation.
use std::collections::BTreeMap;

use mypaper_plugin_sdk::guest::{self, Event, Plugin, style, timers, ui};
use mypaper_plugin_sdk::{Role, Tone, View};

#[derive(Default)]
struct Hello {
    clicks: u32,
    tinted: bool,
    ticks: u32,
    fetched: Option<String>,
    /// instance → face id
    open: BTreeMap<String, String>,
}

impl Hello {
    fn draw(&self, instance: &str, face: &str) {
        let view = if face == "clock" {
            View::text(format!("hello · {}", self.ticks), Role::Caption)
        } else {
            View::stack([
                View::text(format!("Clicked {}×", self.clicks), Role::Headline),
                View::tinted(self.fetched.clone().unwrap_or_else(|| "Not fetched yet".into()), Role::Caption, Tone::Secondary),
                View::row([View::button("Click", "click"), View::button(if self.tinted { "Untint bar" } else { "Tint bar" }, "tint"), View::button("Fetch", "fetch")]),
            ])
        };
        let mut face = view.face();
        face.footer = Some(format!("{} s awake", self.ticks));
        ui::set_view(instance, face);
    }

    fn redraw(&self) {
        for (instance, face) in &self.open {
            self.draw(instance, face);
        }
    }

    fn tint(&self) {
        if self.tinted {
            let token = |name: &str, value| style::Token { name: name.into(), value };
            let patch = style::StylePatch {
                surface: style::Surface::TopBar,
                tokens: vec![token("tint", style::TokenValue::Color("#3a6df0".into())), token("tint.opacity", style::TokenValue::Number(0.18))],
            };
            if let Err(e) = style::apply(&patch) {
                guest::log(&format!("tint refused: {e:?}"));
            }
        } else {
            style::reset(style::Surface::TopBar);
        }
    }
}

impl Plugin for Hello {
    fn init(&mut self, _info: &guest::InitInfo, state: Option<&[u8]>) -> Result<(), String> {
        if let Some([a, b, c, d, tinted, ..]) = state {
            self.clicks = u32::from_le_bytes([*a, *b, *c, *d]);
            self.tinted = *tinted != 0;
        }
        if self.tinted {
            self.tint();
        }
        Ok(())
    }

    fn on_event(&mut self, event: Event) {
        match event {
            Event::Open(open) | Event::Resize(open) => {
                if self.open.is_empty() {
                    let _ = timers::every("tick", 1000);
                }
                self.draw(&open.instance, &open.face);
                self.open.insert(open.instance, open.face);
            }
            Event::Close(instance) => {
                self.open.remove(&instance);
                if self.open.is_empty() {
                    timers::cancel("tick");
                }
            }
            Event::Timer(_) => {
                self.ticks += 1;
                self.redraw();
            }
            Event::Action(action) => match action.action.as_str() {
                "click" => {
                    self.clicks += 1;
                    self.redraw();
                }
                "tint" => {
                    self.tinted = !self.tinted;
                    self.tint();
                    self.redraw();
                }
                "fetch" => {
                    self.fetched = Some(match guest::get("https://api.open-meteo.com/v1/forecast?latitude=52.52&longitude=13.41&current=temperature_2m") {
                        Ok(_) => "Fetching…".into(),
                        Err(e) => format!("{e:?}"),
                    });
                    self.redraw();
                }
                "denied" => {
                    self.fetched = Some(match guest::get("https://example.com/") {
                        Ok(_) => "fetch allowed?!".into(),
                        Err(e) => format!("{e:?}"),
                    });
                    self.redraw();
                }
                "noop" => self.redraw(),
                #[allow(clippy::empty_loop)]
                "spin" => loop {},
                "crash" => panic!("asked to crash"),
                _ => {}
            },
            Event::Http(reply) => {
                self.fetched = Some(match reply.outcome {
                    Ok(response) => format!("HTTP {} · {} bytes", response.status, response.body.len()),
                    Err(e) => format!("{e:?}"),
                });
                self.redraw();
            }
            _ => {}
        }
    }

    fn save(&mut self) -> Vec<u8> {
        let mut state = self.clicks.to_le_bytes().to_vec();
        state.push(self.tinted.into());
        state
    }
}

mypaper_plugin_sdk::export!(Hello);
