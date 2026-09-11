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

// Exercise the checked-in workflow branch with Cargo stubbed: this proves the
// setup protocol, not that a release archive was built or linked successfully.
function ciSetup(suites, cargoExit = 0) {
  const workflow = fs.readFileSync(path.join(root, '.github/workflows/test.yml'), 'utf8');
  const scoped = workflow.match(/^ {6}- name: Run scoped integration suites\n[\s\S]*?^ {10}status=0$/m);
  assert(scoped, 'scoped integration setup must be present');
  const start = scoped[0].indexOf('          if printf');
  assert(start >= 0, 'runtime selection must be present');
  const setup = scoped[0].slice(start);
  const script = `set -eu
cargo() { printf 'cargo:%s\\n' "$*"; return ${cargoExit}; }
${setup}
printf 'prepared:%s\\nruntime:%s\\n' "\${PERRY_TEST_RUNTIME_PREBUILT-unset}" "\${PERRY_RUNTIME_DIR-unset}"
`;
  const env = Object.fromEntries(Object.entries(process.env).filter(([key]) => !key.startsWith('PERRY_')));
  return spawnSync('bash', ['-c', script], { cwd: root, env: { ...env, SUITES: suites },
    encoding: 'utf8', timeout: 10_000 });
}

test('scoped CI prepares coherent providers for each standalone native consumer', () => {
  for (const suite of ['bun_text_modules', 'import_meta_require_value', 'minsize_inline_policy']) {
    const result = ciSetup(`perry-codegen typed_feedback 300\nperry ${suite} 1500`);
    assert.equal(result.status, 0, result.stderr);
    assert.match(result.stdout, /prepared:1\n/, suite);
    assert(result.stdout.includes(`runtime:${root}target/release\n`), result.stdout);
    const calls = result.stdout.split('\n').filter(line => line.startsWith('cargo:'));
    assert.equal(calls.length, 1, suite);
    assert.match(calls[0], /^cargo:build --release /);
    for (const name of ['perry', 'perry-runtime', 'perry-stdlib', 'perry-runtime-static',
      'perry-stdlib-static', 'perry-ext-events', 'perry-ext-http', 'perry-ext-net',
      'perry-ext-typescript', 'perry-ext-ws', 'perry-ext-zlib']) {
      assert(calls[0].includes(`-p ${name} `), name);
    }
    assert.match(calls[0], /--features perry-stdlib\/external-net-pump$/);
  }
});

test('scoped CI does not mark unrelated or partial runtime setup prepared', () => {
  for (const suites of ['', 'perry-codegen minsize_inline_policy 300',
    'perry minsize_inline_policy_extra 1500', 'perry unrelated 1500']) {
    const result = ciSetup(suites);
    assert.equal(result.status, 0, result.stderr);
    assert.match(result.stdout, /prepared:unset\n/, suites);
    assert.doesNotMatch(result.stdout, /-p perry-ext-/);
  }
});

test('scoped CI propagates provider-build failure before declaring prepared', () => {
  const result = ciSetup('perry minsize_inline_policy 1500', 73);
  assert.equal(result.status, 73, result.stderr);
  assert.doesNotMatch(result.stdout, /^prepared:/m);
});
