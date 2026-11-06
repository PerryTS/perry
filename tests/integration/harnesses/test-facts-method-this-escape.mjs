
// Standalone linked regression: no application sources or credentials.
// PERRY_BIN=/path/to/perry PERRY_RUNTIME_DIR=/matching/runtime node tests/integration/harnesses/test-facts-method-this-escape.mjs
//
// Per-unit facts: a.js publishes a literal whose method hands `this` to
// `globalThis.zz`, which main.js sets to a function writing `o.n`; main.js
// calls the method, then a.js reads `lit.n` (`show`). Before the fix the
// owner kept `n` numeric per unit (`string:NaN`, and a false-proof abort
// under PERRY_VERIFY_FACTS=1). The program prints `string:str1`
// under node, whole-program facts and per-unit facts alike; with
// PERRY_VERIFY_FACTS=1 neither mode may abort on a false proof of `.n`.
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { spawnSync } from 'node:child_process';

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '../../..');
const compiler = process.env.PERRY_BIN ?? path.join(root, 'target/debug/perry');
const work = fs.mkdtempSync(path.join(os.tmpdir(), 'perry-facts-method-this-escape-'));
let passed = false;
try {
  const fixture = 'facts_method_this_escape';
  const dir = path.join(root, 'tests/integration/modules', fixture);
  const source = path.join(dir, 'main.js');
  const oracle = spawnSync(process.execPath, [source], { encoding: 'utf8', timeout: 15_000 });
  if (oracle.status !== 0) throw new Error('Node oracle failed: ' + oracle.stderr);
  if (oracle.stdout.trim() !== 'string:str1') throw new Error('Unexpected oracle output: ' + oracle.stdout);
  for (const units of ['1', '0']) {
    const output = path.join(work, fixture + '-u' + units);
    const compile = spawnSync(compiler, ['compile', source, '-o', output,
      '--cache-dir', path.join(work, 'cache-u' + units), '--no-color'], {
      cwd: dir,
      env: { ...process.env, PERRY_FACTS_UNITS: units, PERRY_VERIFY_FACTS: '1', PERRY_ALLOW_PERRY_FEATURES: '1' },
      encoding: 'utf8', timeout: 300_000, maxBuffer: 8 * 1024 * 1024,
    });
    fs.writeFileSync(output + '-compile.log', (compile.stdout ?? '') + (compile.stderr ?? ''));
    if (compile.error || compile.status !== 0) {
      throw new Error('Compilation failed (units=' + units + '): ' + (compile.error ?? compile.status) + '\n' +
        (compile.stdout ?? '') + (compile.stderr ?? ''));
    }
    const run = spawnSync(output, [], { cwd: work, encoding: 'utf8', timeout: 15_000 });
    fs.writeFileSync(output + '-run.log', (run.stdout ?? '') + (run.stderr ?? ''));
    if (run.status !== 0 || run.stdout !== oracle.stdout || /PERRY_FALSE_PROOF/.test(run.stderr ?? '')) {
      throw new Error('Native regression failed (units=' + units + '): ' + (run.error ?? run.status) + '\n' +
        (run.stdout ?? '') + (run.stderr ?? ''));
    }
    console.log('PASS units=' + units + ': ' + fixture);
  }
  passed = true;
} finally {
  if (passed) fs.rmSync(work, { recursive: true });
  else console.error('Retained regression diagnostics: ' + work);
}
