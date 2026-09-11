// #10045: application-independent dispatch, context, and fallback regression.
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { spawnSync } from 'node:child_process';
import { prepareRequireRuntime } from './test-require-runtime.mjs';

const root = path.dirname(path.dirname(fileURLToPath(import.meta.url)));
prepareRequireRuntime(root);
const compiler = process.env.PERRY_BIN ?? path.join(root, 'target/perry-dev/perry');
const work = fs.mkdtempSync(path.join(os.tmpdir(), 'perry-async-own-bind-'));
const env = { ...process.env };
if (env.PERRY_TEST_WASM === '1' && env.PERRY_RUNTIME_DIR) delete env.PERRY_WORKSPACE_ROOT;
let passed = false;
function run(name, executable, args, timeout, extraEnv = {}) {
  const result = spawnSync(executable, args, { cwd: work, env: { ...env, ...extraEnv },
    encoding: 'utf8', timeout, maxBuffer: 8 * 1024 * 1024 });
  fs.writeFileSync(path.join(work, `${name}.log`), `${result.stdout ?? ''}${result.stderr ?? ''}`);
  if (result.error || result.status !== 0) {
    throw new Error(`${name}: ${result.error ?? result.status} (signal ${result.signal ?? 'none'})\n` +
      `${result.stdout ?? ''}${result.stderr ?? ''}`);
  }
  return result.stdout;
}
try {
  for (const [fixture, expected] of [
    ['main.ts', 'PASS: own function methods and async context binding\n'],
    ['require.cjs', 'PASS: require AsyncResource.bind\n'],
  ]) {
    const source = path.join(root, 'tests/modules/async_resource_own_bind', fixture);
    if (run(`node-${fixture}`, process.execPath, [source], 15000) !== expected) throw new Error('Node witness missing');
    for (const opt of ['0', 's', 'z']) {
      const label = `${fixture}-O${opt}`;
      const output = path.join(work, `native-${label}${process.platform === 'win32' ? '.exe' : ''}`);
      run(`compile-${label}`, compiler, ['compile', source, '-o', output,
        '--cache-dir', path.join(work, `cache-${label}`), '--platform', 'bun',
        '--no-auto-optimize', '--no-color', ...(env.PERRY_TEST_WASM === '1' ? ['--enable-wasm-runtime'] : [])],
        120000, { PERRY_LL_OPT_LEVEL: opt });
      if (run(`native-${label}`, output, [], 15000) !== expected) throw new Error(`${label} witness mismatch`);
      console.log(`PASS async-resource-own-bind ${label}`);
    }
  }
  passed = true;
} finally {
  if (passed) fs.rmSync(work, { recursive: true });
  else console.error(`Retained regression diagnostics: ${work}`);
}
