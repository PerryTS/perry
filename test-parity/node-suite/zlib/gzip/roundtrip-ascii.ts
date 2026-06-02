import { gzipSync, gunzipSync } from "node:zlib";

const input = "the quick brown fox jumps over the lazy dog";
const out = gunzipSync(gzipSync(Buffer.from(input))).toString();
console.log("roundtrip equal:", out === input);
console.log("roundtrip out:", out);
