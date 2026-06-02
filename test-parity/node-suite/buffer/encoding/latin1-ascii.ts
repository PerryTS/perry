import { Buffer } from "node:buffer";

const b = Buffer.from([0x68, 0x69, 0x21]);
console.log("latin1 ascii-safe:", b.toString("latin1"));
console.log("ascii:", b.toString("ascii"));
console.log("binary ascii-safe:", b.toString("binary"));
