import { deflateSync } from "node:zlib";

// zlib format always starts with two bytes: CMF (typically 0x78 for default
// deflate window) followed by FLG. The low nibble of CMF is the compression
// method (8 = deflate) and the high nibble is window size minus 8.
const compressed = deflateSync(Buffer.from("zlib header test"));
console.log("cmf low nibble:", compressed[0] & 0x0f);
console.log("cmf high nibble:", compressed[0] >> 4);
// (CMF * 256 + FLG) must be a multiple of 31 per RFC 1950.
const check = (compressed[0] * 256 + compressed[1]) % 31;
console.log("rfc1950 check mod31:", check);
