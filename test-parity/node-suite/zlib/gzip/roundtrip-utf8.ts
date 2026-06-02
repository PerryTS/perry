import { gzipSync, gunzipSync } from "node:zlib";

const input = "héllo 世界 🌍 αβγ";
const out = gunzipSync(gzipSync(Buffer.from(input, "utf8"))).toString("utf8");
console.log("utf8 equal:", out === input);
console.log("utf8 out:", out);
