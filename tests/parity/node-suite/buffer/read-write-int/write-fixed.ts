import { Buffer } from "node:buffer";

const b = Buffer.alloc(8);
console.log("u8 ret:", b.writeUInt8(0xab, 0));
b.writeInt8(-1, 1);
b.writeUInt16BE(0x1234, 2);
b.writeUInt16LE(0x5678, 4);
b.writeInt16BE(-2, 6);
console.log("fixed hex:", b.toString("hex"));
