import { Buffer } from "node:buffer";

const b = Buffer.from("01020304feffffff", "hex");
console.log("u8:", b.readUInt8(0));
console.log("i8:", b.readInt8(4));
console.log("u16be:", b.readUInt16BE(0));
console.log("u16le:", b.readUInt16LE(0));
console.log("u32be:", b.readUInt32BE(0));
console.log("u32le:", b.readUInt32LE(0));
console.log("i32le:", b.readInt32LE(4));
