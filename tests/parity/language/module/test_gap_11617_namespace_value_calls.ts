// #11617: calls through a node: namespace held as a value.
import * as crypto from "node:crypto";
import * as zlib from "node:zlib";
const B: any = { crypto, zlib };
console.log("hash hex:", B.crypto.hash("sha1", "x", "hex"));
console.log("hash default:", B.crypto.hash("sha1", "x"));
console.log("hash base64:", B.crypto.hash("sha256", "abc", "base64"));
const gz = zlib.gzipSync(new Uint8Array(1000).fill(65));
const g = B.zlib.createGunzip();
console.log("createGunzip:", typeof g, typeof g?.on);
let total = 0;
g.on("data", (chunk: Uint8Array) => (total += chunk.length));
g.on("end", () => console.log("gunzipped:", total));
g.end(gz);
console.log("createGzip:", typeof B.zlib.createGzip(), "createDeflate:", typeof B.zlib.createDeflate());
