import { Buffer } from "node:buffer";

function show(label: string, run: () => unknown): void {
  try {
    const value = run();
    console.log(label, typeof value === "bigint" ? `${value}n` : String(value));
  } catch (e: any) {
    console.log(label, e.constructor.name, e.code, e.message);
  }
}

const offsets: [string, any][] = [
  ["undefined", undefined],
  ["'1'", "1"],
  ["1.5", 1.5],
  ["NaN", NaN],
  ["-1", -1],
  ["7", 7],
  ["Infinity", Infinity],
  ["null", null],
  ["valueOf", { valueOf: () => 1 }],
  ["1n", 1n],
  ["2**33", 2 ** 33],
];
const bytes = [1, 2, 3, 4, 5, 6, 7, 8];

// The integer readers check the offset with Node's validateNumber and
// boundsError: no coercion, an integer, and the bytes must fit.
for (const method of ["readUInt8", "readInt8", "readUInt16LE", "readInt16BE", "readUInt32BE", "readInt32LE"]) {
  for (const [label, offset] of offsets) {
    const buf: any = Buffer.from(bytes);
    show(`${method}(${label})`, () => buf[method](offset));
  }
}

// readIntLE and siblings: a missing offset throws before byteLength is read.
for (const method of ["readUIntLE", "readUIntBE", "readIntLE", "readIntBE"]) {
  for (const [label, offset] of offsets) {
    const buf: any = Buffer.from(bytes);
    show(`${method}(${label}, 3)`, () => buf[method](offset, 3));
  }
  for (const [label, byteLength] of [["0", 0], ["7", 7], ["2.5", 2.5], ["'2'", "2"], ["undefined", undefined]] as [string, any][]) {
    const buf: any = Buffer.from(bytes);
    show(`${method}(0, ${label})`, () => buf[method](0, byteLength));
    show(`${method}(undefined, ${label})`, () => buf[method](undefined, byteLength));
  }
}

const typed = Buffer.from(bytes);
const str: any = "1";
show("typed readUInt16BE '1'", () => typed.readUInt16BE(str));
show("typed readInt32LE 5", () => typed.readInt32LE(5));
show("typed readUIntBE(2, 6)", () => typed.readUIntBE(2, 6));
show("empty readUInt8", () => Buffer.alloc(0).readUInt8());
show("short readUInt16LE", () => Buffer.alloc(1).readUInt16LE());
