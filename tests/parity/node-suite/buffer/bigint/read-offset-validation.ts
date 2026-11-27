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

// readBig{U}Int64* check the offset with Node's validateNumber and
// boundsError.
for (const method of ["readBigInt64LE", "readBigInt64BE", "readBigUInt64LE", "readBigUInt64BE"]) {
  for (const [label, offset] of offsets) {
    const buf: any = Buffer.from(bytes);
    show(`${method}(${label})`, () => buf[method](offset));
  }
}
const typed = Buffer.from(bytes);
const str: any = "0";
show("typed readBigInt64LE '0'", () => typed.readBigInt64LE(str));
show("typed readBigUInt64BE 1", () => typed.readBigUInt64BE(1));
show("short readBigInt64BE", () => Buffer.alloc(4).readBigInt64BE());
