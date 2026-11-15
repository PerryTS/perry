// A Number whose IEEE bits look like a heap address (a denormal double) asked
// for its truthiness by the runtime. `js_is_truthy` read an 8-byte-aligned word
// with clear top 16 bits above the handle band as a raw string pointer and read
// its length (SIGSEGV for `stream.getDefaultHighWaterMark(e)`, and a falsy
// answer for a Number whose bits are an empty string's address). Every nonzero
// Number is truthy; +0 and NaN are falsy. Strings are the positive controls.
import * as stream from "stream";

function bitsDouble(hi: number, lo: number): number {
  const f = new Float64Array(1);
  const u = new Uint32Array(f.buffer);
  u[0] = lo;
  u[1] = hi;
  return f[0];
}

const VALUES: [string, any][] = [
  ["min", Number.MIN_VALUE],
  ["2^21", Number.MIN_VALUE * Math.pow(2, 21)],
  ["2^28", Number.MIN_VALUE * Math.pow(2, 28)],
  ["2^40", Number.MIN_VALUE * Math.pow(2, 40)],
  ["odd", bitsDouble(0, 0x10000003)],
  ["mmap", bitsDouble(0x7f12, 0x34567890)],
  ["brk", bitsDouble(0x5555, 0x55560010)],
  ["maxden", bitsDouble(0x000fffff, 0xffffffff)],
  ["-2^28", -(Number.MIN_VALUE * Math.pow(2, 28))],
  ["+0", 0],
  ["NaN", NaN],
  ["1.5", 1.5],
  ["empty", ""],
  ["str", "ab"],
];

function pick(x: any): number {
  return x ? 1 : 0;
}

const OPS: [string, (e: any) => any][] = [
  ["getDefaultHighWaterMark", (e) => typeof stream.getDefaultHighWaterMark(e)],
  ["filter Boolean", (e) => [e].filter(Boolean).length],
  ["some", (e) => [e].some((x: any) => x)],
  ["find index", (e) => [e].findIndex((x: any) => x)],
  ["ternary", (e) => pick(e)],
  ["not", (e) => !e],
];

for (const [name, op] of OPS) {
  let line = name + ":";
  for (const [label, e] of VALUES) {
    let r: string;
    try {
      r = String(op(e));
    } catch (x: any) {
      r = "THROW " + (x && x.name);
    }
    line += " " + label + "=" + r;
  }
  console.log(line);
}
