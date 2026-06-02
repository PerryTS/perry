import { Buffer } from "node:buffer";

const b = Buffer.alloc(16);
b.writeBigInt64BE(BigInt(-1), 0);
b.writeBigUInt64LE(BigInt(2), 8);
console.log("big hex:", b.toString("hex"));
console.log("big int be:", b.readBigInt64BE(0).toString());
console.log("big uint le:", b.readBigUInt64LE(8).toString());
