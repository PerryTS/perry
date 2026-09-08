// Standalone linked regression: no application sources or credentials.
// PERRY_BIN=/path/to/perry PERRY_RUNTIME_DIR=/matching/runtime node scripts/test-object-array-methods.mjs
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { spawnSync } from 'node:child_process';

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const compiler = process.env.PERRY_BIN ?? path.join(root, 'target/perry-dev/perry');
const work = fs.mkdtempSync(path.join(os.tmpdir(), 'perry-object-array-methods-'));
let passed = false;
try {
  for (const fixture of ["object_array_method_names"]) {
    const source = path.join(root, 'tests/modules', fixture, 'main.js');
    const oracle = spawnSync(process.execPath, ['--expose-gc', source], {
      encoding: 'utf8', timeout: 15_000,
    });
    if (oracle.status !== 0) throw new Error('Node oracle failed: ' + oracle.stderr);
    for (const opt of (process.env.PERRY_TEST_OPT_LEVELS ?? '0,s,z').split(',')) {
      const output = path.join(work, fixture + '-' + opt);
      const compile = spawnSync(compiler, ['compile', source, '-o', output,
        '--cache-dir', path.join(work, 'cache-' + fixture + '-' + opt),
        '--no-auto-optimize', '--no-color',

        ...(process.env.PERRY_TEST_WASM === '1' ? ['--enable-wasm-runtime'] : [])], {
        cwd: work, env: { ...process.env, PERRY_LL_OPT_LEVEL: opt },
        encoding: 'utf8', timeout: 120_000, maxBuffer: 8 * 1024 * 1024,
      });
      fs.writeFileSync(output + '-compile.log', (compile.stdout ?? '') + (compile.stderr ?? ''));
      if (compile.status !== 0) {
        throw new Error('Compilation failed: ' + (compile.error ?? compile.status) + '\n' +
          (compile.stdout ?? '') + (compile.stderr ?? ''));
      }
      const run = spawnSync(output, [], { cwd: work, encoding: 'utf8', timeout: 15_000 });
      fs.writeFileSync(output + '-run.log', (run.stdout ?? '') + (run.stderr ?? ''));
      if (run.status !== 0 || run.stdout !== oracle.stdout) {
        throw new Error('Native regression failed: ' + (run.error ?? run.status) + '\n' +
          (run.stdout ?? '') + (run.stderr ?? ''));
      }
      console.log('PASS O' + opt + ': ' + fixture);
    }
  }
  passed = true;
} finally {
  if (passed) fs.rmSync(work, { recursive: true });
  else console.error('Retained regression diagnostics: ' + work);
}
