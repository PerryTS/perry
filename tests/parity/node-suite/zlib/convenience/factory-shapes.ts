import * as zlib from "node:zlib";

console.log("createDeflate:", typeof zlib.createDeflate);
console.log("createDeflateRaw:", typeof zlib.createDeflateRaw);
console.log("createGzip:", typeof zlib.createGzip);
console.log("createGunzip:", typeof zlib.createGunzip);
console.log("createInflate:", typeof zlib.createInflate);
console.log("createInflateRaw:", typeof zlib.createInflateRaw);
console.log("createUnzip:", typeof zlib.createUnzip);
console.log("createBrotliCompress:", typeof zlib.createBrotliCompress);
console.log("createBrotliDecompress:", typeof zlib.createBrotliDecompress);
