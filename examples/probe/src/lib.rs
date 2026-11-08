//! The capability probe: a test plugin for `crates/plugins` (its tests build it for
//! wasm32-wasip2 and install it with the permissions each test needs). Every action exercises
//! one capability (`value` is its argument), and every result or event is appended to the face
//! as a line, which the tests read back.
use glaze_plugin_sdk::guest::{Error, Event, Plugin, events, http, native, notify, pipes, system, ui, websocket};
use glaze_plugin_sdk::{Role, View};

#[derive(Default)]
struct Probe {
    instance: Option<String>,
    lines: Vec<String>,
    socket: u64,
    pipe: u64,
}

fn id(what: &str, result: Result<u64, Error>) -> (String, u64) {
    match result {
        Ok(id) => (format!("{what} id {id}"), id),
        Err(e) => (format!("{what} err {e:?}"), 0),
    }
}

fn topic(name: &str) -> Option<events::Topic> {
    use events::Topic::*;
    Some(match name {
        "windows" => Windows,
        "monitors" => Monitors,
        "notifications" => Notifications,
        "media" => Media,
        "power" => Power,
        "theme" => Theme,
        _ => return None,
    })
}

impl Probe {
    fn log(&mut self, line: String) {
        self.lines.push(line);
        if self.lines.len() > 80 {
            self.lines.remove(0);
        }
        if let Some(instance) = &self.instance {
            ui::set_view(instance, View::stack(self.lines.iter().map(|l| View::text(l.clone(), Role::Caption))).face());
        }
    }

    fn act(&mut self, action: &str, value: &str) -> String {
        match action {
            "ws" => {
                let auth = http::Header { name: "Authorization".into(), value: "Bearer probe".into() };
                let request = websocket::ConnectRequest { url: value.into(), headers: vec![auth], protocols: vec!["obswebsocket.json".into()] };
                let (line, id) = id("ws", websocket::connect_with(&request));
                self.socket = id;
                line
            }
            "ws-send" => format!("ws-send {:?}", websocket::send(self.socket, &websocket::Message::Text(value.into()))),
            "ws-binary" => format!("ws-binary {:?}", websocket::send(self.socket, &websocket::Message::Binary(value.as_bytes().to_vec()))),
            "ws-close" => {
                websocket::close(self.socket);
                "ws-close".into()
            }
            "pipe" => {
                let (line, id) = id("pipe", pipes::open(value));
                self.pipe = id;
                line
            }
            "pipe-write" => format!("pipe-write {:?}", pipes::write(self.pipe, value.as_bytes())),
            "pipe-close" => {
                pipes::close(self.pipe);
                "pipe-close".into()
            }
            "pipe-http" => {
                let (name, url) = value.split_once('|').unwrap_or((value, "http://localhost/"));
                let request = http::Request { method: http::Method::Get, url: url.into(), headers: Vec::new(), body: None, timeout_ms: None };
                id("pipe-http", pipes::http_request(name, &request)).0
            }
            "notify" => format!("notify {:?}", notify::show(&notify::Notification { title: "Probe".into(), body: value.into(), action: None })),
            "subscribe" => format!("subscribe {:?}", events::subscribe(&value.split(',').filter_map(topic).collect::<Vec<_>>())),
            "image" => format!("image {:?}", ui::put_image(2, 2, &[255; 16])),
            "image-bad" => format!("image {:?}", ui::put_image(2, 2, &[0; 3])),
            "foreground" => format!("foreground {:?}", system::foreground().map(|w| w.map(|w| w.process))),
            "native" => id("native", native::call("native_echo.dll", value, b"hi")).0,
            other => format!("unknown {other}"),
        }
    }
}

impl Plugin for Probe {
    fn on_event(&mut self, event: Event) {
        use websocket::{Message, SocketEvent};
        let line = match event {
            Event::Open(open) => {
                self.instance = Some(open.instance);
                "open".into()
            }
            Event::Action(action) => self.act(&action.action, action.value.as_deref().unwrap_or_default()),
            Event::Socket(SocketEvent::Opened(id)) => format!("ws opened {id}"),
            Event::Socket(SocketEvent::Message((_, Message::Text(text)))) => format!("ws text {text}"),
            Event::Socket(SocketEvent::Message((_, Message::Binary(bytes)))) => format!("ws binary {}", bytes.len()),
            Event::Socket(SocketEvent::Closed((_, why))) => format!("ws closed {why:?}"),
            Event::Pipe(pipes::PipeEvent::Data((_, bytes))) => format!("pipe data {}", String::from_utf8_lossy(&bytes)),
            Event::Pipe(pipes::PipeEvent::Closed(_)) => "pipe closed".into(),
            Event::Http(reply) => match reply.outcome {
                Ok(response) => format!("http {} {}", response.status, String::from_utf8_lossy(&response.body)),
                Err(e) => format!("http err {e:?}"),
            },
            Event::Native(reply) => match reply.outcome {
                Ok(bytes) => format!("native ok {}", String::from_utf8_lossy(&bytes)),
                Err(e) => format!("native err {e:?}"),
            },
            Event::NativeEvent(message) => format!("native event {} {}", message.name, String::from_utf8_lossy(&message.bytes)),
            Event::Window(window) => format!("window {:?} {} {}", window.kind, window.id, window.title),
            Event::Monitors => "monitors".into(),
            Event::Notification(json) => format!("notification {json}"),
            Event::Media(json) => format!("media {json}"),
            Event::Power => "power".into(),
            Event::Theme => "theme".into(),
            _ => return,
        };
        self.log(line);
    }
}

glaze_plugin_sdk::export!(Probe);
