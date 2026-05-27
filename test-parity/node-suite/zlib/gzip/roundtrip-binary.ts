import { gzipSync, gunzipSync } from "node:zlib";

const bytes = new Uint8Array(256);
for (let i = 0; i < 256; i++) bytes[i] = i;
const out = gunzipSync(gzipSync(Buffer.from(bytes)));
console.log("binary length:", out.length);
let matches = true;
for (let i = 0; i < 256; i++) {
  if (out[i] !== i) { matches = false; break; }
}
console.log("binary identity:", matches);
