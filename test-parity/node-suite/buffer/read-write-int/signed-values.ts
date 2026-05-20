import { Buffer } from "node:buffer";

const b = Buffer.alloc(7);
b.writeInt8(-1, 0);
b.writeInt16LE(-2, 1);
b.writeInt32BE(-3, 3);
console.log("signed hex:", b.toString("hex"));
console.log("i8:", b.readInt8(0));
console.log("i16le:", b.readInt16LE(1));
console.log("i32be:", b.readInt32BE(3));
