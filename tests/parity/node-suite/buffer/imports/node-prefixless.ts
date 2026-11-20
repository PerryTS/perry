import { Buffer } from "buffer";

const b = Buffer.from([0x70, 0x66]);
console.log("prefixless usable:", Buffer.isBuffer(b));
console.log("prefixless value:", b.toString("utf8"));
