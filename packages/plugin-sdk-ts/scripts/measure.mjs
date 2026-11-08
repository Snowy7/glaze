// Repeatable headless comparison against the same immutable Glaze runtime sources.
import { spawnSync } from 'node:child_process';
import { cp, mkdir, readFile, writeFile } from 'node:fs/promises';
import { fileURLToPath } from 'node:url';
import { resolve } from 'node:path';
import { parseArgs } from 'node:util';
import { createHash } from 'node:crypto';
import { cpus, release } from 'node:os';

const sdk = new URL('../', import.meta.url);
const { values } = parseArgs({ options: {
  'rust-component': { type: 'string' },
  runs: { type: 'string', default: '3' },
} });
const runs = Number(values.runs);
if (!Number.isInteger(runs) || runs < 1 || runs > 10) throw new Error('--runs must be 1..10');
function command(program, args, options = {}) {
  const result = spawnSync(program, args, { cwd: sdk, encoding: 'utf8', windowsHide: true, timeout: 180_000, ...options });
  if (result.error) throw result.error;
  return result;
}
function check(program, args, options) {
  const result = command(program, args, options);
  if (result.status !== 0) throw new Error(result.stderr || result.stdout);
  return result.stdout;
}
const revision = (await readFile(new URL('bench/BASELINE', sdk), 'utf8')).trim();
check(process.execPath, ['bench/prepare.mjs', revision]);
if (!values['rust-component']) {
  check('cargo', ['build', '--release', '--locked', '--target', 'wasm32-wasip2', '--manifest-path', 'bench/.source/hello/Cargo.toml', '--target-dir', 'bench/target/guest']);
}
check('cargo', ['build', '--release', '--locked', '--manifest-path', 'bench/Cargo.toml', '--target-dir', 'bench/target'], {
  env: { ...process.env, GLAZE_PROBE_MEMORY_MB: '32' },
});

const fixtures = [
  { id: 'hello', source: null },
  { id: 'starling', source: 'test/minimal.ts', backend: 'starlingmonkey' },
  { id: 'quickjs-minimal', source: 'test/minimal.ts', backend: 'quickjs' },
  { id: 'quickjs-sdk', source: '../../plugins/hello-ts/src/index.ts', backend: 'quickjs' },
  { id: 'abi', source: 'test/abi.ts', backend: 'quickjs' },
];
for (const fixture of fixtures) {
  const folder = new URL(`measurements/stock/${fixture.id}/`, sdk);
  await mkdir(folder, { recursive: true });
  const manifest = { id: fixture.id, component: `${fixture.id}.wasm`, api: '0.1', faces: [{ id: 'main' }], permissions: fixture.id === 'abi' ? ['storage'] : [] };
  await writeFile(new URL('plugin.json', folder), JSON.stringify(manifest, null, 2) + '\n');
  if (fixture.source) {
    check(process.execPath, ['scripts/build.mjs', fixture.source, '-o', `measurements/stock/${fixture.id}/${fixture.id}.wasm`, '--backend', fixture.backend, ...(fixture.backend === 'starlingmonkey' ? ['--allow-high-memory'] : [])]);
  } else {
    await cp(resolve(fileURLToPath(sdk), values['rust-component'] ?? 'bench/target/guest/wasm32-wasip2/release/hello.wasm'), new URL('hello.wasm', folder));
  }
}
const data = {
  measuredAt: new Date().toISOString(), revision, platform: process.platform, os: release(),
  cpu: cpus()[0]?.model, node: process.version, runs,
  probeOverrides: { maxCoreInstanceBytes: 512 * 1024, maxComponentInstanceBytes: 512 * 1024, linearMemoryCapBytes: 32 * 1024 * 1024 },
  fixtures: {},
};
const probe = fileURLToPath(new URL('bench/target/release/glaze-ts-probe.exe', sdk));
for (const fixture of fixtures) {
  const samples = [];
  for (let run = 0; run < (fixture.id === 'abi' ? 1 : runs); run++) {
    const output = check(probe, [`measurements/stock/${fixture.id}`]);
    await writeFile(new URL(`measurements/${fixture.id}-${run + 1}.txt`, sdk), output);
    const sample = Object.fromEntries(output.trim().split(/\r?\n/).map(line => {
      const [key, value] = line.split('=');
      return [key, Number(value)];
    }));
    samples.push(sample);
    console.log(`${fixture.id} ${run + 1}: ${sample.open_linear_bytes} bytes, cold ${sample.cold_load_ms} ms, event p50 ${sample.event_p50_us} us`);
  }
  const median = Object.fromEntries(Object.keys(samples[0]).map(key => {
    const ordered = samples.map(sample => sample[key]).sort((a, b) => a - b);
    return [key, ordered[Math.floor(ordered.length / 2)]];
  }));
  const wasm = await readFile(new URL(`measurements/stock/${fixture.id}/${fixture.id}.wasm`, sdk));
  data.fixtures[fixture.id] = { sha256: createHash('sha256').update(wasm).digest('hex'), samples, median };
}
// Compare the finished SDK and StarlingMonkey against the actual production helper too.
const helper = fileURLToPath(new URL('../../target/release/glaze-plugins.exe', sdk));
data.production = {};
for (const id of ['starling', 'quickjs-sdk']) {
  const result = command(helper, ['--bench', `measurements/stock/${id}`]);
  data.production[id] = { exitCode: result.status, stdout: result.stdout, stderr: result.stderr };
}
await writeFile(new URL('measurements/results.json', sdk), JSON.stringify(data, null, 2) + '\n');
