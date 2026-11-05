import * as zlib from "node:zlib";

console.log("Deflate:", typeof zlib.Deflate);
console.log("DeflateRaw:", typeof zlib.DeflateRaw);
console.log("Gzip:", typeof zlib.Gzip);
console.log("Gunzip:", typeof zlib.Gunzip);
console.log("Inflate:", typeof zlib.Inflate);
console.log("InflateRaw:", typeof zlib.InflateRaw);
console.log("Unzip:", typeof zlib.Unzip);
console.log("BrotliCompress:", typeof zlib.BrotliCompress);
console.log("BrotliDecompress:", typeof zlib.BrotliDecompress);
