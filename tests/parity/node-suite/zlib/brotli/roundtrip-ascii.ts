import { brotliCompressSync, brotliDecompressSync } from "node:zlib";

const input = "brotli roundtrip parity check";
const out = brotliDecompressSync(brotliCompressSync(Buffer.from(input))).toString();
console.log("brotli equal:", out === input);
console.log("brotli out:", out);
