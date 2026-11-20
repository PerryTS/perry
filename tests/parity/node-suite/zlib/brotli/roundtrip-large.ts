import { brotliCompressSync, brotliDecompressSync } from "node:zlib";

const chunk = "lorem ipsum dolor sit amet consectetur adipiscing elit ";
const input = chunk.repeat(500);
const compressed = brotliCompressSync(Buffer.from(input));
const out = brotliDecompressSync(compressed).toString();
console.log("large equal:", out === input);
console.log("compressed shorter:", compressed.length < input.length);
