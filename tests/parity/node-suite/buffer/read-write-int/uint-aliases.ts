import { Buffer } from "node:buffer";

const b = Buffer.alloc(6);
b.writeUint8(0x11, 0);
b.writeUint16BE(0x2233, 1);
b.writeUint16LE(0x4455, 3);
console.log("alias hex:", b.toString("hex"));
console.log("readUint8:", b.readUint8(0));
console.log("readUint16BE:", b.readUint16BE(1));
console.log("readUint16LE:", b.readUint16LE(3));
