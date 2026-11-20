import { Buffer } from "node:buffer";

const b = Buffer.alloc(5);
console.log("fill return same:", b.fill(0x61) === b);
console.log("fill all:", b.toString("utf8"));
b.fill(0x62, 1, 4);
console.log("fill range:", b.toString("utf8"));
