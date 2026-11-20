import { Buffer } from "node:buffer";

console.log("pool type:", typeof Buffer.poolSize);
console.log("pool positive:", Buffer.poolSize > 0);
Buffer.poolSize = 4096;
console.log("pool set:", Buffer.poolSize);
