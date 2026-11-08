// Regenerate the path inventory after Studio changes. Consumers below were traced manually;
// line numbers are resolved from source so the report does not retain stale references.
const fs = require('node:fs');
const path = require('node:path');
const { execFileSync } = require('node:child_process');
const root = path.resolve(__dirname, '../../..');
process.chdir(root);
const read = file => fs.readFileSync(file, 'utf8');
function ref(file, needle) {
  const source = read(file), at = source.indexOf(needle);
  if (at < 0) throw new Error(`${file}: missing consumer ${needle}`);
  const line = source.slice(0, at).split('\n').length;
  return `[${file}:${line}](../${file}#L${line})`;
}
const refs = {
  look: () => ref('crates/shell/src/look_settings.rs', 'pub fn appearance('),
  barLook: () => ref('crates/shell/src/look_settings.rs', 'pub fn bar('),
  tokens: () => ref('crates/shell/src/bars/look.rs', 'pub fn resolve_settings('),
  widgets: () => ref('crates/shell/src/widgets/mod.rs', 'let mut global = crate::look::settings::appearance'),
  widgetView: () => ref('crates/shell/src/widgets/views/mod.rs', 'pub fn view('),
  flyouts: () => ref('crates/shell/src/flyouts/look.rs', 'pub fn apply_settings('),
  top: () => ref('crates/shell/src/bars/view.rs', 'pub fn top('),
  dock: () => ref('crates/shell/src/bars/view.rs', 'pub fn dock('),
  bars: () => ref('crates/shell/src/bars/mod.rs', 'fn reconcile('),
  motion: () => ref('crates/shell/src/bars/motion.rs', 'pub fn resolve('),
  styling: () => ref('crates/shell/src/bars/styling.rs', 'pub fn resolve('),
  engine: () => ref('crates/wallpaper/src/control.rs', 'Action::UpdateRenderer =>'),
  menu: () => ref('crates/shell/src/main.rs', 'fn follow_config('),
  tray: () => ref('crates/shell/src/bars/mod.rs', 'self.config.tray.host'),
  clock: () => ref('crates/bars/src/clock.rs', 'let Some(pattern)'),
};
function runtime(p, context = '') {
  if (p.startsWith('/look/windows') || p === '/windows' || p.startsWith('/windows/'))
    return ref('crates/theme/src/windows.rs', 'pub fn resolve(') + '; applied by ' + ref('crates/core/src/styling.rs', 'fn apply(');
  if (p.startsWith('/look/')) {
    if (p === '/look/text/icon_pack') return ref('crates/shell/src/bars/mod.rs', 'self.config.icons.as_deref().or(');
    if (p.startsWith('/look/menus')) return refs.barLook() + '; ' + refs.flyouts();
    if (p.startsWith('/look/widgets')) return ref('crates/shell/src/widgets/mod.rs', 'fn apply_style(') + '; ' + refs.widgets();
    return refs.look() + '; ' + refs.tokens() + '; ' + refs.widgets();
  }
  if (p === '/look') return refs.look() + ' (reset removes the runtime layer)';
  if (p === '/appearance') return context === 'settings' ? ref('crates/studio/src/app.rs', 'fn after_edit') : refs.widgets();
  if (p.startsWith('/engine/')) return refs.engine() + '; ' + ref('crates/config/src/engine.rs', 'pub fn resolve(');
  if (['/paused','/pause_on_battery','/pause_on_fullscreen'].includes(p)) return ref('crates/wallpaper/src/control.rs', 'Action::UpdateHold =>');
  if (p === '/static_snapshot') return ref('crates/wallpaper/src/engine.rs', 'pub fn set_static_snapshot(');
  if (p === '/classic_context_menu') return ref('crates/core/src/desktop_extras.rs', 'extras.apply_classic(');
  if (p === '/experimental/file_index') return ref('crates/core/src/main.rs', 'if config.experimental.file_index &&');
  if (p === '/experimental/app_styler') return ref('crates/core/src/main.rs', 'if config.experimental.app_styler &&');
  if (p === '/experimental/files_default') return ref('crates/core/src/desktop_extras.rs', 'apply_folder_routing(config.experimental.files_default)') + '; ' + ref('crates/shell/src/flyouts/mod.rs', 'glaze_config::Config::load().is_ok_and(|c| c.experimental.files_default)');
  if (p.startsWith('/desktop_menu')) return refs.menu() + '; ' + ref('crates/shell/src/desktop_menu/items.rs', 'pub fn build(');
  if (p === '/taskbar/mode' || p === '/taskbar/alignment') return ref('crates/shell/src/bars/mod.rs', 'fn apply_taskbar(');
  if (p === '/performance' || p.startsWith('/animation/')) return refs.motion() + '; ' + refs.bars();
  if (p === '/overlay' || p.startsWith('/instances')) return ref('crates/shell/src/widgets/mod.rs', 'fn reconcile(') + '; ' + refs.widgetView();
  if (p.startsWith('/launcher/')) return ref('crates/shell/src/flyouts/launcher.rs', 'let config = &cx.bars.config().launcher;') + '; ' + ref('crates/shell/src/flyouts/mod.rs', 'fn follow_win_key(');
  if (p.startsWith('/calendar/')) return ref('crates/shell/src/flyouts/calendar.rs', 'pub fn new(');
  if (p.startsWith('/audio/')) return ref('crates/shell/src/flyouts/mixer.rs', 'let switchable =');
  if (p.startsWith('/tray/')) return refs.tray();
  if (p.startsWith('/notifications/')) return ref('crates/shell/src/integrations/mod.rs', 'state.config.notifications.live_verified');
  if (p.startsWith('/modules/') || p.startsWith('/module_args/')) return ref('crates/shell/src/bars/view.rs', 'fn contents(') + '; ' + ref('crates/shell/src/bars/plugin_modules.rs', 'pub fn place(');
  if (p === '/overrides' || p.startsWith('/overrides/')) return ref('crates/bars/src/config.rs', 'pub fn for_monitor(') + '; ' + refs.bars();
  if (p === '/preset') return ref('crates/bars/src/config.rs', 'pub fn parse(') + '; ' + refs.bars();
  if (p === '/theme') return ref('crates/wallpaper/src/control.rs', 'Action::SwitchTheme(name) =>');
  if (p.startsWith('/theme:') || p.startsWith('/wallpaper/') || p.startsWith('/@both/') || p.startsWith('/widgets/')) return ref('crates/theme/src/lib.rs', 'impl LoadedTheme') + '; ' + refs.look() + ' (theme aliases expand in Studio before writing)';
  if (p === '/recent_wallpapers') return ref('crates/studio/src/pages/wallpapers.rs', '/recent_wallpapers') + ' (Studio library history)';
  if (p === '/first_run_done') return ref('crates/studio/src/app.rs', 'fn first_run_due(') + ' (Studio onboarding state)';
  if (/^\/(top|dock)\//.test(p)) {
    if (p.includes('/animation/')) return refs.motion();
    if (p.includes('/look/indicator/')) return ref('crates/shell/src/bars/view.rs', 'd.look.indicator.attention.as_ref()');
    if (p.includes('/look/items/') || p.includes('/look/border/') || p.includes('/look/shadow/')) return refs.styling() + '; ' + refs.dock();
    if (p.includes('/look/')) return refs.tokens() + '; ' + refs.barLook();
    if (p.includes('/clock/')) return refs.clock() + '; ' + refs.top();
    if (p.includes('hide_when_maximized')) return ref('crates/shell/src/bars/bar.rs', 'pub fn hide_when_maximized(');
    if (p.includes('/preview/')) return ref('crates/shell/src/bars/mod.rs', 'b.config.dock.preview.enabled');
    if (p.endsWith('/autohide')) return ref('crates/shell/src/bars/bar.rs', 'fn auto_hides(');
    if (p.endsWith('/running') || p.endsWith('/split')) return ref('crates/shell/src/bars/services.rs', 'running: config.dock.running,');
    if (p.endsWith('/bounce') || p.endsWith('/attention')) return ref('crates/shell/src/bars/bar.rs', 'pub fn animate_item(');
    if (p.endsWith('/enabled') || p.endsWith('/monitors')) return refs.bars();
    return p.startsWith('/top/') ? refs.top() : refs.dock();
  }
  if (p === '/{}/look/transparent') return refs.tokens() + '; ' + refs.barLook() + ' (top/dock)';
  if (p.includes('{') && (p.startsWith('/{') || p.startsWith('/top/'))) return refs.bars() + '; ' + refs.top() + ' (template expanded by Studio)';
  if (p.startsWith('/plugins/setting/')) return ref('crates/plugins/src/main.rs', 'fn main(') + ' (plugin-owned settings and watcher)';
  if (p.startsWith('/plugins/') || /\/(integration_|new_|list_new|add_|look_scope|palette\/|windows_null|menu_label|menu_command|app_args\/)/.test(p) || p === '/' || p === '/integration_access')
    return 'Studio local state/action or forwarding alias; **not a runtime config path**';
  throw new Error('Unclassified path: ' + p);
}
const source = read('crates/studio/src/settings.rs');
const studioRevision = execFileSync('git', ['log', '-1', '--format=%h', '--', 'crates/studio'], {encoding:'utf8'}).trim();
const settingsPaths = [...source.matchAll(/"(\/[^"\r\n]*)"/g)];
const dead = new Set(['/look/shape/edge','/look/bars/style','/look/menus/style']);
const partial = new Set(['/look/shape/corners','/look/text/font','/look/text/size','/look/text/shadow','/look/text/tone','/look/text/icons']);
const out = [
  '# Studio settings runtime audit', '',
  `Audited 2026-11-06 against the merged Studio, revision \`${studioRevision}\`, including the three new Experimental switches. The regex \`/"(\\/[^"\\r\\n]*)"/g\` finds **${settingsPaths.length}** setting-table paths (the brief counted 106 in an earlier revision). The custom-control table below inventories every distinct slash-prefixed literal in \`pages/*.rs\`, including virtual paths and draft state, so those cannot be mistaken for dead runtime settings. Regenerate with \`node crates/shell/scripts/audit-settings.cjs\` after a Studio merge.`, '',
  '## Findings', '',
  '**Three controls were wholly dead for the native shell:** `/look/shape/edge`, `/look/bars/style`, `/look/menus/style`. **Six more were partial/indirect:** `/look/shape/corners`, `/look/text/font`, `/look/text/size`, `/look/text/shadow`, `/look/text/tone`, `/look/text/icons`. Thus **nine controls needed runtime wiring**. For those six, Studio already projected values to bars.json/widgets.json; saying that no runtime ever applied them would be inaccurate. Direct edits to config.json nevertheless had no shell consumer. Font names were discarded/fell back inconsistently; Fluent/One UI and strip/pill shapes clamped or replaced requested radii; widgets ignored the edge and icon controls; menus/panels inherited bars rather than their own family.', '',
  'All nine now have direct consumers. Existing style/material/appearance/accent/icon-pack controls also resolve directly from config.json on reload. The resolver reads config once per reconciliation; views do no file IO, polling, or recurring restyle work. Specific per-bar/per-monitor radius, font, and border fields and per-widget appearance win. The resolved bar shapes and widget plate_radius feed both rendering/hit masks and glass transport. Bare wallpaper text alone receives tone and text shadow. Text size is the existing 0.5–2 content scale; a projected scale is replaced, never multiplied twice. Lucide uses the kit vectors; Fluent uses the installed system icon font. Missing families fall back to an installed system family.', '',
  'Baseline labels below refer to the code before this task. **PARTIAL** means a Studio projection or subset of surfaces worked. **DEAD** means no native-shell consumer applied it. Runtime references describe the resulting implementation. Windows styling is owned by the concurrent tweaks/core task; this audit cites its resolver and application boundary without modifying it.', '',
  '## Settings table', '',
  '| Document | Path | Baseline | Runtime consumer |', '|---|---|---|---|',
];
for (const m of settingsPaths) {
  const p = m[1], doc = p === '/appearance' ? 'studio.json' : p === '/overlay' ? 'widgets.json' : /^(\/(look|engine|experimental|desktop_menu)\/|\/(static_snapshot|pause_on_battery|pause_on_fullscreen|classic_context_menu)$)/.test(p) ? 'config.json' : 'bars.json';
  out.push(`| ${doc} | \`${p}\` | ${dead.has(p) ? '**DEAD**' : partial.has(p) ? '**PARTIAL**' : 'Live'} | ${runtime(p, 'settings')} |`);
}
out.push('', '## Custom controls and virtual paths', '', '| Source | Literal/template | Consumer or disposition |', '|---|---|---|');
let customCount = 0;
for (const name of fs.readdirSync('crates/studio/src/pages').filter(n => n.endsWith('.rs')).sort()) {
  const file = 'crates/studio/src/pages/' + name;
  const text = read(file), seen = new Set();
  for (const m of text.matchAll(/"(\/[^"\r\n]*)"/g)) {
    const p = m[1]; if (seen.has(p)) continue; seen.add(p); customCount++;
    const line = text.slice(0, m.index).split('\n').length;
    out.push(`| ${name}:${line} | \`${p.replaceAll('|','\\|')}\` | ${runtime(p, name)} |`);
  }
}
out.push('', '## Studio follow-ups', '',
  '- Stop projecting Look into bars.json/widgets.json once Studio uses the runtime-owned layer. The projection has two sources of truth, can freeze former global values into per-family files, and does not project `/look/bars/style` or `/look/menus/style` when those exact fields change. Runtime now reads them itself. Keep the migration/undo implications in mind before removing old projected keys.',
  '- The Corners slider permits 0–32 while config validation permits 0–48. Align the documented range. Defaults shown by the Look page (16 corners, Fluent icons, shadow on) are not necessarily the active theme/design defaults; an unset row should show its resolved/inherited value.',
  '- `/look/text/tone` and `/look/text/shadow` mean bare wallpaper text. Projection currently writes global `appearance/text` and `appearance/shadow`, which also change panel appearance or surface shadow. Keep those separate in Studio previews and resets.',
  '- Custom virtual paths (`/windows_null/*`, `/@both/*`, `/theme:*`, draft form values, plugin actions) are intentionally not runtime JSON pointers. Their expansion/forwarding is recorded above. No Studio files were edited by this task.', '',
  '## Validation and live evidence', '',
  '`cargo test -p glaze-shell --lib -- --test-threads=1 --nocapture`: **203 passed, 10 intentionally ignored**. `cargo test -p glaze-ui --lib`: **71 passed**. Eleven focused Look tests cover the nine controls plus strip/glass shape agreement; the widget shape test also covers media-pill and edit-pill overrides. The native RichEdit test verifies font/size changes without losing text or selection, and that repeating a style schedules no repaint. `cargo check -p glaze-shell -p glaze-ui --examples` and Clippy correctness checks passed. Existing unrelated dead-code/style warnings remain. The release workspace and fixture built in `C:\\glaze-targets\\main`; deployment used only `scripts/deploy-local.ps1`.', '',
  'Additive kit APIs: `Theme::with_surface_radius(self, radius: f32) -> Self`; `Tokens::icon_set: Option<IconSet>` (default `None` preserves legacy glyph rendering); `TextField::set_style(&self, style: &TextStyle, color: Rgba)` (opt-in, cached, preserves input state); `TextField::prepare(&self) -> Result<()>` prepares inactive specimens while blocking focus/capture requests. Existing `WM_CONFIG` notifications drive the new resolution; there is no new config schema or IPC message.', '',
);
const evidence = 'crates/shell/evidence/look-settings';
if (fs.existsSync(evidence + '/verification.json') && fs.existsSync(evidence + '/idle.json')) {
  const v = JSON.parse(read(evidence + '/verification.json').replace(/^\uFEFF/, ''));
  const idle = JSON.parse(read(evidence + '/idle.json').replace(/^\uFEFF/, ''));
  const fixture = idle.rows.find(r => r.part === 'glaze-look-fixture');
  const shell = idle.rows.find(r => r.part === 'glaze-shell');
  if (fixture?.pid !== v.fixture_pid || !v.config_unchanged || !v.same_windows_all_revisions || Object.values(v.visual_changes).some(changed => !changed))
    throw new Error('Live evidence must pass and the measured fixture must match the captured process');
  out.push(`Live fixture: **${v.observations.length} config revisions**, one process, native no-activate windows over an opaque fixture-owned backdrop. Only its config.json changes; APPDATA and LOCALAPPDATA are isolated. The fixture uses production resolvers, bar/dock/widget/menu display lists, config watcher, and the deployed engine's real glass overlays. It does not reserve app bars, change the taskbar, publish the production plate snapshot, or send synthetic input. Screenshots are cropped only to fixture HWNDs, checked against their owning process.`, '',
    '| Measurement | Observed | Gate / interpretation |', '|---|---:|---|',
    `| Settled fixture CPU, 20 s | ${fixture?.cpu_core_pct?.toFixed(2) ?? 'unavailable'}% of one core | No recurring restyle work measured |`,
    `| Fixture private RAM | ${fixture?.private_mb ?? 'unavailable'} MiB | Standalone fixture, not the complete idle fixture |`,
    `| Production shell CPU during sample | ${shell?.cpu_core_pct ?? 'unavailable'}% of one core | The ≤0.1% full idle-fixture gate is **not established** on this active user desktop |`,
    `| Production shell private RAM | ${shell?.private_mb ?? 'unavailable'} MiB | Full fixture budget is ≤40 MiB; this is an uncontrolled desktop sample |`,
    `| User config byte preservation | ${v.config_unchanged ? 'SHA-256 unchanged' : 'FAILED'} | No writes to the user config |`,
    `| Same fixture HWNDs through every revision | ${v.same_windows_all_revisions === true ? 'Yes' : 'Not yet recorded'} | Config updates do not restart surfaces |`, '',
    `Raw results: [verification.json](../${evidence}/verification.json), [revisions.jsonl](../${evidence}/revisions.jsonl), [measure.ps1 sample](../${evidence}/idle.json). The report records the fixture/deployed-shell image hashes and source commit. The fixture exits and its surfaces/overlays are destroyed; the deployed product stays running.`, '',
    '| Control | Before | After |', '|---|---|---|',
    ...[
      ['Corners','01-square-top.png','02-corners-top.png'],['Edge','02-corners-top.png','03-edge-top.png'],
      ['Font','03-edge-widget.png','04-font-widget.png'],['Native input font','03-edge-field.png','04-font-field.png'],['Scale','04-font-widget.png','05-size-widget.png'],
      ['Tone on wallpaper','05-size-bare.png','06-tone-bare.png'],['Text shadow','06-tone-bare.png','07-shadow-bare.png'],
      ['Icon set','07-shadow-menu.png','08-icons-menu.png'],['Bars family style','09-style-top.png','10-bars_style-top.png'],
      ['Menus family style','10-bars_style-menu.png','11-menus_style-menu.png']
    ].map(([name,before,after]) => `| ${name} | [PNG](../${evidence}/${before}) | [PNG](../${evidence}/${after}) |`), '',
    'The glass capture [09-style-widget.png](../' + evidence + '/09-style-widget.png) shows the real backdrop blur inside the same rounded body. The full stock-desktop idle fixture, maximized-app budget, GPU timestamp gates and median-of-three 60 s performance gates were not reproduced: the user remained active and other work/indexing was running. This evidence isolates the cost and visual effect of live restyling; it does not claim those broader product gates passed.', '');
} else {
  out.push('Live evidence has not been captured yet.', '');
}
fs.writeFileSync('docs/settings-audit.md', out.join('\n'));
console.log(JSON.stringify({settings:settingsPaths.length,customLiterals:customCount,whollyDead:dead.size,partial:partial.size,wired:dead.size+partial.size}));
