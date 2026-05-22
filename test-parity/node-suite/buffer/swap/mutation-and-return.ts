import { Buffer } from "node:buffer";

const b16 = Buffer.from([0, 1, 2, 3]);
console.log("swap16 same:", b16.swap16() === b16, b16.toString("hex"));
const b32 = Buffer.from([0, 1, 2, 3, 4, 5, 6, 7]);
console.log("swap32 same:", b32.swap32() === b32, b32.toString("hex"));
const b64 = Buffer.from([0, 1, 2, 3, 4, 5, 6, 7]);
console.log("swap64 same:", b64.swap64() === b64, b64.toString("hex"));
