// Independent linked regression. No application sources or credentials needed.
// PERRY_BIN=/path/to/perry PERRY_RUNTIME_DIR=/matching/runtime node scripts/test-namespace-getter-binding-identity.mjs
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { spawnSync } from 'node:child_process';

const root = path.dirname(path.dirname(fileURLToPath(import.meta.url)));
const compiler = process.env.PERRY_BIN ?? path.join(root, 'target/perry-dev/perry');
const work = fs.mkdtempSync(path.join(os.tmpdir(), 'perry-namespace-getter-'));
let passed = false;
try {
  for (const fixture of ['renamed_import_namespace_getters', 'export_variable_getter_collision']) {
    const source = path.join(root, `tests/modules/${fixture}/main.js`);
    const oracle = spawnSync(process.execPath, [source], { encoding: 'utf8', timeout: 15_000 });
    if (oracle.status !== 0) throw new Error(`Node oracle failed: ${oracle.stderr}`);
    const cases = ['0', '1'].flatMap(outline => ['0', 's', 'z'].map(opt => [outline, opt]));
    for (const [outline, opt] of cases) {
      const label = `${fixture}-outline${outline}-O${opt}`;
      const output = path.join(work, label);
      const compileEnv = { ...process.env, PERRY_LL_OPT_LEVEL: opt,
        RUST_LOG: 'perry::commands::compile::collect_modules::finish=debug',
        PERRY_OUTLINE_ENTRY: outline, PERRY_OUTLINE_ENTRY_CHUNK_STMTS: '1' };
      // An explicit frozen Wasm archive set must not trigger a workspace
      // rebuild of the optional provider with different feature unification.
      if (process.env.PERRY_TEST_WASM === '1' && process.env.PERRY_RUNTIME_DIR) {
        delete compileEnv.PERRY_WORKSPACE_ROOT;
      }
      const compile = spawnSync(compiler, ['compile', source, '-o', output,
        '--cache-dir', path.join(work, `cache-${label}`), '--no-auto-optimize', '--no-color',
        ...(process.env.PERRY_TEST_WASM === '1' ? ['--enable-wasm-runtime'] : [])],
        { cwd: work, env: compileEnv,
          encoding: 'utf8', timeout: 120_000, maxBuffer: 8 * 1024 * 1024 });
      const compileLog = `${compile.stdout ?? ''}${compile.stderr ?? ''}`;
      fs.writeFileSync(path.join(work, `compile-${label}.log`), compileLog);
      if (compile.status !== 0) throw new Error(`${label} compile failed: ${compile.error ?? compile.status}`);
      if (outline === '1' && fixture === 'renamed_import_namespace_getters'
          && !compileLog.includes("outlined entry body of 'provider.js'")) {
        throw new Error(`${label}: regression did not exercise provider outlining`);
      }
      const run = spawnSync(output, [], { cwd: work, encoding: 'utf8', timeout: 15_000 });
      fs.writeFileSync(path.join(work, `run-${label}.log`), `${run.stdout ?? ''}${run.stderr ?? ''}`);
      if (run.status !== 0 || run.stdout !== oracle.stdout) {
        throw new Error(`${label} native/Node mismatch: ${run.error ?? run.status}\n${run.stdout ?? ''}${run.stderr ?? ''}`);
      }
      console.log(`PASS ${label}`);
    }
  }
  passed = true;
} finally {
  if (passed) fs.rmSync(work, { recursive: true });
  else console.error(`Retained regression diagnostics: ${work}`);
}
