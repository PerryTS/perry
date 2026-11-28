import { Buffer } from "node:buffer";

// writeIntLE/writeUIntBE and siblings: byteLength, then `+value`, then the
// range and offset checks.
function show(label: string, run: () => unknown): void {
  try {
    console.log(label, String(run()));
  } catch (e: any) {
    console.log(label, e.constructor.name, e.code, e.message);
  }
}

const values: [string, any][] = [
  ["'7'", "7"],
  ["valueOf", { valueOf: () => 5 }],
  ["1n", 1n],
  ["NaN", NaN],
  ["-0.5", -0.5],
  ["-1.5", -1.5],
  ["2**47", 2 ** 47],
  ["2**48", 2 ** 48],
  ["-(2**40)", -(2 ** 40)],
];
for (const method of ["writeIntLE", "writeIntBE", "writeUIntLE", "writeUIntBE"]) {
  for (const byteLength of [1, 3, 5, 6]) {
    for (const [label, value] of values) {
      const buf: any = Buffer.alloc(8);
      show(`${method}(${label}, 0, ${byteLength})`, () => `${buf[method](value, 1, byteLength)} ${buf.toString("hex")}`);
    }
  }
}

const lengths: [string, any][] = [["0", 0], ["7", 7], ["2.5", 2.5], ["NaN", NaN], ["'3'", "3"], ["undefined", undefined]];
for (const [label, byteLength] of lengths) {
  const buf: any = Buffer.alloc(8);
  show(`writeUIntLE byteLength ${label}`, () => buf.writeUIntLE(1, 0, byteLength));
}
const buf: any = Buffer.alloc(8);
show("byteLength before value", () => buf.writeIntBE({ valueOf: () => { console.log("valueOf ran"); return 1; } }, 0, 9));
show("no offset", () => buf.writeIntBE(1, undefined, 2));
show("offset '0'", () => buf.writeUIntLE(1, "0", 4));
show("offset out of range", () => buf.writeIntLE(1, 6, 3));

const typed = Buffer.alloc(6);
const str: any = "-2";
show("typed writeIntBE '-2'", () => typed.writeIntBE(str, 0, 6) + " " + typed.toString("hex"));
show("typed writeUIntLE 2**48", () => typed.writeUIntLE(2 ** 48, 0, 6));
