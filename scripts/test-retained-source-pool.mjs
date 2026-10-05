// Standalone native acceptance: exact Node reflection, live source sharing,
// and retained strings through GC, in default and compact codegen modes.
// Requires PERRY_BIN and a matching PERRY_RUNTIME_DIR; never builds Rust here.
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { spawnSync } from 'node:child_process';
const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const pin = fs.readFileSync(path.join(root, '.node-version'), 'utf8').trim().replace(/^v/, '');
assert.equal(process.versions.node, pin, 'use the pinned Node oracle');
assert(process.env.PERRY_BIN && process.env.PERRY_RUNTIME_DIR, 'explicit matched toolchain required');
assert.equal(process.argv.length, 2, 'no arguments expected');
const work = fs.mkdtempSync(path.join(os.tmpdir(), 'perry-retained-source-'));
const source = path.join(work, 'source.ts');
fs.copyFileSync(path.join(root, 'test-files/test_gap_retained_source_pool.ts'), source);
// The native run sets GC instrument knobs; link the instruments it needs.
const env = { ...process.env, PERRY_LL_OPT_LEVEL: 'z', PERRY_GC_INSTRUMENTS: '1' };
for (const key of ['PERRY_WORKSPACE_ROOT', 'PERRY_LIB_DIR', 'PERRY_FULL_OUTLINE_IC', 'PERRY_SAVE_LL']) delete env[key];
const report = { work, results: [] };
function run(label, executable, args, cwd, overrides = {}, timeout = 15000) {
  const result = spawnSync(executable, args, { cwd, env: { ...env, ...overrides },
    encoding: 'utf8', timeout, killSignal: 'SIGKILL', maxBuffer: 16 * 1024 * 1024 });
  fs.writeFileSync(path.join(work, label + '.stdout'), result.stdout ?? '');
  fs.writeFileSync(path.join(work, label + '.stderr'), result.stderr ?? '');
  assert(!result.error, `${label}: ${result.error}`);
  assert.equal(result.status, 0, `${label}: ${result.stderr}`);
  return result;
}
function llvmFiles(dir) {
  return fs.readdirSync(dir, { withFileTypes: true }).flatMap(entry => {
    const file = path.join(dir, entry.name);
    return entry.isDirectory() ? llvmFiles(file) : entry.name.endsWith('.ll') ? [file] : [];
  });
}
console.log('Evidence: ' + work);
try {
  const oracle = run('node', process.execPath, ['--expose-gc', source], work).stdout;
  for (const mode of ['default', 'compact']) {
    const cwd = path.join(work, mode);
    fs.mkdirSync(cwd);
    const output = path.join(cwd, 'app');
    const settings = mode === 'compact' ? { PERRY_FULL_OUTLINE_IC: '1' } : {};
    run(mode + '-compile', env.PERRY_BIN, ['compile', source, '-o', output,
      '--cache-dir', path.join(cwd, 'cache'), '--trace', 'llvm', '--no-auto-optimize', '--no-color',
      ...(env.PERRY_TEST_WASM === '1' ? ['--platform', 'bun', '--enable-wasm-runtime'] : [])],
    cwd, settings, 120000);
    const candidates = llvmFiles(path.join(cwd, '.perry-trace/llvm')).map(file => fs.readFileSync(file, 'utf8'));
    const ir = candidates.find(text => text.includes('RETAINED_SOURCE_PARENT') &&
      text.includes('call void @js_register_class_source_static('));
    assert(ir, 'fixture source registrations must actually be emitted');
    const sourceCalls = ir.split('\n').filter(line =>
      line.includes('call void @js_register_function_source_static('));
    const sourceBatchCalls = ir.split('\n').filter(line =>
      line.includes('call void @js_register_function_sources_static('));
    const sourceTable = ir.split('\n').find(line =>
      line.startsWith('@__perry_function_source_descriptors_'));
    const sourceDescriptors = sourceTable ? sourceTable.split(
      '{ i32, i32, i32, i32 } { i32 trunc (i64 sub (i64 ptrtoint (ptr @').length - 1 : 0;
    const sourceInfos = ir.split('\n').filter(line => line.includes('$info =') &&
      line.includes('.perry.retained_source') && line.includes('ptrtoint'));
    assert(ir.includes('section ".perry_src"') || ir.includes('section "__TEXT,__perry_src"'),
      'retained source must occupy its own read-only section');
    assert.equal(sourceCalls.length, 0, 'executable source registration must use one batch path');
    assert.equal(sourceBatchCalls.length, 1, 'raw method registration batch must remain live');
    assert.equal(sourceDescriptors, 1, 'only the raw method may occupy the source descriptor table');
    assert(sourceInfos.length >= 2, 'outer and inner must carry relative source info records');
    // Hide the input while executing; reflection must come from the image.
    fs.renameSync(source, source + '.hidden');
    let actual;
    try {
      actual = run(mode + '-native', output, [], cwd, { ...settings,
        PERRY_GC_SCHEDULE_SEED: '7', PERRY_GC_SCHEDULE_RATE: '0.05', PERRY_GC_SCHEDULE_ALLOC_KB: '0',
        PERRY_GC_DIAG: '1', PERRY_GC_VERIFY_EVACUATION: '1', PERRY_GC_PROTECT_FROMSPACE: '1' });
    } finally { fs.renameSync(source + '.hidden', source); }
    assert.equal(actual.stdout, oracle, 'native reflection differs from pinned Node');
    const moving = actual.stderr.match(/\[gc-schedule\] done:.*copying_minors=(\d+) moved_objects=(\d+) loop_polls=(\d+)/);
    assert(moving && moving.slice(1).every(n => Number(n) > 0), 'moving GC must actually execute');
    const row = { mode, sourceBatchCalls: sourceBatchCalls.length, sourceDescriptors,
      sourceInfos: sourceInfos.length,
      executableBytes: fs.statSync(output).size,
      copyingMinors: Number(moving[1]), movedObjects: Number(moving[2]), loopPolls: Number(moving[3]) };
    report.results.push(row);
    console.log('PASS ' + JSON.stringify(row));
  }
  report.passed = true;
} catch (error) {
  report.passed = false; report.error = String(error); process.exitCode = 1; console.error(error);
} finally {
  fs.writeFileSync(path.join(work, 'result.json'), JSON.stringify(report, null, 2) + '\n');
}
