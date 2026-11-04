//! Calls a widget's plugin from Studio: a `place` setting's town search, and the plugin calls of
//! the real widget pages in Studio's live preview. The rules are the widget host's
//! (crates/widgets/src/plugins.rs, docs/writing-plugins.md): the widget must list the plugin in
//! its `plugins`, the plugin must list the method in its `methods`; the process is the plugin's
//! `command` started in its folder, spoken to in JSON-RPC, one object per line. Studio starts one
//! short-lived process per call and closes it once the answer is in.

use std::io::{BufRead, BufReader, Write};
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use serde::Deserialize;
use serde_json::{Value, json};

const PLUGIN_MANIFEST: &str = "plugin.json";
const CREATE_NO_WINDOW: u32 = 0x0800_0000;
const TIMEOUT: Duration = Duration::from_secs(12);
const INIT_ID: u64 = 0;
const CALL_ID: u64 = 1;

#[derive(Debug, Deserialize)]
struct PluginManifest {
    id: String,
    command: String,
    #[serde(default)]
    args: Vec<String>,
    #[serde(default)]
    methods: Vec<String>,
}

/// The plugin folders, as the widget host looks: `%APPDATA%\mypaper\plugins`, then `plugins/`
/// beside the exe or any parent.
fn plugin_roots() -> Vec<PathBuf> {
    let user = mypaper_config::dir().ok().map(|dir| dir.join("plugins"));
    let bundled = std::env::current_exe().ok().and_then(|exe| {
        exe.ancestors()
            .skip(1)
            .map(|dir| dir.join("plugins"))
            .find(|dir| dir.is_dir())
    });
    user.into_iter()
        .chain(bundled)
        .filter(|dir| dir.is_dir())
        .collect()
}

fn find_plugin(id: &str) -> Result<(PluginManifest, PathBuf)> {
    for dir in plugin_roots().into_iter().map(|root| root.join(id)) {
        let path = dir.join(PLUGIN_MANIFEST);
        if let Ok(text) = std::fs::read_to_string(&path) {
            let manifest: PluginManifest =
                serde_json::from_str(&text).with_context(|| format!("in {}", path.display()))?;
            return Ok((manifest, dir));
        }
    }
    bail!("the {id} plugin isn't installed")
}

/// Whether a widget manifest lists `plugin` among its `plugins`.
fn widget_uses(manifest: &Value, plugin: &str) -> bool {
    manifest
        .get("plugins")
        .and_then(Value::as_array)
        .is_some_and(|plugins| plugins.iter().any(|p| p.as_str() == Some(plugin)))
}

/// What a reply line says about the call `id`: its result, its error, or nothing (another message).
fn reply_to(line: &str, id: u64) -> Option<Result<Value>> {
    let message: Value = serde_json::from_str(line).ok()?;
    if message.get("id").and_then(Value::as_u64) != Some(id) {
        return None;
    }
    if let Some(error) = message.get("error") {
        let text = error
            .get("message")
            .and_then(Value::as_str)
            .unwrap_or("the plugin failed");
        return Some(Err(anyhow::anyhow!("{text}")));
    }
    Some(Ok(message.get("result").cloned().unwrap_or(Value::Null)))
}

/// Calls `method` of `plugin` for the widget whose manifest is `widget` (its `widget.json`).
pub fn call(widget: &Value, plugin: &str, method: &str, params: Value) -> Result<Value> {
    if !widget_uses(widget, plugin) {
        bail!("this widget doesn't use the {plugin} plugin");
    }
    let (manifest, dir) = find_plugin(plugin)?;
    if !manifest.methods.iter().any(|m| m == method) {
        bail!("the {plugin} plugin has no {method} method");
    }
    let data_dir = mypaper_config::dir()?
        .join("plugin-data")
        .join(&manifest.id);
    std::fs::create_dir_all(&data_dir)?;
    let local = dir.join(&manifest.command);
    let program = if local.is_file() {
        local
    } else {
        PathBuf::from(&manifest.command)
    };
    let mut child = Command::new(&program)
        .args(&manifest.args)
        .current_dir(&dir)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .creation_flags(CREATE_NO_WINDOW)
        .spawn()
        .with_context(|| format!("starting the {plugin} plugin"))?;
    let answer = talk(&mut child, &manifest.id, &data_dir, method, params);
    stop(child);
    answer
}

fn talk(
    child: &mut Child,
    id: &str,
    data_dir: &Path,
    method: &str,
    params: Value,
) -> Result<Value> {
    let mut stdin = child.stdin.take().context("no stdin")?;
    let stdout = child.stdout.take().context("no stdout")?;
    let (tx, rx) = mpsc::channel::<String>();
    std::thread::spawn(move || {
        for line in BufReader::new(stdout).lines().map_while(Result::ok) {
            if tx.send(line).is_err() {
                break;
            }
        }
    });
    let initialize = json!({ "jsonrpc": "2.0", "id": INIT_ID, "method": "initialize", "params": { "protocol": 1, "plugin": id, "dataDir": data_dir } });
    let request = json!({ "jsonrpc": "2.0", "id": CALL_ID, "method": method, "params": params });
    for message in [initialize, request] {
        writeln!(stdin, "{message}")?;
    }
    stdin.flush()?;
    let deadline = Instant::now() + TIMEOUT;
    loop {
        let left = deadline.saturating_duration_since(Instant::now());
        let line = rx
            .recv_timeout(left)
            .map_err(|_| anyhow::anyhow!("the {id} plugin didn't answer"))?;
        if let Some(answer) = reply_to(&line, CALL_ID) {
            return answer;
        }
    }
}

/// Closing stdin tells a plugin to exit; one that doesn't within a second is ended.
fn stop(mut child: Child) {
    drop(child.stdin.take());
    let deadline = Instant::now() + Duration::from_secs(1);
    while Instant::now() < deadline {
        if matches!(child.try_wait(), Ok(Some(_))) {
            return;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    let _ = child.kill();
    let _ = child.wait();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_widget_may_only_call_the_plugins_it_lists() {
        let weather = json!({ "id": "weather", "plugins": ["weather"] });
        assert!(widget_uses(&weather, "weather"));
        assert!(!widget_uses(&weather, "rss"));
        assert!(!widget_uses(&json!({ "id": "clock" }), "weather"));
        assert!(call(&json!({ "id": "clock" }), "weather", "search", Value::Null).is_err());
    }

    #[test]
    fn only_the_reply_to_the_call_counts() {
        assert!(
            reply_to(
                r#"{"jsonrpc":"2.0","id":0,"result":{"protocol":1}}"#,
                CALL_ID
            )
            .is_none()
        );
        assert!(
            reply_to(
                r#"{"jsonrpc":"2.0","method":"updated","params":{}}"#,
                CALL_ID
            )
            .is_none()
        );
        assert!(reply_to("not json", CALL_ID).is_none());
        let found = reply_to(
            r#"{"jsonrpc":"2.0","id":1,"result":[{"name":"Paris"}]}"#,
            CALL_ID,
        )
        .unwrap()
        .unwrap();
        assert_eq!(found[0]["name"], "Paris");
        let failed = reply_to(
            r#"{"jsonrpc":"2.0","id":1,"error":{"code":-32602,"message":"query is too short"}}"#,
            CALL_ID,
        )
        .unwrap();
        assert_eq!(failed.unwrap_err().to_string(), "query is too short");
    }
}
