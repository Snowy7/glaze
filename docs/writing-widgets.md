# Writing native-widget service plugins

Desktop widgets are compiled native views. The former HTML/widget SDK has been removed. Extend
services with a trusted executable speaking JSON-RPC; adding a new visual view requires Rust code
in `crates/shell/src/widgets/` plus a catalog schema in `crates/widgets/src/catalog/`. Studio consumes
that schema for settings. Plugins do not supply HTML or UI code.

## Manifest and installation

Install a folder beside the executable under `plugins/<id>/` or in
`%APPDATA%\mypaper\plugins\<id>/`. User plugins take precedence. Its `plugin.json` is:

```json
{
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

`command` resolves inside the folder first, then on PATH. The process works in that folder, has
piped stdin/stdout/stderr and no console. Plugins run with the user's rights; manifest permissions
are documentation, not a sandbox. Install only trusted plugins. The host puts each process in a
kill-on-close Job object, so child processes cannot outlive the host. Native widget catalog entries
list plugin ids in `plugins`; native code uses `Plugins::call` and `Plugins::subscribe` with typed
`Target` correlation ids. Only declared methods/events are routed.

## Wire contract

One UTF-8 JSON object per line; flush stdout after each line. Keep diagnostic logging on stderr.
JSON-RPC 2.0 request ids are numbers chosen by the host. A first request looks like:

```json
{"jsonrpc":"2.0","id":0,"method":"initialize","params":{"protocol":1,"dataDir":"C:\\Users\\me\\AppData\\Roaming\\mypaper\\plugin-data\\calc"}}
```

Reply within 10 seconds:

```json
{"jsonrpc":"2.0","id":0,"result":{"protocol":1}}
```

The host then forwards declared requests, for example:

```json
{"jsonrpc":"2.0","id":1,"method":"add","params":{"a":2,"b":3}}
{"jsonrpc":"2.0","id":1,"result":5}
```

An error uses `error: { "code": -32602, "message": "expected two numbers" }`. Optional
`error.data.code` can name a `WidgetError` code. Ordinary calls time out after 30 seconds. Emit only
events declared in the manifest, with no request id:

```json
{"jsonrpc":"2.0","method":"event","params":{"event":"added","data":{"total":5}}}
```

The reserved names are `initialize`, `shutdown`, `suspend`, `resume`, and `event`. The host serves
no RPC methods back to a plugin; plugin-initiated requests get method-not-found. Never use stdout
for progress text or non-JSON messages.

## Sleep and shutdown

The host starts a plugin on first use. Suspend/resume notifications have no id and require no reply;
stop polling, network work and event production while suspended. A process with no users stops after
30 seconds. On `shutdown`, reply to the request, flush, and exit; the host allows 2 seconds before
killing it. A crashed subscribed plugin restarts after 2 seconds, doubling to at most 60 seconds.
Cancel outstanding work on shutdown and store cache data only below the supplied `dataDir`.

`plugins/rss` is a stdio-process example; `plugins/weather` also exposes a linked Rust library so
built-in weather needs no child process. Its blocking WinHTTP requests run on the shell's worker,
which sleeps on a channel between fetches. Do not put HTTP or recurring polling in the UI thread.

## Adding a compiled view

Retain `instance`, settings and storage keys when changing a view. Read data through
`mypaper-widgets::storage::Storage`; use logical grid cells in `layout.rs`. Paint a full `Rgba::HIT`
body and native display-list ops. Register explicit semantic hits, schedule only displayed digits,
and stop sampling when hidden. Reuse `mypaper_ui::calendar::MonthGrid` and `TextField` for calendar
and text entry. See [ui.md](ui.md) and [widgets.md](widgets.md) for surfaces and host integration.
