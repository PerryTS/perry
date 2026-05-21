import { Buffer } from "node:buffer";

const cases = [
  ["writeUInt8 high", () => Buffer.alloc(2).writeUInt8(256, 0)],
  ["writeUInt8 low", () => Buffer.alloc(2).writeUInt8(-1, 0)],
  ["writeUInt16BE high", () => Buffer.alloc(3).writeUInt16BE(0x10000, 0)],
  ["writeUInt16LE frac", () => Buffer.alloc(3).writeUInt16LE(1.5, 0)],
  ["writeUInt32BE high", () => Buffer.alloc(5).writeUInt32BE(0x1_0000_0000, 0)],
  ["writeInt8 high", () => Buffer.alloc(2).writeInt8(128, 0)],
  ["writeInt8 low", () => Buffer.alloc(2).writeInt8(-129, 0)],
  ["writeInt16BE low", () => Buffer.alloc(3).writeInt16BE(-32769, 0)],
  ["writeInt32LE high", () => Buffer.alloc(5).writeInt32LE(2147483648, 0)],
  ["writeUIntBE high", () => Buffer.alloc(8).writeUIntBE(0x1_000000, 0, 3)],
  ["writeIntLE low", () => Buffer.alloc(8).writeIntLE(-0x800001, 0, 3)],
  ["writeUIntLE byteLength", () => Buffer.alloc(8).writeUIntLE(1, 0, 7)],
];

for (const [label, fn] of cases) {
  try {
    fn();
    console.log(label, "no throw");
  } catch (e) {
    console.log(label, e.name, e.code);
  }
}

const ok = Buffer.alloc(10);
ok.writeUInt8(Number.NaN, 0);
ok.writeUInt16BE(0xffff, 1);
ok.writeUInt32LE(0xffffffff, 3);
ok.writeIntBE(-0x800000, 7, 3);
console.log("ok hex:", ok.toString("hex"));
