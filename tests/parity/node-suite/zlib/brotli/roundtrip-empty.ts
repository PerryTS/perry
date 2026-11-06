import { brotliCompressSync, brotliDecompressSync } from "node:zlib";

const out = brotliDecompressSync(brotliCompressSync(Buffer.from("")));
console.log("empty length:", out.length);
console.log("empty toString:", JSON.stringify(out.toString()));
