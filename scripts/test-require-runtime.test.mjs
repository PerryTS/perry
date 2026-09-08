// Setup protocol tests only: dummy files are not claimed to be linkable archives.
// Native fixture runs separately exercise compiler/source/Tokio coherence.
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import test from 'node:test';

const helper = new URL('./test-require-runtime.mjs', import.meta.url).href;
const root = fileURLToPath(new URL('../', import.meta.url));
const names = ['runtime', 'stdlib', 'ext_events', 'ext_http', 'ext_net',
  'ext_typescript', 'ext_ws', 'ext_zlib'];
const filename = name => process.platform === 'win32' ? `perry_${name}.lib` : `libperry_${name}.a`;

function fixture(t, omitted) {
  const directory = fs.mkdtempSync(path.join(os.tmpdir(), 'perry-require-setup-'));
  t.after(() => fs.rmSync(directory, { recursive: true, force: true }));
  for (const name of names) {
    if (name !== omitted) fs.writeFileSync(path.join(directory, filename(name)), 'setup test only');
  }
  return directory;
}

function run(directory, changes = {}) {
  const env = { ...process.env, PERRY_TEST_BUILD_RUNTIME: '1',
    PERRY_TEST_RUNTIME_PREBUILT: '1', PERRY_TEST_RUNTIME_PROFILE: 'release',
    PERRY_RUNTIME_DIR: directory, CARGO: path.join(directory, 'cargo-must-not-run'), ...changes };
  if (env.PERRY_RUNTIME_DIR === undefined) delete env.PERRY_RUNTIME_DIR;
  return spawnSync(process.execPath, ['--input-type=module', '-e',
    `import { prepareRequireRuntime } from ${JSON.stringify(helper)}; prepareRequireRuntime(${JSON.stringify(root)});`],
  { env, encoding: 'utf8', timeout: 10_000 });
}

test('prepared mode verifies the complete archive set without a second Cargo build', t => {
  const result = run(fixture(t));
  assert.equal(result.status, 0, `${result.error ?? ''}\n${result.stderr}`);
  assert.match(result.stdout, /Using coherent require-test runtime:/);
  assert.doesNotMatch(result.stdout, /Building coherent/);
});

test('prepared mode rejects a missing provider instead of falling back', t => {
  const result = run(fixture(t, 'ext_net'));
  assert.notEqual(result.status, 0);
  assert.match(result.stderr, /ENOENT/);
  assert(result.stderr.includes(filename('ext_net')), result.stderr);
  assert.doesNotMatch(result.stdout, /Using coherent/);
});

test('prepared mode requires an explicit runtime directory', t => {
  const result = run(fixture(t), { PERRY_RUNTIME_DIR: undefined });
  assert.notEqual(result.status, 0);
  assert.match(result.stderr, /needs an explicit PERRY_RUNTIME_DIR/);
});

test('unwind-enabled debug profiles remain rejected', t => {
  const result = run(fixture(t), { PERRY_TEST_RUNTIME_PROFILE: 'debug' });
  assert.notEqual(result.status, 0);
  assert.match(result.stderr, /panic=abort runtime profile/);
});
