import { deflateSync, inflateSync } from "node:zlib";

const out = inflateSync(deflateSync(Buffer.from("")));
console.log("empty length:", out.length);
console.log("empty toString:", JSON.stringify(out.toString()));
