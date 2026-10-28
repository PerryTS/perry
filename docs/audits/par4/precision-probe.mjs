import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { mkdtempSync, readFileSync, writeFileSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import path from 'node:path';

// DEC220 regression: a private receiver retains its own-slot certificate.
// --require-whole remains accepted; both modes are always required.
// --sabotage gives the private record to unknown code; unchanged assertions must fail.
const binary = process.env.PERRY_BIN;
assert(binary, 'set PERRY_BIN to the compiler under review');
const dir = mkdtempSync(path.join(tmpdir(), 'par4-precision-'));
const sabotage = process.argv.includes('--sabotage');
const a = `export function privateRead() { const r = { source: 1, target: 2 }; ${sabotage ? '(globalThis as any).keep(r);' : ''} return r.target; }
export function giveAway() { return { source: 3, target: 4 }; }
`;
const main = `import { privateRead, giveAway } from './a';
(globalThis as any).keep(giveAway());
console.log(privateRead());
`;
try {
  writeFileSync(path.join(dir, 'a.ts'), a);
  writeFileSync(path.join(dir, 'main.ts'), main);
  const records = {};
  for (const [mode, units] of [['whole', '0'], ['unit', '1']]) {
    const dump = path.join(dir, `${mode}.facts`);
    const r = spawnSync(binary, ['compile', '--cache-dir', path.join(dir, `cache-${mode}`), 'main.ts', '-o', path.join(dir, `${mode}.out`)], {
      cwd: dir,
      env: { ...process.env, PERRY_ALLOW_PERRY_FEATURES: '1', PERRY_FACTS_UNITS: units, PERRY_FACTS_ONLY: '1', PERRY_FACTS_REPORT: '1', PERRY_FACTS_EXPLAIN: '0', PERRY_FACTS_DUMP: dump },
      encoding: 'utf8', maxBuffer: 4 * 1024 * 1024,
    });
    assert.equal(r.status, 0, `${mode} compiler failed: ${r.stderr}`);
    const lines = readFileSync(dump, 'utf8').split('\n');
    assert(!lines.some(l => l.startsWith('# stats ') && l.includes('aborted=true')), `${mode} has no oracle`);
    records[mode] = lines.filter(l => l.startsWith('S ') && l.includes(' .target call=0 '));
    assert(records[mode].length > 0, `${mode}: private read must be live`);
  }
  const proven = line => / verdict=(?:T0|T0set)$/.test(line);
  assert(records.unit.every(proven), `private record lost its per-unit proof:\n${records.unit.join('\n')}`);
  assert(records.whole.every(proven), `named whole imprecision remains:\n${records.whole.join('\n')}`);
  // Native JavaScript checks the same confinement argument with a real mutator.
  const jsA = a.replace('export ', '').replace('export ', '').replaceAll('(globalThis as any)', 'globalThis');
  const js = `globalThis.keep = r => { r.target = 99; };\n${jsA}\nkeep(giveAway()); console.log(privateRead());`;
  const node = spawnSync(process.execPath, ['--input-type=module', '-e', js], { encoding: 'utf8' });
  assert.equal(node.status, 0, node.stderr);
  assert.equal(node.stdout.trim(), '2', 'foreign retention changed a private record');
  console.log('PASS: private own-slot read is proven in whole and per-unit modes; JS prints 2');
} finally {
  rmSync(dir, { recursive: true, force: true });
}
