import { deflateSync, deflateRawSync } from "node:zlib";

// Raw deflate output should NOT start with a zlib header (0x78xx).
// Same payload through deflateSync vs deflateRawSync must differ in
// at least the first two bytes for non-empty input.
const sample = Buffer.from("compare headers");
const zlib = deflateSync(sample);
const raw = deflateRawSync(sample);
console.log("zlib first byte 0x78:", zlib[0] === 0x78);
console.log("raw first byte 0x78:", raw[0] === 0x78);
console.log("headers differ:", zlib[0] !== raw[0] || zlib[1] !== raw[1]);
// Raw output is shorter — no 2-byte header, no 4-byte adler32 trailer.
console.log("raw shorter than zlib:", raw.length < zlib.length);
