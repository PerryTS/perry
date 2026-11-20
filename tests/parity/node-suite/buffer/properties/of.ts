import { Buffer } from "node:buffer";

const b = Buffer.of(1, 255, 256, 511);
console.log("of hex:", b.toString("hex"));
console.log("of is buffer:", Buffer.isBuffer(b));
