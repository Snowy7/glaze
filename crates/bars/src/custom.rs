//! Custom modules and `exec` actions (docs/bars.md, "Custom modules"). A custom module runs its
//! command every `interval` seconds while a bar on screen shows it, on a worker thread with no
//! console window and a 10 s limit, and shows what it printed: the first line as text, or a
//! JSON object `{ "text", "icon", "tooltip", "class" }`. Only `bars.json` names commands: the
//! page asks for a module's output or its configured action, never for a command line.

use std::collections::{BTreeMap, HashSet};
use std::io::Read;
use std::os::windows::process::CommandExt;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use windows::Win32::System::Threading::CREATE_NO_WINDOW;

use crate::wake::{Msg, Waker};

const TIMEOUT: Duration = Duration::from_secs(10);
/// What a module may print; the rest is cut.
const MAX_OUTPUT: usize = 4096;
const DEFAULT_INTERVAL: f64 = 60.0;

/// What a custom module shows (the `custom` event carries one per id).
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct Output {
    pub text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tooltip: Option<String>,
    /// A CSS class the module gets (`[data-class]`), for themes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub class: Option<String>,
    /// The command failed or timed out.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub error: bool,
}

/// A module's output from what its command printed.
pub fn parse_output(stdout: &str) -> Output {
    let trimmed = stdout.trim();
    if trimmed.starts_with('{')
        && let Ok(mut output) = serde_json::from_str::<Output>(trimmed)
    {
        output.text = output.text.chars().take(200).collect();
        output.class = output.class.filter(|c| {
            c.chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
        });
        output.error = false;
        return output;
    }
    Output {
        text: trimmed
            .lines()
            .next()
            .unwrap_or_default()
            .trim_end()
            .chars()
            .take(200)
            .collect(),
        ..Output::default()
    }
}

/// One module's command.
#[derive(Debug, Clone, PartialEq)]
pub struct Spec {
    pub command: String,
    pub args: Vec<String>,
    pub interval: Duration,
}

impl Spec {
    pub fn new(command: &str, args: &[String], interval: Option<f64>) -> Self {
        Self {
            command: command.to_owned(),
            args: args.to_vec(),
            interval: Duration::from_secs_f64(interval.unwrap_or(DEFAULT_INTERVAL)),
        }
    }
}

/// The modules being run and what they last printed.
pub struct Custom {
    waker: Waker,
    specs: BTreeMap<String, Spec>,
    due: BTreeMap<String, Instant>,
    running: HashSet<String>,
    pub outputs: BTreeMap<String, Output>,
}

impl Custom {
    pub fn new(waker: Waker) -> Self {
        Self {
            waker,
            specs: BTreeMap::new(),
            due: BTreeMap::new(),
            running: HashSet::new(),
            outputs: BTreeMap::new(),
        }
    }

    /// Runs exactly `wanted` (the modules on screen): new or changed ones run now, the rest stop.
    pub fn sync(&mut self, wanted: BTreeMap<String, Spec>) {
        if wanted == self.specs {
            return;
        }
        let now = Instant::now();
        self.due.retain(|id, _| wanted.contains_key(id));
        self.outputs.retain(|id, _| wanted.contains_key(id));
        for (id, spec) in &wanted {
            if self.specs.get(id) != Some(spec) {
                self.due.insert(id.clone(), now);
            }
        }
        self.specs = wanted;
    }

    /// Starts the commands that are due; returns when the next one is.
    pub fn tick(&mut self, now: Instant) -> Option<Instant> {
        let ready: Vec<String> = self
            .due
            .iter()
            .filter(|(id, at)| **at <= now && !self.running.contains(*id))
            .map(|(id, _)| id.clone())
            .collect();
        for id in ready {
            let Some(spec) = self.specs.get(&id).cloned() else {
                continue;
            };
            self.due.insert(id.clone(), now + spec.interval);
            self.running.insert(id.clone());
            let waker = self.waker.clone();
            let spawned = std::thread::Builder::new()
                .name("custom module".into())
                .spawn(move || {
                    let output = run(&spec);
                    waker.send(Msg::Custom(id, output));
                });
            if let Err(e) = spawned {
                log::warn!("custom module: {e}");
            }
        }
        self.due.values().min().copied()
    }

    /// A command finished. Returns whether its module shows something new.
    pub fn finished(&mut self, id: String, output: Output) -> bool {
        self.running.remove(&id);
        if !self.specs.contains_key(&id) || self.outputs.get(&id) == Some(&output) {
            return false;
        }
        self.outputs.insert(id, output);
        true
    }
}

/// Runs a module's command with its time limit and reads what it printed.
fn run(spec: &Spec) -> Output {
    let failed = |why: String| {
        log::info!("custom module {:?}: {why}", spec.command);
        Output {
            text: String::new(),
            tooltip: Some(why),
            error: true,
            ..Output::default()
        }
    };
    let child = Command::new(&spec.command)
        .args(&spec.args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .creation_flags(CREATE_NO_WINDOW.0)
        .spawn();
    let mut child = match child {
        Ok(child) => child,
        Err(e) => return failed(format!("couldn't start: {e}")),
    };
    let Some(mut stdout) = child.stdout.take() else {
        return failed("no output".into());
    };
    let reader = std::thread::spawn(move || {
        let mut bytes = Vec::new();
        let _ = (&mut stdout)
            .take(MAX_OUTPUT as u64)
            .read_to_end(&mut bytes);
        bytes
    });
    let started = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) if started.elapsed() > TIMEOUT => {
                let _ = child.kill();
                let _ = child.wait();
                return failed(format!("took longer than {TIMEOUT:?}"));
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(25)),
            Err(e) => return failed(e.to_string()),
        }
    }
    let bytes = reader.join().unwrap_or_default();
    parse_output(&String::from_utf8_lossy(&bytes))
}

/// An `exec` action: starts the program, detached, with its own window if it has one.
pub fn exec(command: &str, args: &[String]) {
    match Command::new(command)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
    {
        Ok(_) => log::info!("exec {command:?}"),
        Err(e) => log::warn!("exec {command:?}: {e}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn output_is_a_line_or_a_json_object() {
        assert_eq!(parse_output("  21°C  \nmore\n").text, "21°C");
        let json = parse_output(
            r#"{ "text": "CPU 41°", "icon": "temp", "tooltip": "package", "class": "warm" }"#,
        );
        assert_eq!(json.text, "CPU 41°");
        assert_eq!(json.icon.as_deref(), Some("temp"));
        assert_eq!(json.class.as_deref(), Some("warm"));
        let sneaky = parse_output(r#"{ "text": "x", "class": "a\" onclick=\"y" }"#);
        assert_eq!(sneaky.class, None, "classes are plain words");
        assert_eq!(parse_output("{ not json").text, "{ not json");
    }

    #[test]
    fn commands_run_with_a_limit_and_report_failures() {
        let echo = Spec::new("cmd.exe", &["/c".into(), "echo hello".into()], None);
        assert_eq!(run(&echo).text, "hello");
        let missing = Spec::new("definitely-not-a-program-7f3a.exe", &[], None);
        assert!(run(&missing).error);
    }
}
