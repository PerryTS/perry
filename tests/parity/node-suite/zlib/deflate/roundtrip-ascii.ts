import { deflateSync, inflateSync } from "node:zlib";

const input = "the quick brown fox jumps over the lazy dog";
const out = inflateSync(deflateSync(Buffer.from(input))).toString();
console.log("deflate roundtrip equal:", out === input);
console.log("deflate roundtrip out:", out);
