import { deflateRawSync, inflateRawSync } from "node:zlib";

const input = "raw deflate has no zlib header or adler32";
const out = inflateRawSync(deflateRawSync(Buffer.from(input))).toString();
console.log("raw roundtrip equal:", out === input);
console.log("raw roundtrip out:", out);
