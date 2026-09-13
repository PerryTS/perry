// #10051: independent Node/native oracle, both lexical-scope lowering paths.
// PERRY_BIN and PERRY_RUNTIME_DIR must name one coherent compiler/runtime build.
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { spawnSync } from 'node:child_process';

const root = path.dirname(path.dirname(fileURLToPath(import.meta.url)));
const compiler = process.env.PERRY_BIN ?? path.join(root, 'target/perry-dev/perry');
const work = fs.mkdtempSync(path.join(os.tmpdir(), 'perry-loop-tdz-'));
let passed = false;
try {
  for (const type of ['commonjs', 'module']) {
    const dir = path.join(work, type);
    fs.mkdirSync(dir);
    fs.writeFileSync(path.join(dir, 'package.json'), JSON.stringify({ type }));
    const source = path.join(dir, 'main.ts');
    fs.copyFileSync(path.join(root, 'test-files/test_gap_10051_loop_lexical_tdz.ts'), source);
    const oracle = spawnSync(process.execPath, [source], { cwd: dir, timeout: 15_000 });
    if (oracle.status !== 0) throw new Error(`Node ${type} oracle failed: ${oracle.stderr}`);
    fs.writeFileSync(path.join(dir, 'node.stdout'), oracle.stdout);
    for (const gc of ['default', 'compact']) {
      for (const opt of ['0', 's', 'z']) {
        const label = `${type}-${gc}-O${opt}`;
        const output = path.join(dir, label);
        const env = { ...process.env, PERRY_LL_OPT_LEVEL: opt };
        for (const flag of ['PERRY_RS4GC', 'PERRY_SHADOW_STACK',
          'PERRY_INLINE_SHADOW_SLOT', 'PERRY_FULL_OUTLINE_IC']) delete env[flag];
        if (gc === 'compact') Object.assign(env, {
          PERRY_RS4GC: '0', PERRY_SHADOW_STACK: '1',
          PERRY_INLINE_SHADOW_SLOT: '0', PERRY_FULL_OUTLINE_IC: '1',
        });
        const compile = spawnSync(compiler, ['compile', source, '-o', output,
          '--no-cache', '--no-auto-optimize', '--no-codegen', '--no-color'],
          { cwd: dir, env, timeout: 120_000, maxBuffer: 8 * 1024 * 1024 });
        fs.writeFileSync(output + '.compile.log', Buffer.concat([
          compile.stdout ?? Buffer.alloc(0), compile.stderr ?? Buffer.alloc(0)]));
        if (compile.status !== 0) throw new Error(`${label} compile failed: ${compile.error ?? compile.status}`);
        const run = spawnSync(output, [], { cwd: dir, env, timeout: 15_000 });
        fs.writeFileSync(output + '.stdout', run.stdout ?? Buffer.alloc(0));
        fs.writeFileSync(output + '.stderr', run.stderr ?? Buffer.alloc(0));
        if (run.status !== 0 || !run.stdout?.equals(oracle.stdout)) {
          throw new Error(`${label} native/Node mismatch: ${run.error ?? run.status}\n${run.stdout ?? ''}${run.stderr ?? ''}`);
        }
        console.log(`PASS ${label}`);
      }
    }
  }
  passed = true;
} finally {
  if (passed) fs.rmSync(work, { recursive: true });
  else console.error(`Retained regression diagnostics: ${work}`);
}
