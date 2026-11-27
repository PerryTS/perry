import { Buffer } from "node:buffer";

// The integer writers apply `+value` before the range and offset checks.
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
  ["Symbol", Symbol("s")],
  ["NaN", NaN],
  ["'x'", "x"],
  ["null", null],
  ["true", true],
  ["-0.5", -0.5],
  ["1.9", 1.9],
  ["Infinity", Infinity],
  ["2**40", 2 ** 40],
  ["'-129'", "-129"],
];
const methods = ["writeUInt8", "writeInt8", "writeUInt16LE", "writeInt16BE", "writeUInt32BE", "writeInt32LE"];
for (const method of methods) {
  for (const [label, value] of values) {
    const buf: any = Buffer.alloc(4);
    show(`${method}(${label})`, () => `${buf[method](value)} ${buf.toString("hex")}`);
  }
}

const typed = Buffer.alloc(4);
const str: any = "7";
const obj: any = { valueOf: () => 5 };
const big: any = 1n;
show("typed writeInt8 '7'", () => typed.writeInt8(str) + " " + typed[0]);
show("typed writeUInt16LE valueOf", () => typed.writeUInt16LE(obj) + " " + typed[0]);
show("typed writeInt8 1n", () => typed.writeInt8(big));
show("typed writeInt32BE 2**31", () => typed.writeInt32BE(2 ** 31));
show("typed writeUInt32LE -1", () => typed.writeUInt32LE(-1));

// `+value` runs before the offset check; the 8-bit writers check the offset
// type before the value range, the wider ones after it.
const order: any = Buffer.alloc(4);
show("valueOf before offset", () => order.writeUInt16BE({ valueOf: () => { console.log("valueOf ran"); return 1; } }, "0"));
show("uint8 offset type first", () => order.writeUInt8(300, "0"));
show("uint16 range first", () => order.writeUInt16LE(70000, "0"));

const offsets: [string, any][] = [
  ["'1'", "1"],
  ["1.5", 1.5],
  ["NaN", NaN],
  ["-1", -1],
  ["4", 4],
  ["Infinity", Infinity],
  ["null", null],
  ["valueOf", obj],
  ["undefined", undefined],
];
for (const method of ["writeUInt8", "writeInt16LE", "writeUInt32BE"]) {
  for (const [label, offset] of offsets) {
    const buf: any = Buffer.alloc(4);
    show(`${method}(1, ${label})`, () => buf[method](1, offset));
  }
}
show("empty writeUInt16LE", () => Buffer.alloc(1).writeUInt16LE(1));

// A `valueOf` that allocates heavily runs before the write lands.
const target = Buffer.alloc(4);
const junk: object[] = [];
const allocating: any = {
  valueOf() {
    for (let i = 0; i < 200000; i++) junk.push({ i });
    return 0x1234;
  },
};
show("allocating valueOf", () => target.writeUInt16BE(allocating, 2) + " " + target.toString("hex"));
