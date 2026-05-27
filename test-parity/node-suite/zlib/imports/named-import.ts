import { gzipSync, gunzipSync, deflateSync, inflateSync, constants } from "node:zlib";

console.log("gzipSync typeof:", typeof gzipSync);
console.log("gunzipSync typeof:", typeof gunzipSync);
console.log("deflateSync typeof:", typeof deflateSync);
console.log("inflateSync typeof:", typeof inflateSync);
console.log("constants typeof:", typeof constants);
