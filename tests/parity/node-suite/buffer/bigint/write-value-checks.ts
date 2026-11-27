import { Buffer } from "node:buffer";

// writeBig{U}Int64* take a BigInt as is: the range check, then the offset
// check, and only then a non-BigInt value fails to mix.
function show(label: string, run: () => unknown): void {
  try {
    console.log(label, String(run()));
  } catch (e: any) {
    console.log(label, e.constructor.name, e.code, e.message);
  }
}

const values: [string, any][] = [
  ["1n", 1n],
  ["-1n", -1n],
  ["2n**63n", 2n ** 63n],
  ["2n**63n-1n", 2n ** 63n - 1n],
  ["-(2n**63n)", -(2n ** 63n)],
  ["-(2n**63n)-1n", -(2n ** 63n) - 1n],
  ["2n**64n-1n", 2n ** 64n - 1n],
  ["2n**64n", 2n ** 64n],
  ["-5n", -5n],
  ["1", 1],
  ["'7'", "7"],
  ["valueOf", { valueOf: () => 5n }],
  ["-(2**40)", -(2 ** 40)],
  ["-1", -1],
  ["2**64", 2 ** 64],
  ["1e21", 1e21],
  ["2**70", 2 ** 70],
  ["1.5e300", 1.5e300],
  ["NaN", NaN],
  ["true", true],
  ["null", null],
  ["Symbol", Symbol("s")],
  ["'1e30'", "1e30"],
  ["' 0x10 '", " 0x10 "],
  ["''", ""],
  ["'99999999999999999999'", "99999999999999999999"],
  ["'-99999999999999999999'", "-99999999999999999999"],
  ["valueOf '2**70'", { valueOf: () => "1180591620717411303424" }],
  ["valueOf 1", { valueOf: () => 1 }],
];
for (const method of ["writeBigInt64LE", "writeBigInt64BE", "writeBigUInt64LE", "writeBigUInt64BE"]) {
  for (const [label, value] of values) {
    const buf: any = Buffer.alloc(8);
    show(`${method}(${label})`, () => `${buf[method](value)} ${buf.toString("hex")}`);
  }
}

const offsets: [string, any][] = [["'0'", "0"], ["0.5", 0.5], ["-1", -1], ["1", 1], ["null", null]];
for (const [label, offset] of offsets) {
  const buf: any = Buffer.alloc(8);
  show(`writeBigInt64BE(1n, ${label})`, () => buf.writeBigInt64BE(1n, offset));
  show(`writeBigUInt64LE(1, ${label})`, () => buf.writeBigUInt64LE(1, offset));
}
show("empty", () => Buffer.alloc(4).writeBigInt64LE(1n));

const typed = Buffer.alloc(8);
const num: any = 1;
show("typed writeBigInt64LE 1", () => typed.writeBigInt64LE(num));
show("typed writeBigUInt64BE -1n", () => typed.writeBigUInt64BE(-1n));
show("typed writeBigUInt64BE 2n**64n-1n", () => typed.writeBigUInt64BE(2n ** 64n - 1n) + " " + typed.toString("hex"));

// Node converts the value afresh for each step: `value > max`, `value < min`,
// then `value & 0xffffffffn` (the low word is written) and `value >> 32n`.
function sequence(values: any[]) {
  let calls = 0;
  const seen: string[] = [];
  const value = {
    valueOf() {
      const next = values[Math.min(calls++, values.length - 1)];
      seen.push(String(next));
      if (next === "throw") throw new Error(`valueOf call ${calls}`);
      return next;
    },
  };
  return { seen, value };
}
const sequences: [string, any[]][] = [
  ["5n", [5n]],
  ["1n, throw", [1n, "throw"]],
  ["1n, 1n, throw", [1n, 1n, "throw"]],
  ["1n, 1n, 1n, throw", [1n, 1n, 1n, "throw"]],
  ["2n**70n, 1n", [2n ** 70n, 1n]],
  ["1n, -(2n**70n)", [1n, -(2n ** 70n)]],
  ["1n, 1n, 1n, 2n**40n", [1n, 1n, 1n, 2n ** 40n]],
  ["1n, 1n, 1n, 1", [1n, 1n, 1n, 1]],
  ["1n, 1n, 1", [1n, 1n, 1]],
];
for (const method of ["writeBigInt64LE", "writeBigUInt64BE"]) {
  for (const [label, values] of sequences) {
    const buf: any = Buffer.alloc(8);
    const { seen, value } = sequence(values);
    show(`${method}(${label})`, () => buf[method](value));
    console.log("  calls", seen.join(","), buf.toString("hex"));
  }
}
