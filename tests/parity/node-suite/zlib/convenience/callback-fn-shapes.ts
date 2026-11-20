import * as zlib from "node:zlib";

console.log("deflate:", typeof zlib.deflate);
console.log("deflateRaw:", typeof zlib.deflateRaw);
console.log("gzip:", typeof zlib.gzip);
console.log("gunzip:", typeof zlib.gunzip);
console.log("inflate:", typeof zlib.inflate);
console.log("inflateRaw:", typeof zlib.inflateRaw);
console.log("unzip:", typeof zlib.unzip);
console.log("brotliCompress:", typeof zlib.brotliCompress);
console.log("brotliDecompress:", typeof zlib.brotliDecompress);
