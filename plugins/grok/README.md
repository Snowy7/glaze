Grok Bot runs as `grok.wasm` in mypaper's shared Wasmtime helper. Build it with
`powershell -NoProfile -File scripts/build-plugins.ps1`; its Rust workspace is independent of
the desktop workspace.

The faces are `grok--routines`, `grok--recent`, and `grok--prompt`. Existing built-in variants
migrate to those faces while retaining their placement and instance ids. Legacy routine settings
are inherited, and keys retain the `grok/<routine-id>` namespace through the secrets broker.

Connect checks Grok Bot's public desktop-status marker and verifies its process. An existing
sign-in connects without opening Grok Bot. Otherwise Connect opens Grok Bot for sign-in.
Discovery refreshes on open, resume, Connect, and refresh. Prompt is a field that opens a draft
through `grokbot://app/v1/prompt`; the user reviews and sends it in Grok Bot.

Webhook configuration stays in the plugin's Advanced settings. The `routines` field contains
JSON, for example:

```json
[{"id":"morning","title":"Morning brief","url":"https://example.test/routine","input":{}}]
```

Open Advanced from the routines face's ellipsis to choose a routine and save its key. Keys are
stored in Windows Credential Manager, outside settings and saved component state. Permissions
are `launch`, `secrets`, and the exact HTTPS origins in the reviewed routine settings. Redirects
are refused. Requests run only on explicit actions, with a 64 KiB input limit and duplicate
in-flight request protection.

Recent retains up to 16 request receipts. HTTP 200 means accepted; results open in Grok Bot.
Interrupted pending receipts become failed when restored. No response bodies or keys are saved.

Visual evidence is `golden/sizes.png`, rendered by the production shell kit and compared with
`crates/shell/src/widgets/views/integrations/grok-sizes.png` and
`crates/shell/src/integrations/golden/grok-accepted.png`. The prompt field combines the draft
handoff into one action, and routines keep their Advanced key controls in the face.

Verification includes guest validation tests, host hit routing and size bounds at 1×/1.5× DPI,
and execution of the compiled WASM against the real capability brokers. The runtime test
inspects request effects without dispatching them or launching an application.
