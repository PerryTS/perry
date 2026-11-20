import { gzipSync, gunzipSync } from "node:zlib";

// Plain Uint8Array (not a Buffer subclass instance) should be accepted.
const bytes = new Uint8Array([104, 101, 108, 108, 111]); // "hello"
const roundtripped = gunzipSync(gzipSync(bytes));
console.log("uint8array length:", roundtripped.length);
console.log("uint8array bytes:", roundtripped[0], roundtripped[4]);
