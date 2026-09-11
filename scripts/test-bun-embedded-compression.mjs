// Independent native byte/loader regression. No extracted app or Bun install.
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { spawnSync } from 'node:child_process';
import { prepareRequireRuntime } from './test-require-runtime.mjs';
const repo = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
assert.equal(process.versions.node, fs.readFileSync(path.join(repo, '.node-version'), 'utf8').trim().replace(/^v/, ''));
prepareRequireRuntime(repo);
const compiler = process.env.PERRY_BIN ?? path.join(repo, 'target/perry-dev/perry');
const work = fs.mkdtempSync(path.join(os.tmpdir(), 'perry-bun-embedded-compression-'));
const source = path.join(work, 'source');
fs.mkdirSync(source);
const text = 'héllo\0世界\n'.repeat(32768);
const binary = Buffer.alloc(524288);
for (let i = 0; i < binary.length; i++) binary[i] = i % 256;
fs.writeFileSync(path.join(source, 'message.md'), text);
fs.writeFileSync(path.join(source, 'file-only.md'), text);
fs.writeFileSync(path.join(source, 'data.bin'), binary);
fs.writeFileSync(path.join(source, 'empty.txt'), '');
fs.writeFileSync(path.join(source, 'tiny.txt'), 'tiny');
fs.writeFileSync(path.join(source, 'package.json'), '{"type":"module","private":true}');
fs.writeFileSync(path.join(source, 'unbun-manifest.json'), JSON.stringify({
  version: 1, runtime: 'bun', modules: [
    { path: '/$bunfs/root/message.md', extracted_path: 'message.md', loader: 13 },
    { path: '/$bunfs/root/file-only.md', extracted_path: 'file-only.md', loader: 5 },
    { path: '/$bunfs/root/empty.txt', extracted_path: 'empty.txt', loader: 13 },
  ],
}));
fs.writeFileSync(path.join(source, 'entry.js'), `
import { readFileSync, statSync } from 'node:fs';
import { createRequire } from 'node:module';
const native = process.env.PERRY_EMBED_TEST === '1';
const textPath = native ? '/$bunfs/root/message.md' : './message.md';
const dataPath = native ? '/$bunfs/root/data.bin' : './data.bin';
const tinyPath = native ? '/$bunfs/root/tiny.txt' : './tiny.txt';
const emptyPath = native ? '/$bunfs/root/empty.txt' : './empty.txt';
const expected = 'héllo\\0世界\\n'.repeat(32768);
const contents = readFileSync(textPath, 'utf8');
if (contents !== expected) throw new Error('text changed');
if (readFileSync(tinyPath, 'utf8') !== 'tiny') throw new Error('raw tiny asset changed');
if (readFileSync(emptyPath, 'utf8') !== '') throw new Error('empty asset changed');
const bytes = readFileSync(dataPath);
if (bytes.length !== 524288 || statSync(dataPath).size !== 524288) throw new Error('binary size changed');
for (let i = 0; i < bytes.length; i++) if (bytes[i] !== i % 256) throw new Error('binary byte changed: ' + i);
if (native) {
  const load = createRequire(import.meta.url);
  if (load(textPath) !== expected || load(emptyPath) !== '') throw new Error('text loader changed');
  let rejected = false;
  try { load('/$bunfs/root/file-only.md'); } catch (error) { rejected = error.code === 'MODULE_NOT_FOUND'; }
  if (!rejected) throw new Error('file-only loader changed');
  if (await Bun.file(textPath).text() !== expected) throw new Error('Bun.file bytes changed');
  if (Bun.file(dataPath).size !== 524288) throw new Error('Bun.file size changed');
  globalThis.gc();
  if (load(textPath) !== expected || contents !== expected) throw new Error('retained text changed after GC');
}
console.log('PASS: exact compressed/raw/empty bytes, sizes, text/file loaders and retained values');
`);
function run(label, executable, args, cwd, env, timeout) {
  const result = spawnSync(executable, args, { cwd, env, encoding: 'utf8', timeout,
    maxBuffer: 8 * 1024 * 1024 });
  fs.writeFileSync(path.join(work, `${label}.stdout`), result.stdout ?? '');
  fs.writeFileSync(path.join(work, `${label}.stderr`), result.stderr ?? '');
  assert(!result.error, `${label}: ${result.error}`);
  assert.equal(result.status, 0, `${label}\n${result.stdout}\n${result.stderr}`);
  return result.stdout;
}
function tables(directory) {
  return fs.readdirSync(directory, { withFileTypes: true }).flatMap(entry => {
    const filename = path.join(directory, entry.name);
    return entry.isDirectory() ? tables(filename) : entry.name === '__perry_embedded_assets.c' ? [filename] : [];
  });
}
const env = { ...process.env };
delete env.PERRY_EMBED_TEST;
const oracle = run('node', process.execPath, [path.join(source, 'entry.js')], source, env, 15000);
const results = [];
console.log('Evidence: ' + work);
for (const mode of ['default', 'compact']) {
  const staging = path.join(work, mode);
  fs.mkdirSync(staging);
  const settings = { ...env, TMPDIR: staging, PERRY_LL_OPT_LEVEL: 'z' };
  for (const key of ['PERRY_RS4GC', 'PERRY_SHADOW_STACK', 'PERRY_INLINE_SHADOW_SLOT', 'PERRY_FULL_OUTLINE_IC']) delete settings[key];
  if (mode === 'compact') Object.assign(settings, { PERRY_RS4GC: '0', PERRY_SHADOW_STACK: '1',
    PERRY_INLINE_SHADOW_SLOT: '0', PERRY_FULL_OUTLINE_IC: '1' });
  const output = path.join(work, mode + '-app');
  run(mode + '-compile', compiler, ['compile', path.join(source, 'entry.js'),
    '-o', output, '--platform', 'bun', '--bunfs-root', source, '--keep-intermediates',
    '--cache-dir', path.join(staging, 'cache'), '--no-auto-optimize', '--no-color',
    ...(process.env.PERRY_TEST_WASM === '1' ? ['--enable-wasm-runtime'] : [])], source, settings, 120000);
  const emitted = tables(staging);
  assert.equal(emitted.length, 1, 'the actual asset emitter must be retained');
  const c = fs.readFileSync(emitted[0], 'utf8');
  assert.equal((c.match(/if \(!js_register_embedded_zstd_asset\(/g) ?? []).length, 3,
    'three large payloads must actually use compression');
  assert(c.includes('PERRY_ASSET_DATA_'), 'raw tiny/empty payloads must coexist');
  const moved = path.join(work, 'source-away');
  fs.renameSync(source, moved);
  try {
    const actual = run(mode + '-native', output, [], work, { ...settings, PERRY_EMBED_TEST: '1' }, 15000);
    assert.equal(actual, oracle);
  } finally { fs.renameSync(moved, source); }
  results.push({ mode, passed: true, bytes: fs.statSync(output).size, compressedAssets: 3 });
  console.log('PASS: ' + mode);
}
fs.writeFileSync(path.join(work, 'result.json'), JSON.stringify({ results }, null, 2));
