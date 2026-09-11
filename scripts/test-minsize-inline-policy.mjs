// Standalone native/IR gate: no application-specific source or patched IR.
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import crypto from 'node:crypto';
import { spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import { prepareRequireRuntime } from './test-require-runtime.mjs';

const root = path.dirname(path.dirname(fileURLToPath(import.meta.url)));
assert.equal(process.versions.node, fs.readFileSync(path.join(root, '.node-version'), 'utf8').trim().replace(/^v/, ''));
prepareRequireRuntime(root);
const compiler = process.env.PERRY_BIN ?? path.join(root, 'target/perry-dev/perry');
const work = fs.mkdtempSync(path.join(os.tmpdir(), 'perry-minsize-inline-'));
const source = path.join(work, 'fixture.ts');
fs.copyFileSync(path.join(root, 'test-files/test_gap_minsize_inline_policy.ts'), source);
const hash = file => crypto.createHash('sha256').update(fs.readFileSync(file)).digest('hex');
const compilerHash = hash(compiler);
const baseEnv = Object.fromEntries(Object.entries(process.env).filter(([key]) => !key.startsWith('PERRY_')));
if (process.env.PERRY_RUNTIME_DIR) baseEnv.PERRY_RUNTIME_DIR = process.env.PERRY_RUNTIME_DIR;
// Optional local toolchains may include wasm-host in their coherent graph.
const wasmArgs = process.env.PERRY_TEST_WASM === '1' ? ['--enable-wasm-runtime'] : [];
const rows = [];
let passed = false;
function run(label, executable, args, timeout, extraEnv = {}) {
  const result = spawnSync(executable, args, { cwd: work, env: { ...baseEnv, ...extraEnv },
    encoding: 'utf8', timeout, killSignal: 'SIGKILL', maxBuffer: 16 * 1024 * 1024 });
  fs.writeFileSync(path.join(work, `${label}.stdout`), result.stdout ?? '');
  fs.writeFileSync(path.join(work, `${label}.stderr`), result.stderr ?? '');
  assert(!result.error && result.status === 0,
    `${label}: ${result.error ?? result.status}; signal=${result.signal}\n${result.stderr?.slice(-2000)}`);
  return result;
}
try {
  const oracle = run('node', process.execPath, [source], 20000).stdout;
  assert(oracle.includes('inline-policy-native-complete'), 'Node must finish');
  for (const roots of ['shadow', 'native']) {
    for (const transport of ['1', 'native']) {
      for (const opt of ['s', 'z']) {
        const label = `${roots}-${transport}-O${opt}`;
        const output = path.join(work, `${label}${process.platform === 'win32' ? '.exe' : ''}`);
        const irDir = path.join(work, `${label}-ir`);
        fs.mkdirSync(irDir);
        const compileEnv = { PERRY_LL_OPT_LEVEL: opt, PERRY_LLVM_INPROCESS: transport,
          PERRY_FULL_OUTLINE_IC: '1', PERRY_KEEP_SYMBOLS: '1', PERRY_SAVE_LL: irDir,
          PERRY_MODULE_JOBS: '1', PERRY_CODEGEN_UNIT_JOBS: '1',
          ...(roots === 'shadow' ? { PERRY_RS4GC: '0', PERRY_SHADOW_STACK: '1', PERRY_INLINE_SHADOW_SLOT: '0' } : {}),
        };
        run(`compile-${label}`, compiler, ['compile', source, '-o', output,
          '--cache-dir', path.join(work, `cache-${label}`), '--no-auto-optimize', '--no-color', ...wasmArgs],
          120000, compileEnv);
        const files = fs.readdirSync(irDir).filter(name => name.endsWith('.ll'));
        assert.equal(files.length, 1, 'one complete fixture module must be retained');
        const ir = fs.readFileSync(path.join(irDir, files[0]), 'utf8');
        const headers = ir.split('\n').filter(line =>
          /^define .*@perry_fn_[^(]*__(?:identityLeaf|throwLeaf)\(/.test(line));
        assert.equal(headers.length, 2, 'both ordinary forced-inline witnesses must be present');
        for (const header of headers) {
          assert.equal(header.includes(' minsize'), opt === 'z', header);
          assert.equal(header.includes(' alwaysinline'), roots === 'shadow' && opt === 's', header);
          assert.equal(header.includes(' inlinehint'), roots === 'native', header);
        }
        const strategy = ir.includes('gc "statepoint-example"');
        assert.equal(strategy, roots === 'native', 'requested root mode must actually be emitted');
        // Native execution must not load the source to satisfy the oracle.
        fs.renameSync(source, source + '.hidden');
        try {
          for (const moving of [false, true]) {
            const runLabel = `${label}-${moving ? 'moving' : 'ordinary'}`;
            const actual = run(runLabel, output, [], 20000, moving ? {
              PERRY_GC_SCHEDULE_SEED: '7', PERRY_GC_SCHEDULE_RATE: '0.05', PERRY_GC_SCHEDULE_ALLOC_KB: '0',
              PERRY_GC_DIAG: '1', PERRY_GC_VERIFY_EVACUATION: '1', PERRY_GC_PROTECT_FROMSPACE: '1',
            } : {});
            assert.equal(actual.stdout, oracle, `${runLabel}: exact Node mismatch`);
            const counters = actual.stderr.match(/\[gc-schedule\] done:.*copying_minors=(\d+) moved_objects=(\d+) loop_polls=(\d+)/);
            if (moving) assert(counters && counters.slice(1).every(n => Number(n) > 0), 'collection must actually move objects');
            const row = { roots, transport, opt, moving, bytes: fs.statSync(output).size,
              sha256: hash(output), copyingMinors: Number(counters?.[1] ?? 0),
              movedObjects: Number(counters?.[2] ?? 0), loopPolls: Number(counters?.[3] ?? 0) };
            rows.push(row); console.log('PASS minsize-inline ' + JSON.stringify(row));
          }
        } finally { fs.renameSync(source + '.hidden', source); }
      }
    }
  }
  assert.equal(rows.length, 16);
  assert.equal(hash(compiler), compilerHash, 'compiler must not change during validation');
  passed = true;
} finally {
  fs.writeFileSync(path.join(work, 'result.json'), JSON.stringify({ passed, compiler, compilerHash, rows }, null, 2) + '\n');
  console.log(`Retained minsize-inline evidence: ${work}`);
}
