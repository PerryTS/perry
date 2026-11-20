import { Buffer } from "node:buffer";

const b = Buffer.alloc(4);
function show(label: string, fn: () => unknown): void {
  try { console.log(label + ":", fn()); } catch (err: any) { console.log(label + ":", err?.name, err?.code || "no-code"); }
}
show("writeUInt8 256", () => b.writeUInt8(256, 0));
show("writeInt8 -129", () => b.writeInt8(-129, 0));
show("readUInt32BE oob", () => b.readUInt32BE(1));
show("readInt16LE neg offset", () => b.readInt16LE(-1 as any));
