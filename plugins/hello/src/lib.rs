//! The proof plugin (docs/plugins.md): a widget face, a bar module that opens a panel, a
//! desktop-menu item and a shortcut (`say-hi`, which raises a toast whose click comes back as
//! an action), window-focus events, a top-bar style patch, a timer, actions, the network, and
//! state that survives unloads. `spin` and `crash` exist to
//! test the host's budget and trap isolation.
use std::collections::BTreeMap;

use mypaper_plugin_sdk::guest::{self, Event, Plugin, events, notify, style, timers, ui};
use mypaper_plugin_sdk::{Role, Tone, View};

#[derive(Default)]
struct Hello {
    clicks: u32,
    tinted: bool,
    ticks: u32,
    fetched: Option<String>,
    /// The process of the last focused window, and how many said hi.
    focused: Option<String>,
    hellos: u32,
    /// instance → face id
    open: BTreeMap<String, String>,
    /// The demo face's `greeting` option, by instance (docs/plugins.md, "Face options").
    greetings: BTreeMap<String, String>,
}

impl Hello {
    fn draw(&self, instance: &str, face: &str) {
        let view = if face == "details" {
            View::stack([
                View::text(format!("Said hi {}×", self.hellos), Role::Headline),
                View::tinted(format!("Focused: {}", self.focused.as_deref().unwrap_or("—")), Role::Caption, Tone::Secondary),
                View::tinted(self.fetched.clone().unwrap_or_default(), Role::Caption, Tone::Tertiary),
                View::row([View::button("Say hi", "say-hi"), View::button(if self.tinted { "Untint bar" } else { "Tint bar" }, "tint")]),
            ])
        } else if face == "clock" {
            // The bar draws the text; a click on the module is its first action (the tint).
            View::row([View::text(format!("hello · {}", self.ticks), Role::Caption), View::button(if self.tinted { "Untint" } else { "Tint" }, "tint")])
        } else {
            View::stack([
                View::text(format!("Clicked {}×", self.clicks), Role::Headline),
                View::tinted(self.fetched.clone().unwrap_or_else(|| "Not fetched yet".into()), Role::Caption, Tone::Secondary),
                self.canvas(),
                View::row([View::button("Click", "click"), View::button(if self.tinted { "Untint bar" } else { "Tint bar" }, "tint"), View::button("Fetch", "fetch")]),
            ])
        };
        let mut face = view.face();
        face.footer = Some(match self.greetings.get(instance) {
            Some(greeting) => format!("{greeting} · {} s awake", self.ticks),
            None => format!("{} s awake", self.ticks),
        });
        ui::set_view(instance, face);
    }

    /// The canvas sample: a track, a dot that moves with the seconds, a wave path and a label.
    fn canvas(&self) -> View {
        use mypaper_plugin_sdk::{Draw, Paint};
        let x = 12.0 + (self.ticks % 10) as f32 * 18.0;
        View::Canvas {
            height: 44.0,
            commands: vec![
                Draw::Rect { x: 0.0, y: 0.0, w: 200.0, h: 44.0, radius: 10.0, paint: Paint::Fill(Tone::Hex("#1d2a4a".into())) },
                Draw::Path { d: "M8 30 C 40 6, 72 54, 104 30 C 136 6, 168 54, 192 30".into(), paint: Paint::Stroke(Tone::Accent, 2.0) },
                Draw::Path { d: "M150 8 l6 12 l-12 0 Z".into(), paint: Paint::Fill(Tone::Warning) },
                Draw::Circle { x, y: 22.0, r: 6.0, paint: Paint::Fill(Tone::Positive) },
                Draw::Circle { x, y: 22.0, r: 9.0, paint: Paint::Stroke(Tone::Label, 1.0) },
                Draw::Text { x: 10.0, y: 4.0, text: "canvas".into(), role: Role::Caption, color: Tone::Secondary },
            ],
        }
    }

    fn say_hi(&mut self) {
        self.hellos += 1;
        let toast = notify::Notification { title: "Hello".into(), body: format!("Hi #{} from the Hello plugin", self.hellos), action: Some("hi".into()) };
        if let Err(e) = notify::show(&toast) {
            guest::log(&format!("toast refused: {e:?}"));
        }
        guest::log(&format!("said hi {}", self.hellos));
        self.redraw();
    }

    /// Keeps the instance's `greeting` option for its footer.
    fn greet(&mut self, open: &guest::FaceOpen) {
        if let Some(greeting) = guest::settings(open)["greeting"].as_str() {
            self.greetings.insert(open.instance.clone(), greeting.to_owned());
        }
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
        if let Err(e) = events::subscribe(&[events::Topic::Windows]) {
            guest::log(&format!("window events refused: {e:?}"));
        }
        Ok(())
    }

    fn on_event(&mut self, event: Event) {
        match event {
            Event::Options(open) => {
                self.greet(&open);
                self.draw(&open.instance, &open.face);
            }
            Event::Open(open) | Event::Resize(open) => {
                self.greet(&open);
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
            Event::Command(id) if id == "say-hi" => self.say_hi(),
            Event::Window(window) if matches!(window.kind, guest::bindings::exports::mypaper::plugin::lifecycle::WindowChange::Focused) => {
                guest::log(&format!("focused {}", window.process));
                self.focused = Some(window.process);
                self.redraw();
            }
            Event::Action(action) if action.instance == "toast" => {
                guest::log(&format!("toast clicked: {}", action.action));
                self.fetched = Some("Toast clicked".into());
                self.redraw();
            }
            Event::Action(action) => match action.action.as_str() {
                "say-hi" => self.say_hi(),
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
