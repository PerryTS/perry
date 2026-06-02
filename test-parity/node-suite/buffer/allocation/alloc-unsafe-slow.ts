import { Buffer } from "node:buffer";

const b = Buffer.allocUnsafeSlow(4);
b.fill(0x2a);
console.log("unsafeSlow len:", b.length);
console.log("unsafeSlow fill:", b.toString("hex"));
