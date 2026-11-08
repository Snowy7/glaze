Grok Bot runs as `grok.wasm` in Glaze's shared Wasmtime helper. Build it with
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

Release measurement (2026-11-06, `golden/runtime-memory.txt`): the routines face used 1.06 MiB
of guest linear memory and added 1.20 MiB of process private memory. Cached load took 1.27 ms;
install-time AOT compilation plus first load took 448.5 ms. The benchmark process finished at
8.0 MiB private memory including its shared engine. The component is 284 KiB.

The release workspace was built from the committed `581f522` archive and deployed with
`scripts/deploy-local.ps1` under the desktop lock. A concurrent port deployed a newer desktop
immediately afterward; its runtime successfully validated Grok's cached component. The 20-second
desktop sample is `golden/desktop-memory.json` (shared helper: 7 MiB private, 0% of one CPU core).
No Grok face was placed in the user's layout during that sample; use the component benchmark
above for Grok's active memory cost.
