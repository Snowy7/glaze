#!/usr/bin/env node
import { build } from 'esbuild';
import { spawnSync } from 'node:child_process';
import { mkdir, stat, writeFile } from 'node:fs/promises';
import { resolve, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';
import { parseArgs } from 'node:util';

const { values, positionals } = parseArgs({ allowPositionals: true, options: {
  out: { type: 'string', short: 'o' }, backend: { type: 'string', default: 'quickjs' },
  'allow-high-memory': { type: 'boolean', default: false },
} });
if (positionals.length !== 1 || !values.out) throw new Error('Usage: mypaper-plugin-build <entry.ts> -o <plugin.wasm> [--backend quickjs|starlingmonkey]');
if (!['quickjs', 'starlingmonkey'].includes(values.backend)) throw new Error('Unknown backend');
if (values.backend === 'starlingmonkey' && !values['allow-high-memory']) {
  throw new Error('StarlingMonkey exceeds the current host limits. Experimental builds require --allow-high-memory; see docs/plugins.md#typescript.');
}
const sdk = new URL('../', import.meta.url);
const jco = fileURLToPath(new URL('node_modules/@bytecodealliance/jco/dist/jco.js', sdk));
const wit = fileURLToPath(new URL('wit/', sdk));
const output = resolve(values.out);
if (!output.endsWith('.wasm')) throw new Error('Output must end in .wasm');
const bundle = resolve(dirname(output), '.hello-ts-build', 'plugin.js');
await mkdir(dirname(output), { recursive: true });
await build({ entryPoints: [resolve(positionals[0])], outfile: bundle, bundle: true, format: 'esm', platform: 'neutral', target: 'es2022', external: ['mypaper:*'] });
const flags = values.backend === 'quickjs'
  ? ['--backend', 'quickjs', '--backend-qjs-disable-async', '--backend-qjs-stub-wasi']
  : ['--backend', 'starlingmonkey', '--disable', 'all'];
const result = spawnSync(process.execPath, [jco, 'componentize', bundle, '--wit', wit, '--world-name', 'plugin', ...flags, '-o', output], { stdio: 'inherit' });
if (result.error) throw result.error;
if (result.status !== 0) process.exit(result.status ?? 1);
const metadata = {
  experimental: true, language: 'typescript', engine: values.backend,
  componentBytes: (await stat(output)).size,
  budgetMiB: [0.5, 2],
  warning: values.backend === 'starlingmonkey' ? 'Exceeds current runtime limits; do not install in production.' : 'Prototype; measure each plugin. Uses synchronous WIT capabilities only.',
};
await writeFile(output.replace(/\.wasm$/, '.cost.json'), JSON.stringify(metadata, null, 2) + '\n');
console.log(`${metadata.engine}: ${metadata.componentBytes} bytes; experimental cost metadata written beside component`);
