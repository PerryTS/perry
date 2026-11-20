import { Buffer } from "node:buffer";

const a = Buffer.alloc(4);
const b = Buffer.alloc(4, 0xab);
console.log("alloc len:", a.length);
console.log("alloc zero:", a.toString("hex"));
console.log("alloc fill:", b.toString("hex"));
