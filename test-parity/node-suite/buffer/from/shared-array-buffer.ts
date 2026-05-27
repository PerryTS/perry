import { Buffer } from "node:buffer";

const sab = new SharedArrayBuffer(4);
const u8 = new Uint8Array(sab);
u8[0] = 1; u8[1] = 2; u8[2] = 3; u8[3] = 4;
const b = Buffer.from(sab as any);
console.log("initial:", b.toJSON().data.join(","));
console.log("identity:", b.buffer === sab);
u8[1] = 9;
console.log("shared:", b.toJSON().data.join(","));
b[2] = 8;
console.log("back:", Array.from(u8).join(","));
