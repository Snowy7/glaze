# Writing plugins

A plugin is a separate process that widgets can call: for network access with caching, for
anything a web page can't do, or for code in a language you prefer. It is a folder with a
`plugin.json` and an executable, written in any language that can read and write lines on
stdin/stdout. The contract is [widgets.md](widgets.md#plugins); this is the practical guide. The
bundled `plugins/weather` and `plugins/rss` (both Rust) are complete examples.

Plugins are native code with the user's rights. They are **not sandboxed**: the `permissions` in
`plugin.json` are documentation for users, and Studio tells them to install only plugins they
trust.

## 1. The folder

`plugins/<id>/` beside the exe, or `%APPDATA%\mypaper\plugins\<id>\`:

```json
{
  "$schema": "../../packages/widget-sdk/schema/plugin.schema.json",
  "id": "calc",
  "name": "Calculator",
  "version": "1.0.0",
  "command": "calc-plugin.exe",
  "args": [],
  "methods": ["add"],
  "events": ["added"],
  "permissions": []
}
```

- `command` is an executable in the plugin folder, or a program on `PATH` (`python`). The process
  starts with the plugin folder as its working directory, so `"args": ["plugin.py"]` works.
- `methods` is what widgets may call. The host only forwards calls to listed methods. The names
  `initialize`, `shutdown`, `suspend`, `resume` and `event` are reserved.
- `events` is what you may emit.
- A widget lists the plugin ids it uses in its own manifest (`"plugins": ["calc"]`) and then calls
  `mypaper.plugin("calc", "add", { a: 1, b: 2 })` and `mypaper.onPlugin("calc", "added", cb)`.

## 2. The protocol

JSON-RPC 2.0, **one JSON object per line** (UTF-8, `\n`) on stdin and stdout.

- **stdout is for protocol messages only.** Log to **stderr**: it goes to the mypaper log. A stray
  `println!` or `print` that isn't a JSON message is a bug.
- Write each message compact (no pretty printing, no raw newlines inside: JSON strings escape them)
  and **flush** after every line.
- Read until stdin closes. Closing means the host is gone: exit.

### Handshake

The host starts the process on the first call and sends:

```json
{"jsonrpc":"2.0","id":0,"method":"initialize","params":{"protocol":1,"plugin":"calc","dataDir":"…"}}
```

`dataDir` is a folder the plugin may keep files in. Reply with
`{"jsonrpc":"2.0","id":0,"result":{"protocol":1}}`.

### Calls

```json
{"jsonrpc":"2.0","id":7,"method":"add","params":{"a":2,"b":3}}
```

Reply with the same `id` and either `result` or `error`:

```json
{"jsonrpc":"2.0","id":7,"result":5}
{"jsonrpc":"2.0","id":7,"error":{"code":-32602,"message":"a and b must be numbers","data":{"code":"InvalidParams"}}}
```

Calls can overlap: several requests may be outstanding at once, and you may answer in any order,
so don't block the read loop on slow work (the weather plugin answers each call from a thread).

**Errors.** `message` is shown to the widget author (and in logs), not necessarily to the user.
Choose how the widget sees the error with `error.data.code`, one of `PermissionDenied`,
`NotFound`, `InvalidParams`, `PluginUnavailable` (the service you wrap is unreachable) or
`Internal`. Without it: `-32601` becomes `NotFound`, `-32602` becomes `InvalidParams`, anything
else `Internal`. Unparsable input: reply `-32700` with `"id": null`.

### Events

A notification (no `id`) that reaches widgets subscribed with `onPlugin(id, event, cb)`:

```json
{"jsonrpc":"2.0","method":"event","params":{"event":"added","data":{"sum":5}}}
```

### Lifecycle

| message | what to do |
|---|---|
| `suspend` (notification) | The wallpaper is held (fullscreen game, lock, display off, battery, user pause). Stop timers, polling and background refresh. Don't emit events. Keep your state. |
| `resume` (notification) | Start again; refresh anything stale. |
| `shutdown` (request, with `id`) | Reply `result: null`, flush, and exit. You have 2 s, then the process is terminated. |

The host starts a plugin on first use and stops it when no widget needs it (after 30 s idle) or
while it's held. So: **start fast**, don't assume you live long, and persist anything expensive to
`dataDir` (given in `initialize`). Plugins run in a Job object, so they die with the host.

## 3. A minimal plugin in Rust

`Cargo.toml` has `serde_json = "1"`. This is the whole thing (it builds with no warnings under
clippy):

```rust
use std::io::{self, BufRead, Write};

use serde_json::{Value, json};

fn main() {
    let mut stdout = io::stdout().lock();
    let mut send = |message: Value| {
        // One compact JSON object per line, flushed at once.
        let _ = writeln!(stdout, "{message}").and_then(|()| stdout.flush());
    };

    for line in io::stdin().lock().lines() {
        let Ok(line) = line else { break };
        let Ok(msg) = serde_json::from_str::<Value>(&line) else { continue };
        let id = msg.get("id").cloned(); // notifications have none
        let params = &msg["params"];

        let result: Result<Value, (i64, &str)> = match msg["method"].as_str().unwrap_or("") {
            "initialize" => Ok(json!({ "protocol": 1 })),
            "shutdown" => {
                send(json!({ "jsonrpc": "2.0", "id": id, "result": null }));
                break;
            }
            "add" => match (params["a"].as_f64(), params["b"].as_f64()) {
                (Some(a), Some(b)) => Ok(json!(a + b)),
                _ => Err((-32602, "a and b must be numbers")),
            },
            "suspend" | "resume" => continue, // stop and restart your timers here
            _ => Err((-32601, "unknown method")),
        };

        if let Some(id) = id {
            send(match result {
                Ok(result) => json!({ "jsonrpc": "2.0", "id": id, "result": result }),
                Err((code, message)) => {
                    json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message } })
                }
            });
        }
    }
}
```

This one answers on the read loop, which is fine for instant work. For anything that waits (the
network), look at `plugins/weather/src/main.rs`: a reader thread feeds a channel, calls run in
their own threads, and a shared `Out` writer serializes lines under a lock.

## 4. A minimal plugin in Python

`plugin.json`: `"command": "python", "args": ["plugin.py"]`.

```python
import json
import sys


def send(message):
    # One compact JSON object per line, flushed at once.
    print(json.dumps(message), flush=True)


def reply(id, result=None, error=None):
    message = {"jsonrpc": "2.0", "id": id}
    message.update({"error": error} if error else {"result": result})
    send(message)


for line in sys.stdin:  # stdin closing means the host is gone: the loop ends and we exit
    try:
        msg = json.loads(line)
    except json.JSONDecodeError:
        continue
    method, id, params = msg.get("method"), msg.get("id"), msg.get("params") or {}

    if method == "initialize":
        reply(id, {"protocol": 1})
    elif method == "shutdown":
        reply(id, None)
        break
    elif method in ("suspend", "resume"):
        pass  # notifications: no reply. Stop and restart your timers here.
    elif method == "add":
        a, b = params.get("a"), params.get("b")
        if isinstance(a, (int, float)) and isinstance(b, (int, float)):
            reply(id, a + b)
            send({"jsonrpc": "2.0", "method": "event", "params": {"event": "added", "data": {"sum": a + b}}})
        else:
            reply(id, error={"code": -32602, "message": "a and b must be numbers"})
    elif id is not None:
        reply(id, error={"code": -32601, "message": "unknown method"})
```

(`print(..., flush=True)` is what makes it work: Python buffers stdout when it isn't a terminal.)

## 5. Testing a plugin

- **By hand.** Pipe lines into it and read lines out (from a file, so a shell doesn't add a byte
  order mark):
  `weather.exe < input.jsonl`. Check that every request gets a reply, that stdout has nothing but
  JSON, and that `shutdown` exits.
- **From a widget, with the mock harness** (`packages/widget-sdk`, `pnpm dev`): set
  `MYPAPER_PLUGINS` to your folder, set the **Calls go to** option to *real*. It runs your plugin
  like the host does (handshake, `suspend`/`resume` from the **Visible** toggle, 30 s idle stop),
  and the wire log shows each call. Your executable is found next to `plugin.json`, then in
  `target/release` and `target/debug`.
- **Unit tests.** Keep the framing and the mapping of your data separate from I/O (as
  `plugins/weather` does: `rpc.rs`, `weather.rs`, `service.rs` with a fake `Source`) so they are
  testable without a network.

## 6. The bundled weather plugin

`plugins/weather`: Rust, open-meteo (free, no API key), `ureq` with rustls for HTTP.

```text
cargo build --release -p mypaper-plugin-weather
copy target\release\weather.exe plugins\weather\       (next to plugin.json)
cargo nextest run -p mypaper-plugin-weather
```

Methods, both taking `{ lat, lon }` **or** `{ city }` (coordinates win), plus optional
`units` (`"metric"` default, or `"imperial"`):

- `current` returns
  `{ location: { name, country?, lat, lon }, units: { temperature, speed, precipitation }, time, temperature, apparent, humidity, wind, wind_direction, precipitation, code, condition, icon, is_day }`.
- `forecast` also takes `days` (1 to 7, default 5) and returns
  `{ location, units, days: [{ date, code, condition, icon, min, max, precipitation_probability? }], hourly: [{ time, temperature, code, condition, icon, precipitation_probability?, is_day }] }`.
  `days` starts with today. `hourly` is always the next 24 hours (not cut by `days`), starting with
  the current hour: `time` is local to the place (`"2026-11-03T22:00"`), and `icon` is the night
  variant when `is_day` is false (the daily icons are always the daytime ones).

`city` may be `"Paris, Texas"`: the part after the comma picks between matches by country, country
code or region. `icon` is one of `clear-day`, `clear-night`, `partly-cloudy-day`,
`partly-cloudy-night`, `cloudy`, `fog`, `drizzle`, `rain`, `snow`, `thunderstorm`. `code` is the
WMO weather code. Unknown city: `NotFound`. Open-meteo unreachable: `PluginUnavailable`.

Data is cached for **10 minutes** per place and unit system (one request serves `current` and
`forecast`; concurrent calls share a single request; a city is geocoded once per process). While
not suspended, stale entries are refreshed in the background and announced with the event
`updated`, whose data is `{ location, units }`: call `current` / `forecast` again (they're served
from the fresh cache). `suspend` stops the background refresh, `resume` catches up, and
`shutdown` exits cleanly.

## 7. The bundled rss plugin

`plugins/rss`: Rust, `ureq` with rustls for HTTP and `quick-xml` as a light pull parser. It reads
RSS 2.0, RSS 1.0 (RDF) and Atom; the `rss` widget is its client.

```text
cargo build --release -p mypaper-plugin-rss
copy target\release\rss.exe plugins\rss\       (next to plugin.json)
cargo nextest run -p mypaper-plugin-rss
```

One method, `feeds`, taking `{ urls, limit? }`: 1 to 12 `http(s)` URLs, and a limit of 1 to 100
(default 30). It returns the feeds merged, newest first:

```json
{
  "items": [{ "title": "...", "link": "https://...", "source": "Feed title or host", "published": 1793707200, "summary": "plain text, up to 280 chars" }],
  "errors": [{ "url": "https://...", "message": "..." }]
}
```

`published` (unix seconds) and `summary` are left out when the feed has none; items without a date
come last, in feed order. `link` is `""` when the entry has no `http(s)` link, so `javascript:` and
the like never reach a widget. A feed that fails (network, a body over 2 MiB, not a feed) is listed
in `errors` and doesn't fail the call, even when every feed fails. Bad params are `InvalidParams`.

Parsed feeds are cached for **15 minutes** per URL, in memory; a failed refresh serves the stale
copy if there is one. The feeds of one call are fetched in parallel (10 s timeout each). The plugin
starts no background work, so `suspend` and `resume` need no handling. The tests run the service
against a fake fetcher and a fake clock, with feed fixtures (no network).
