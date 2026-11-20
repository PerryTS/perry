import { gzipSync } from "node:zlib";

const compressed = gzipSync(Buffer.from("magic"));
console.log("is buffer:", Buffer.isBuffer(compressed));
console.log("magic 0:", compressed[0].toString(16));
console.log("magic 1:", compressed[1].toString(16));
console.log("method:", compressed[2]);
