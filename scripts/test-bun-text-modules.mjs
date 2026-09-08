// Standalone Bun text-loader and metadata cache-invalidation regression.
// No Bun installation or extracted application is required.
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { spawnSync } from 'node:child_process';
const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const compiler = process.env.PERRY_BIN ?? path.join(root, 'target/perry-dev/perry');
const work = fs.mkdtempSync(path.join(os.tmpdir(), 'perry-bun-text-'));
const source = path.join(work, 'source');
fs.cpSync(path.join(root, 'tests/modules/bunfs_text_require'), source, { recursive: true });
const manifestPath = path.join(source, 'unbun-manifest.json');
const original = fs.readFileSync(manifestPath);
const output = path.join(work, 'app');
let passed = false;
function compile(label, success = true) {
  // Give each signed executable a fresh inode while retaining identical CLI/cache inputs.
  if (fs.existsSync(output)) fs.unlinkSync(output);
  const result = spawnSync(compiler, ['compile', path.join(source, 'main.js'),
    '-o', output, '--bunfs-root', source, '--platform', 'bun',
    '--cache-dir', path.join(work, 'cache'), '--no-auto-optimize', '--no-color',
    ...(process.env.PERRY_TEST_WASM === '1' ? ['--enable-wasm-runtime'] : [])], {
    cwd: source, env: { ...process.env, PERRY_LL_OPT_LEVEL: 'z' },
    encoding: 'utf8', timeout: 120_000, maxBuffer: 8 * 1024 * 1024,
  });
  const log = (result.stdout ?? '') + (result.stderr ?? '');
  fs.writeFileSync(path.join(work, label + '-compile.log'), log);
  assert(!result.error, String(result.error));
  assert.equal(result.status === 0, success, log);
  if (!success) assert(log.includes('unsupported unbun manifest'), log);
}
function run(label, success = true) {
  const result = spawnSync(output, [], { cwd: work, encoding: 'utf8', timeout: 15_000 });
  const log = (result.stdout ?? '') + (result.stderr ?? '');
  fs.writeFileSync(path.join(work, label + '-run.log'), log);
  assert(!result.error, String(result.error));
  assert.equal(result.status === 0, success, log);
  assert(log.includes(success ? 'PASS embedded text require' :
    "Cannot find module '/$bunfs/root/message.md'"), log);
}
try {
  compile('text');
  run('text');
  const fileOnly = JSON.parse(original);
  for (const module of fileOnly.modules) module.loader = 5;
  fs.writeFileSync(manifestPath, JSON.stringify(fileOnly));
  compile('file-only');
  run('file-only', false);
  fs.writeFileSync(manifestPath, original);
  compile('restored');
  run('restored');
  fs.writeFileSync(manifestPath, JSON.stringify({ version: 99, runtime: 'bun', modules: [] }));
  compile('invalid-manifest', false);
  console.log('PASS: text bytes, resolution, GC, file-only rejection and metadata cache invalidation');
  passed = true;
} finally {
  if (passed) fs.rmSync(work, { recursive: true });
  else console.error('Retained regression diagnostics: ' + work);
}
