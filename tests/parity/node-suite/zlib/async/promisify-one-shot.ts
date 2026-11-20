import * as zlib from "node:zlib";
import { promisify } from "node:util";

const input = Buffer.from("perry zlib promisify coverage");

const gzipOut = await promisify(zlib.gzip)(input);
console.log("gzip promisify:", Buffer.isBuffer(gzipOut), gzipOut.length > 0);

const gunzipOut = await promisify(zlib.gunzip)(zlib.gzipSync(input));
console.log("gunzip promisify:", Buffer.isBuffer(gunzipOut), gunzipOut.toString());

const deflateOut = await promisify(zlib.deflate)(input);
console.log("deflate promisify:", Buffer.isBuffer(deflateOut), deflateOut.length > 0);

const inflateOut = await promisify(zlib.inflate)(zlib.deflateSync(input));
console.log("inflate promisify:", Buffer.isBuffer(inflateOut), inflateOut.toString());

const deflateRawOut = await promisify(zlib.deflateRaw)(input);
console.log("deflateRaw promisify:", Buffer.isBuffer(deflateRawOut), deflateRawOut.length > 0);

const inflateRawOut = await promisify(zlib.inflateRaw)(zlib.deflateRawSync(input));
console.log("inflateRaw promisify:", Buffer.isBuffer(inflateRawOut), inflateRawOut.toString());

const unzipOut = await promisify(zlib.unzip)(zlib.gzipSync(input));
console.log("unzip promisify:", Buffer.isBuffer(unzipOut), unzipOut.toString());

const brotliCompressOut = await promisify(zlib.brotliCompress)(input);
console.log(
  "brotliCompress promisify:",
  Buffer.isBuffer(brotliCompressOut),
  brotliCompressOut.length > 0,
);

const brotliDecompressOut = await promisify(zlib.brotliDecompress)(
  zlib.brotliCompressSync(input),
);
console.log(
  "brotliDecompress promisify:",
  Buffer.isBuffer(brotliDecompressOut),
  brotliDecompressOut.toString(),
);
