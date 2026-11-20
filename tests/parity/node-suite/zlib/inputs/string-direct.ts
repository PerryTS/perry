import { gzipSync, gunzipSync } from "node:zlib";

// Node accepts a string and encodes it as UTF-8 implicitly.
const out = gunzipSync(gzipSync("hello world")).toString();
console.log("string roundtrip equal:", out === "hello world");
console.log("string roundtrip out:", out);
