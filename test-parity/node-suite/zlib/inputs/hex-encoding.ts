import { gzipSync, gunzipSync } from "node:zlib";

// Buffer decoded from a hex string must round-trip identically.
const original = Buffer.from("deadbeef0011223344556677889900", "hex");
const out = gunzipSync(gzipSync(original));
console.log("hex length:", out.length);
console.log("hex equal:", out.toString("hex") === original.toString("hex"));
console.log("hex out:", out.toString("hex"));
