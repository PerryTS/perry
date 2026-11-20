// External double bits that equal a NaN-box tag pattern must read as NaN.
// v8.deserialize hands foreign bytes to JS (Numbers and Date time values);
// in node every NaN read from them is just NaN.
import * as v8 from "node:v8";

// [hi, lo] u32 halves of the double's bit pattern. Ordered so the patterns
// that forge heap references (pointer, string, handle) come last.
const PATTERNS: Array<[string, number, number]> = [
  ["quiet-payload", 0x7ff80000, 0x00000001],
  ["int32-5", 0x7ffe0000, 0x00000005],
  ["undefined", 0x7ffc0000, 0x00000001],
  ["null", 0x7ffc0000, 0x00000002],
  ["true", 0x7ffc0000, 0x00000004],
  ["neg-payload", 0xfffe0000, 0x00000005],
  ["handle", 0xfff90000, 0x00000001],
  ["pointer", 0x7ffd0000, 0x00000000],
  ["string", 0x7fff0000, 0x00000000],
];

function le(hi: number, lo: number): number[] {
  return [lo & 255, (lo >>> 8) & 255, (lo >>> 16) & 255, lo >>> 24,
          hi & 255, (hi >>> 8) & 255, (hi >>> 16) & 255, hi >>> 24];
}

function show(label: string, x: any): void {
  console.log(label, typeof x, x === 5, Number.isNaN(x), Object.is(x, NaN), String(x));
}

for (const [name, hi, lo] of PATTERNS) {
  // 0xFF 0x0F: header, version 15; 0x4E 'N': kDouble.
  show("deserialize " + name, v8.deserialize(Buffer.from([0xff, 0x0f, 0x4e, ...le(hi, lo)])));
}
for (const [name, hi, lo] of PATTERNS) {
  // 0x44 'D': kDate.
  const date = v8.deserialize(Buffer.from([0xff, 0x0f, 0x44, ...le(hi, lo)]));
  show("date " + name + " " + (date instanceof Date), date.getTime());
}
