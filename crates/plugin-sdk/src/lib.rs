//! The mypaper plugin SDK (docs/plugins.md): the only crate a plugin needs for mypaper.
//!
//! A plugin implements [`Plugin`] and calls [`run`]. The loop reads host messages from stdin,
//! fires timers, and delivers the plugin's own background messages (`Ctx::sender`), all as typed
//! [`Event`]s on one thread. The plugin answers with [`Ctx::view`]; the host draws it with the kit.

pub mod manifest;
pub mod protocol;
pub mod view;
#[cfg(target_arch = "wasm32")]
pub mod guest;

pub use manifest::{FaceSpec, Manifest, Permission, allows, compatible};
pub use protocol::HostMessage;
pub use view::{Align, Face, Role, Segment, Series, SizeClass, Tone, View};

use std::collections::{BTreeMap, VecDeque};
use std::io::{BufRead, Write};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::time::{Duration, Instant};

use protocol::{FromHost, HostRequest, ViewUpdate};
use serde_json::{Value, json};

pub const SDK_MAJOR: u32 = 1;
pub const SDK_MINOR: u32 = 0;

/// What reaches the plugin.
#[derive(Debug)]
pub enum Event<M> {
    /// A face of this plugin was placed (or the plugin restarted with it placed).
    Open {
        instance: String,
        face: String,
        size: SizeClass,
        settings: Value,
    },
    Resize { instance: String, size: SizeClass },
    Close { instance: String },
    /// A button (`action`) or a submitted field (`value`).
    Action {
        instance: String,
        action: String,
        value: Option<String>,
    },
    Suspend,
    Resume,
    Timer(&'static str),
    /// Sent from the plugin's own threads through [`Ctx::sender`].
    Msg(M),
}

/// A plugin. `Msg` is what its background threads (sockets, watchers) send to the loop.
pub trait Plugin {
    type Msg: Send + 'static;

    fn on_event(&mut self, ctx: &mut Ctx<Self::Msg>, event: Event<Self::Msg>);

    /// A protocol-1 data method declared in `methods`; unknown by default.
    fn on_call(&mut self, _ctx: &mut Ctx<Self::Msg>, method: &str, _params: Value) -> Result<Value, String> {
        Err(format!("unknown method \"{method}\""))
    }
}

enum Input<M> {
    Line(String),
    Msg(M),
    Closed,
}

/// The plugin's handle on the loop and the host.
pub struct Ctx<M> {
    out: Box<dyn Write>,
    inbox: Receiver<Input<M>>,
    sender: Sender<Input<M>>,
    /// Host messages read while waiting for a reply, delivered next.
    backlog: VecDeque<String>,
    timers: BTreeMap<&'static str, (Instant, Option<Duration>)>,
    next_id: u64,
    closed: bool,
    /// The plugin's section of `integrations.json`, from `initialize`.
    settings: Value,
    /// Each open instance's settings, from its latest `face.open`.
    instances: BTreeMap<String, Value>,
}

/// Posts messages into the plugin's loop from any thread.
pub struct MsgSender<M>(Sender<Input<M>>);

impl<M> Clone for MsgSender<M> {
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}

impl<M> MsgSender<M> {
    /// False once the loop has ended; background threads should stop then.
    pub fn send(&self, msg: M) -> bool {
        self.0.send(Input::Msg(msg)).is_ok()
    }
}

impl<M: Send + 'static> Ctx<M> {
    fn write(&mut self, line: &str) {
        if self.out.write_all(line.as_bytes()).and_then(|_| self.out.flush()).is_err() {
            self.closed = true;
        }
    }

    /// Replaces what `instance` shows.
    pub fn view(&mut self, instance: &str, face: Face) {
        let line = protocol::view(&ViewUpdate {
            instance: instance.into(),
            tree: face,
        });
        self.write(&line);
    }

    /// A protocol-1 `event` notification.
    pub fn emit(&mut self, event: &str, data: Value) {
        let line = json!({ "jsonrpc": "2.0", "method": "event", "params": { "event": event, "data": data } });
        self.write(&(line.to_string() + "\n"));
    }

    pub fn sender(&self) -> MsgSender<M> {
        MsgSender(self.sender.clone())
    }

    /// Fires `Event::Timer(id)` once after `delay`, replacing a timer with the same id.
    pub fn after(&mut self, id: &'static str, delay: Duration) {
        self.timers.insert(id, (Instant::now() + delay, None));
    }

    /// Fires `Event::Timer(id)` every `period`.
    pub fn every(&mut self, id: &'static str, period: Duration) {
        self.timers.insert(id, (Instant::now() + period, Some(period)));
    }

    pub fn cancel(&mut self, id: &'static str) {
        self.timers.remove(id);
    }

    /// Asks the host and waits for its answer (host messages meanwhile are kept for later).
    fn request(&mut self, request: &HostRequest) -> Result<Value, String> {
        self.next_id += 1;
        let id = self.next_id;
        let line = protocol::host_request(id, request);
        self.write(&line);
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            let wait = deadline.saturating_duration_since(Instant::now());
            match self.inbox.recv_timeout(wait) {
                Ok(Input::Line(line)) => match protocol::parse_from_host(&line) {
                    FromHost::Reply { id: got, outcome } if got == id => return outcome,
                    _ => self.backlog.push_back(line),
                },
                // Our own messages wait in order behind the reply.
                Ok(Input::Msg(msg)) => {
                    let _ = self.sender.send(Input::Msg(msg));
                }
                Ok(Input::Closed) | Err(RecvTimeoutError::Disconnected) => {
                    self.closed = true;
                    return Err("the host went away".into());
                }
                Err(RecvTimeoutError::Timeout) => return Err("the host didn't answer".into()),
            }
        }
    }

    /// This plugin's secret `key` from the host's Credential Manager namespace.
    pub fn secret(&mut self, key: &str) -> Result<Option<String>, String> {
        let value = self.request(&HostRequest::GetSecret { key: key.into() })?;
        Ok(value["value"].as_str().map(str::to_owned))
    }

    /// Stores (`Some`) or deletes (`None`) this plugin's secret `key`.
    pub fn set_secret(&mut self, key: &str, value: Option<&str>) -> Result<(), String> {
        self.request(&HostRequest::SetSecret {
            key: key.into(),
            value: value.map(str::to_owned),
        })
        .map(drop)
    }

    /// Opens a URL, file or app through the host (needs `launch`).
    pub fn launch(&mut self, target: &str) -> Result<(), String> {
        self.request(&HostRequest::Launch { target: target.into() }).map(drop)
    }

    /// A toast through the host (needs `notifications`).
    pub fn notify(&mut self, title: &str, body: &str) -> Result<(), String> {
        let request = HostRequest::Notify {
            title: title.into(),
            body: body.into(),
        };
        self.request(&request).map(drop)
    }

    /// Plugin-level settings: the plugin's section of `integrations.json` (`{}` when unset), as
    /// the host sent them in `initialize`.
    pub fn settings(&self) -> &Value {
        &self.settings
    }

    /// `instance`'s own settings from its latest `face.open` (`Null` once it's closed).
    pub fn instance_settings(&self, instance: &str) -> &Value {
        self.instances.get(instance).unwrap_or(&Value::Null)
    }

    fn next_timer(&self) -> Option<(&'static str, Instant)> {
        self.timers.iter().map(|(id, (at, _))| (*id, *at)).min_by_key(|(_, at)| *at)
    }
}

/// Runs `plugin` on stdio until the host says `shutdown` or closes stdin.
pub fn run<P: Plugin>(plugin: P) {
    let (sender, inbox) = mpsc::channel();
    let reader = sender.clone();
    std::thread::spawn(move || {
        for line in std::io::stdin().lock().lines() {
            let Ok(line) = line else { break };
            if reader.send(Input::Line(line)).is_err() {
                return;
            }
        }
        let _ = reader.send(Input::Closed);
    });
    serve(plugin, Box::new(std::io::stdout()), sender, inbox);
}

/// Runs a plugin over in-memory lines (tests, harnesses): returns everything it wrote.
pub fn run_lines<P: Plugin>(plugin: P, lines: &[&str]) -> String {
    #[derive(Clone, Default)]
    struct Sink(std::rc::Rc<std::cell::RefCell<Vec<u8>>>);
    impl Write for Sink {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            self.0.borrow_mut().extend_from_slice(buf);
            Ok(buf.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let (sender, inbox) = mpsc::channel();
    for line in lines {
        let _ = sender.send(Input::Line((*line).into()));
    }
    let _ = sender.send(Input::Closed);
    let sink = Sink::default();
    serve(plugin, Box::new(sink.clone()), sender, inbox);
    let bytes = sink.0.borrow().clone();
    String::from_utf8(bytes).unwrap_or_default()
}

fn serve<P: Plugin>(mut plugin: P, out: Box<dyn Write>, sender: Sender<Input<P::Msg>>, inbox: Receiver<Input<P::Msg>>) {
    let mut ctx = Ctx {
        out,
        inbox,
        sender,
        backlog: VecDeque::new(),
        timers: BTreeMap::new(),
        next_id: 0,
        closed: false,
        settings: json!({}),
        instances: BTreeMap::new(),
    };
    while !ctx.closed {
        let input = if let Some(line) = ctx.backlog.pop_front() {
            Input::Line(line)
        } else {
            let wait = ctx.next_timer().map(|(_, at)| at.saturating_duration_since(Instant::now()));
            match wait.map_or_else(
                || ctx.inbox.recv().map_err(|_| RecvTimeoutError::Disconnected),
                |wait| ctx.inbox.recv_timeout(wait),
            ) {
                Ok(input) => input,
                Err(RecvTimeoutError::Disconnected) => break,
                Err(RecvTimeoutError::Timeout) => {
                    let Some((id, _)) = ctx.next_timer() else { continue };
                    match ctx.timers.get(id).and_then(|(_, every)| *every) {
                        Some(period) => {
                            ctx.timers.insert(id, (Instant::now() + period, Some(period)));
                        }
                        None => {
                            ctx.timers.remove(id);
                        }
                    }
                    plugin.on_event(&mut ctx, Event::Timer(id));
                    continue;
                }
            }
        };
        match input {
            Input::Closed => break,
            Input::Msg(msg) => plugin.on_event(&mut ctx, Event::Msg(msg)),
            Input::Line(line) => match protocol::parse_from_host(&line) {
                FromHost::Message(message) => {
                    match &message {
                        HostMessage::Open { instance, settings, .. } => {
                            ctx.instances.insert(instance.clone(), settings.clone());
                        }
                        HostMessage::Close { instance } => {
                            ctx.instances.remove(instance);
                        }
                        _ => {}
                    }
                    plugin.on_event(&mut ctx, event(message));
                }
                FromHost::Call { id, method, params } => match method.as_str() {
                    "initialize" => {
                        if let Some(settings @ Value::Object(_)) = params.get("settings") {
                            ctx.settings = settings.clone();
                        }
                        let sdk = format!("{SDK_MAJOR}.{SDK_MINOR}");
                        let line = protocol::result(&id, json!({ "sdk": sdk, "protocol": protocol::PROTOCOL }));
                        ctx.write(&line);
                    }
                    "shutdown" => {
                        let line = protocol::result(&id, Value::Null);
                        ctx.write(&line);
                        break;
                    }
                    _ => {
                        let line = match plugin.on_call(&mut ctx, &method, params) {
                            Ok(value) => protocol::result(&id, value),
                            Err(message) => protocol::error(&id, -32601, &message, "NotFound"),
                        };
                        ctx.write(&line);
                    }
                },
                FromHost::Reply { .. } | FromHost::Invalid => {}
            },
        }
    }
}

fn event<M>(message: HostMessage) -> Event<M> {
    match message {
        HostMessage::Open { instance, face, size, settings } => Event::Open { instance, face, size, settings },
        HostMessage::Resize { instance, size } => Event::Resize { instance, size },
        HostMessage::Close { instance } => Event::Close { instance },
        HostMessage::Action { instance, action, value } => Event::Action { instance, action, value },
        HostMessage::Suspend => Event::Suspend,
        HostMessage::Resume => Event::Resume,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Echo;
    impl Plugin for Echo {
        type Msg = ();
        fn on_event(&mut self, ctx: &mut Ctx<()>, event: Event<()>) {
            match event {
                Event::Open { instance, face, .. } => ctx.view(&instance, View::text(face, Role::Headline).face()),
                Event::Action { instance, value, .. } => {
                    ctx.view(&instance, View::text(value.unwrap_or_default(), Role::Body).face())
                }
                _ => {}
            }
        }
    }

    struct Settings;
    impl Plugin for Settings {
        type Msg = ();
        fn on_event(&mut self, ctx: &mut Ctx<()>, event: Event<()>) {
            if let Event::Open { instance, .. } = event {
                let text = format!("{} {}", ctx.settings()["pipe"], ctx.instance_settings(&instance)["variant"]);
                ctx.view(&instance, View::text(text, Role::Body).face());
            }
        }
    }

    #[test]
    fn settings_reach_the_ctx() {
        let out = run_lines(
            Settings,
            &[
                r#"{"jsonrpc":"2.0","id":0,"method":"initialize","params":{"protocol":2,"settings":{"pipe":"p"}}}"#,
                r#"{"jsonrpc":"2.0","method":"face.open","params":{"instance":"a","face":"f","settings":{"variant":"v"}}}"#,
            ],
        );
        let view: Value = serde_json::from_str(out.lines().nth(1).unwrap()).unwrap();
        assert_eq!(view["params"]["tree"]["body"]["text"], r#""p" "v""#);
    }

    #[test]
    fn the_loop_answers_initialize_and_turns_messages_into_views() {
        let out = run_lines(
            Echo,
            &[
                r#"{"jsonrpc":"2.0","id":0,"method":"initialize","params":{"protocol":2}}"#,
                r#"{"jsonrpc":"2.0","method":"face.open","params":{"instance":"a","face":"limits"}}"#,
                r#"{"jsonrpc":"2.0","method":"action","params":{"instance":"a","action":"send","value":"yo"}}"#,
                r#"{"jsonrpc":"2.0","id":1,"method":"forecast"}"#,
                r#"{"jsonrpc":"2.0","id":2,"method":"shutdown"}"#,
                r#"{"jsonrpc":"2.0","method":"face.open","params":{"instance":"late","face":"x"}}"#,
            ],
        );
        let lines: Vec<Value> = out.lines().map(|l| serde_json::from_str(l).unwrap()).collect();
        assert_eq!(lines[0]["result"]["sdk"], "1.0");
        assert_eq!(lines[1]["params"]["tree"]["body"]["text"], "limits");
        assert_eq!(lines[2]["params"]["tree"]["body"]["text"], "yo");
        assert_eq!(lines[3]["error"]["code"], -32601);
        assert_eq!(lines[4]["id"], 2);
        assert_eq!(lines.len(), 5, "nothing after shutdown");
    }
}
