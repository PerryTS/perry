// Application-independent regression for real child-pipe EOF before iteration.
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { spawnSync } from 'node:child_process';

const root = path.dirname(path.dirname(fileURLToPath(import.meta.url)));
const compiler = process.env.PERRY_BIN ?? path.join(root, 'target/perry-dev/perry');
const work = fs.mkdtempSync(path.join(os.tmpdir(), 'perry-child-late-iterator-'));
const source = path.join(root, 'tests/modules/child_output_late_iterator/main.js');
const env = { ...process.env, PERRY_TEST_CHILD_EXECUTABLE: process.execPath };
if (env.PERRY_TEST_WASM === '1' && env.PERRY_RUNTIME_DIR) delete env.PERRY_WORKSPACE_ROOT;
let passed = false;
function run(name, executable, args, timeout, extraEnv = {}) {
  const result = spawnSync(executable, args, { cwd: work, env: { ...env, ...extraEnv },
    encoding: 'utf8', timeout, maxBuffer: 8 * 1024 * 1024 });
  fs.writeFileSync(path.join(work, `${name}.log`), `${result.stdout ?? ''}${result.stderr ?? ''}`);
  if (result.error || result.status !== 0) throw new Error(`${name}: ${result.error ?? result.status}`);
  return result.stdout;
}
try {
  const expected = 'PASS: child stdout/stderr late async iterators\n';
  if (run('node', process.execPath, [source], 15000) !== expected) throw new Error('Node witness missing');
  for (const opt of ['0', 's', 'z']) {
    const output = path.join(work, `native-O${opt}${process.platform === 'win32' ? '.exe' : ''}`);
    run(`compile-O${opt}`, compiler, ['compile', source, '-o', output,
      '--cache-dir', path.join(work, `cache-O${opt}`), '--platform', 'bun',
      '--no-auto-optimize', '--no-color', ...(env.PERRY_TEST_WASM === '1' ? ['--enable-wasm-runtime'] : [])],
      120000, { PERRY_LL_OPT_LEVEL: opt });
    if (run(`native-O${opt}`, output, [], 15000) !== expected) throw new Error(`O${opt} witness mismatch`);
    console.log(`PASS child-output-late-iterator O${opt}`);
  }
  passed = true;
} finally {
  if (passed) fs.rmSync(work, { recursive: true });
  else console.error(`Retained regression diagnostics: ${work}`);
}
