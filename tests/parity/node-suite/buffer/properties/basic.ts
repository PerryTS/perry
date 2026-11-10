import { Buffer } from "node:buffer";

const b = Buffer.from([10, 20]);
console.log("length:", b.length);
console.log("index0:", b[0]);
b[1] = 30;
console.log("index1:", b[1]);
