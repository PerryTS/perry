import { deflateSync, inflateSync } from "node:zlib";

const bytes = new Uint8Array(512);
for (let i = 0; i < 512; i++) bytes[i] = (i * 7 + 13) & 0xff;
const out = inflateSync(deflateSync(Buffer.from(bytes)));
console.log("binary length:", out.length);
let matches = true;
for (let i = 0; i < 512; i++) {
  if (out[i] !== ((i * 7 + 13) & 0xff)) { matches = false; break; }
}
console.log("binary identity:", matches);
