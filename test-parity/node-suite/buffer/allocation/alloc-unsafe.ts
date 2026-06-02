import { Buffer } from "node:buffer";

const b = Buffer.allocUnsafe(5);
b.fill(0);
console.log("unsafe len:", b.length);
console.log("unsafe normalized:", b.toString("hex"));
