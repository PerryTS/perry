import { deflateSync, unzipSync } from "node:zlib";

const input = "unzipSync should accept zlib-format deflate too";
const out = unzipSync(deflateSync(Buffer.from(input))).toString();
console.log("unzip(deflate) equal:", out === input);
console.log("unzip(deflate) out:", out);
