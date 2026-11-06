import { gzipSync, gunzipSync } from "node:zlib";

const chunk = "abcdefghijklmnopqrstuvwxyz0123456789";
const input = chunk.repeat(1000);
const compressed = gzipSync(Buffer.from(input));
const out = gunzipSync(compressed).toString();
console.log("large input length:", input.length);
console.log("large roundtrip equal:", out === input);
console.log("compressed shorter:", compressed.length < input.length);
