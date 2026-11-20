import { deflateRawSync, inflateRawSync } from "node:zlib";

const out = inflateRawSync(deflateRawSync(Buffer.from("")));
console.log("empty length:", out.length);
console.log("empty toString:", JSON.stringify(out.toString()));
