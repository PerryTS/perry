// Independent CLI regression: complete native units survive a missing module
// object; a missing unit is regenerated; cold, resumed and uncached output agree.
// Usage: node scripts/test-codegen-unit-resume.mjs /absolute/path/to/perry
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { spawnSync } from 'node:child_process';

const compiler = path.resolve(process.argv[2]);
const sourceRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const fixture = path.join(sourceRoot, 'tests/modules/codegen_unit_resume/main.ts');
const scratch = fs.mkdtempSync(path.join(os.tmpdir(), 'perry-unit-resume-'));
const cache = path.join(scratch, 'cache');
console.log(`Artifacts: ${scratch}`);
const env = {
  ...process.env,
  PERRY_LL_OPT_LEVEL: 's', PERRY_RS4GC: '0', PERRY_SHADOW_STACK: '1',
  PERRY_INLINE_SHADOW_SLOT: '0', PERRY_FULL_OUTLINE_IC: '1',
  PERRY_LLVM_INPROCESS: 'native', PERRY_CODEGEN_UNITS: '4',
  PERRY_CODEGEN_UNIT_JOBS: '2', PERRY_CODEGEN_PROGRESS: '1',
};
function run(label, extra = []) {
  const output = path.join(scratch, `${label}.o`);
  const result = spawnSync(compiler, ['compile', fixture, '-o', output,
    '--cache-dir', cache, '--no-link', '--no-auto-optimize', '--no-color', ...extra],
  { cwd: scratch, env, encoding: 'utf8', timeout: 120_000, maxBuffer: 16 * 1024 * 1024 });
  const log = `${result.stdout ?? ''}${result.stderr ?? ''}`;
  fs.writeFileSync(path.join(scratch, `${label}.log`), log);
  assert.equal(result.status, 0, `${label} failed: ${result.error ?? log}`);
  return { bytes: fs.readFileSync(output), log };
}
const cold = run('cold');
assert(!cold.log.includes('reused checkpoint'));
const relative = fs.readdirSync(cache, { recursive: true }).filter(p => p.endsWith('.puc'));
assert.equal(relative.length, 4, 'fixture must exercise four native LLVM units');
// Model interruption before the final module object was stored, and before
// one unit completed. Keep the original files recoverable for investigation.
fs.renameSync(path.join(cache, 'objects'), path.join(scratch, 'completed-objects'));
fs.renameSync(path.join(cache, relative[0]), path.join(scratch, 'missing-unit.puc'));
const resumed = run('resumed');
assert.equal((resumed.log.match(/reused checkpoint/g) ?? []).length, 3);
assert.deepEqual(resumed.bytes, cold.bytes, 'resumed object differs from cold output');
assert(fs.existsSync(path.join(cache, relative[0])), 'missing unit was not saved');
const disabled = run('uncached', ['--no-cache']);
assert(!disabled.log.includes('reused checkpoint'));
assert.deepEqual(disabled.bytes, cold.bytes, 'checkpointing changed object output');
console.log('PASS: 3 units reused, 1 regenerated; cold/resumed/uncached objects identical');
