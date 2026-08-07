// JSON pipeline: read input JSON, filter active records, add 2 derived fields,
// serialize, write output. Perry stdlib JSON + fs (utf-8 text).
//
// This file used to carry a block of "PERRY GAP NOTES (all in v0.5.29)"
// claiming that `process.argv.slice(2)` returned garbage, that iterating a
// large `JSON.parse` result corrupted records above ~200 of them, and that the
// driver therefore ran Perry on the 100-record fixture only. **Every one of
// those claims is false as of v0.5.1338** and they were deleted rather than
// left to mislead: `run.sh` runs Perry on the full 500k-record / 107.5 MB
// fixture like every other language, the output matched the Bun reference on
// 20 of 20 runs during the v0.5.1335 baseline regeneration, and an argv probe
// returns two proper strings. The correctness half was fixed at some point
// without the comment being updated (#7592).
//
// What IS true at 500k is a performance gap, tracked in #7592 — see the issue
// for the per-phase split. Nothing about the code below is a workaround.

import * as fs from 'fs';

function imul32(a: number, b: number): number {
  const aHi = (a >>> 16) & 0xffff;
  const aLo = a & 0xffff;
  const bHi = (b >>> 16) & 0xffff;
  const bLo = b & 0xffff;
  return ((aLo * bLo) + (((aHi * bLo + aLo * bHi) << 16) >>> 0)) | 0;
}
function fnv1a32(s: string): number {
  let h = 0x811c9dc5 | 0;
  for (let i = 0; i < s.length; i++) {
    h = (h ^ s.charCodeAt(i)) | 0;
    h = imul32(h, 0x01000193);
  }
  return h >>> 0;
}

if (process.argv.length < 4) {
  console.error('usage: json_pipeline <input.json> <output.json>');
  process.exit(1);
}
const inPath = process.argv[2];
const outPath = process.argv[3];

const text = fs.readFileSync(inPath, 'utf8');
const inputBytes = text.length;

const records = JSON.parse(text) as any[];
const recordsIn = records.length;

const out: any[] = [];
for (let i = 0; i < recordsIn; i++) {
  const r = records[i];
  if (r.active !== true) continue;
  const age = r.age;
  out.push({
    id: r.id,
    name: r.name,
    email: r.email,
    age: age,
    country: r.country,
    tags: r.tags,
    score: r.score,
    active: r.active,
    addr: r.addr,
    display_name: r.name.toUpperCase(),
    age_group: age < 30 ? 'young' : age < 50 ? 'mid' : 'senior',
  });
}
const recordsOut = out.length;

const serialized = JSON.stringify(out);
const outputBytes = serialized.length;

fs.writeFileSync(outPath, serialized);

const hash = fnv1a32(serialized);
const hex = hash.toString(16).padStart(8, '0');
console.log(`input_bytes=${inputBytes} records_in=${recordsIn} records_out=${recordsOut} output_bytes=${outputBytes} hash=${hex}`);
