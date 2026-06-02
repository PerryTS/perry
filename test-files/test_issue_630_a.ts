import { Buffer } from "node:buffer";
const b = Buffer.allocUnsafe(8);
console.log(b.fill(0xab).toString("hex"));
