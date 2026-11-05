// Copy the two tiny contract crates so unrelated in-progress workspace members cannot block
// this read-only diagnostic. Production runtime/capability sources are included at build time.
import { mkdir, readFile, writeFile } from 'node:fs/promises';
import { spawnSync } from 'node:child_process';
const root = new URL('../../../', import.meta.url);
const git = (...args) => {
  const result = spawnSync('git', args, { cwd: root, encoding: 'utf8' });
  if (result.status !== 0) throw new Error(result.stderr);
  return result.stdout;
};
const revision = process.argv[2] ?? git('rev-parse', 'HEAD').trim();
async function writeChanged(path, text) {
  if (await readFile(path, 'utf8').catch(() => undefined) !== text) await writeFile(path, text);
}
const paths = git('ls-tree', '-r', '--name-only', revision, 'crates/plugin-sdk/src', 'crates/plugin-api/src', 'crates/plugin-api/wit', 'crates/plugins/src/runtime.rs', 'crates/plugins/src/caps.rs', 'plugins/hello').trim().split('\n');
for (const path of paths) {
  const relative = path.replace('crates/plugins/src/', '').replace('crates/', '').replace('plugins/hello/', 'hello/');
  const output = new URL(`.source/${relative}`, import.meta.url);
  await mkdir(new URL('./', output), { recursive: true });
  let content = git('show', `${revision}:${path}`);
  if (path === 'plugins/hello/Cargo.toml') content = content.replace('../../crates/plugin-sdk', '../plugin-sdk');
  await writeChanged(output, content);
}
await writeChanged(new URL('.source/revision.txt', import.meta.url), revision + '\n');
for (const name of ['plugin-sdk', 'plugin-api']) {
  const dest = new URL(`.source/${name}/`, import.meta.url);
  await mkdir(dest, { recursive: true });
  await writeChanged(new URL('Cargo.toml', dest), `[package]\nname = "mypaper-${name}"\nversion = "0.1.0"\nedition = "2024"\n[dependencies]\nserde = { version = "1", features = ["derive"] }\nserde_json = "1"\n${name === 'plugin-api' ? 'postcard = { version = "1", default-features = false, features = ["use-std"] }\nmypaper-plugin-sdk = { path = "../plugin-sdk" }\n' : '[target.\'cfg(target_arch = "wasm32")\'.dependencies]\nwit-bindgen = { version = "0.57", default-features = false, features = ["macros", "realloc"] }\n'}`);
}
