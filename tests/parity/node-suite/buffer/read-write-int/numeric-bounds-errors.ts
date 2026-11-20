import { Buffer } from "node:buffer";

const b = Buffer.alloc(4);

const cases: [string, () => unknown][] = [
  ["readFloatBE 1", () => b.readFloatBE(1)],
  ["readFloatLE -1", () => b.readFloatLE(-1 as any)],
  ["readDoubleBE 0", () => b.readDoubleBE(0)],
  ["readDoubleLE -1", () => b.readDoubleLE(-1 as any)],
  ["writeUInt16BE 4", () => b.writeUInt16BE(1, 4)],
  ["writeInt32LE 1", () => b.writeInt32LE(1, 1)],
  ["writeFloatLE 2", () => b.writeFloatLE(1.5, 2)],
  ["writeDoubleLE -1", () => b.writeDoubleLE(1.5, -1 as any)],
  ["readUIntBE len7", () => b.readUIntBE(0, 7)],
  ["readUIntBE oob", () => b.readUIntBE(2, 3)],
  ["readIntLE len0", () => b.readIntLE(0, 0)],
  ["readIntLE oob", () => b.readIntLE(2, 3)],
  ["writeUIntBE oob", () => b.writeUIntBE(1, 2, 3)],
  ["writeIntLE oob", () => b.writeIntLE(-1, 2, 3)],
  ["readBigUInt64BE 0", () => b.readBigUInt64BE(0)],
  ["readBigInt64LE -1", () => b.readBigInt64LE(-1 as any)],
  ["writeBigUInt64BE 0", () => b.writeBigUInt64BE(1n, 0)],
  ["writeBigInt64LE -1", () => b.writeBigInt64LE(-1n, -1 as any)],
];

for (const [label, fn] of cases) {
  try {
    const value = fn();
    console.log(label + ":", typeof value === "bigint" ? value.toString() : value);
  } catch (err: any) {
    console.log(label + ":", err?.name, err?.code ?? "no-code");
  }
}
