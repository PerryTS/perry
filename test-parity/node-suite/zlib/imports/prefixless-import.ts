import { gzipSync, gunzipSync } from "zlib";

const out = gunzipSync(gzipSync(Buffer.from("prefixless")));
console.log("prefixless roundtrip:", out.toString());
