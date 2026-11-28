import { Buffer } from "node:buffer";

// writeFloat*/writeDouble* apply `+value`, then check the offset.
function show(label: string, run: () => unknown): void {
  try {
    console.log(label, String(run()));
  } catch (e: any) {
    console.log(label, e.constructor.name, e.code, e.message);
  }
}

const values: [string, any][] = [
  ["'7'", "7"],
  ["'2.5e3'", "2.5e3"],
  ["valueOf", { valueOf: () => 5 }],
  ["1n", 1n],
  ["Symbol", Symbol("s")],
  ["'x'", "x"],
  ["undefined", undefined],
  ["null", null],
  ["-0", -0],
  ["1e40", 1e40],
];
for (const method of ["writeFloatLE", "writeFloatBE", "writeDoubleLE", "writeDoubleBE"]) {
  for (const [label, value] of values) {
    const buf: any = Buffer.alloc(8);
    show(`${method}(${label})`, () => `${buf[method](value)} ${buf.toString("hex")}`);
  }
}

const offsets: [string, any][] = [["'0'", "0"], ["0.5", 0.5], ["-1", -1], ["5", 5], ["null", null]];
for (const [label, offset] of offsets) {
  const buf: any = Buffer.alloc(8);
  show(`writeFloatBE(1, ${label})`, () => buf.writeFloatBE(1, offset));
  show(`writeDoubleLE(1, ${label})`, () => buf.writeDoubleLE(1, offset));
}
show("valueOf before offset", () => Buffer.alloc(8).writeDoubleBE({ valueOf: () => { console.log("valueOf ran"); return 1; } } as any, -1));

const typed = Buffer.alloc(8);
const str: any = "7";
const big: any = 1n;
show("typed writeFloatLE '7'", () => typed.writeFloatLE(str) + " " + typed.readFloatLE());
show("typed writeDoubleBE '7'", () => typed.writeDoubleBE(str) + " " + typed.readDoubleBE());
show("typed writeDoubleLE 1n", () => typed.writeDoubleLE(big));
