import { Buffer } from "node:buffer";

const sab = new SharedArrayBuffer(4);
const u8 = new Uint8Array(sab);
u8.set([1, 2, 3, 4]);
const b = Buffer.from(sab as any);
console.log("initial:", b.toJSON().data.join(","));
u8[1] = 9;
console.log("shared:", b.toJSON().data.join(","));
