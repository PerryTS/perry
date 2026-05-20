import { Buffer } from "node:buffer";

const source = new Uint8Array([1, 2, 3, 4]);
const b = Buffer.from(source);
source[1] = 9;
console.log("typed array copy:", b.toString("hex"));
console.log("source changed:", source[1]);
