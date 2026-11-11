import { gzipSync, gunzipSync } from "node:zlib";

const out = gunzipSync(gzipSync(Buffer.from("")));
console.log("empty length:", out.length);
console.log("empty toString:", JSON.stringify(out.toString()));
