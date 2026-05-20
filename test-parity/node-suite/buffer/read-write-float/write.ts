import { Buffer } from "node:buffer";

const b = Buffer.alloc(24);
console.log("fbe ret:", b.writeFloatBE(1.5, 0));
console.log("fle ret:", b.writeFloatLE(1.5, 4));
console.log("dbe ret:", b.writeDoubleBE(1.5, 8));
console.log("dle ret:", b.writeDoubleLE(1.5, 16));
console.log("float hex:", b.toString("hex"));
