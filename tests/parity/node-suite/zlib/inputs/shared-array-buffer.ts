import { gunzipSync, gzipSync } from "node:zlib";

const input = new SharedArrayBuffer(5);
new Uint8Array(input).set([104, 101, 108, 108, 111]);
const output = gunzipSync(gzipSync(input));
console.log("output:", output.toString());
console.log("buffer:", Buffer.isBuffer(output));
