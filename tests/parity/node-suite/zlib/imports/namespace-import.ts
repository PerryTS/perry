import * as zlib from "node:zlib";

console.log("zlib typeof:", typeof zlib);
console.log("gzipSync typeof:", typeof zlib.gzipSync);
console.log("gunzipSync typeof:", typeof zlib.gunzipSync);
console.log("deflateSync typeof:", typeof zlib.deflateSync);
console.log("inflateSync typeof:", typeof zlib.inflateSync);
console.log("deflateRawSync typeof:", typeof zlib.deflateRawSync);
console.log("inflateRawSync typeof:", typeof zlib.inflateRawSync);
console.log("unzipSync typeof:", typeof zlib.unzipSync);
console.log("brotliCompressSync typeof:", typeof zlib.brotliCompressSync);
console.log("brotliDecompressSync typeof:", typeof zlib.brotliDecompressSync);
console.log("crc32 typeof:", typeof zlib.crc32);
