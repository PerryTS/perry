import { gzipSync, gunzipSync } from "node:zlib";

// gzip streams can be concatenated; gunzip must decompress every member.
const a = gzipSync(Buffer.from("first "));
const b = gzipSync(Buffer.from("second "));
const c = gzipSync(Buffer.from("third"));
const concatenated = Buffer.concat([a, b, c]);
const out = gunzipSync(concatenated).toString();
console.log("multi-member length:", out.length);
console.log("multi-member out:", out);
console.log("multi-member equal:", out === "first second third");
