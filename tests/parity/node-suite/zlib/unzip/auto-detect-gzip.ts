import { gzipSync, unzipSync } from "node:zlib";

const input = "unzipSync should auto-detect gzip magic";
const out = unzipSync(gzipSync(Buffer.from(input))).toString();
console.log("unzip(gzip) equal:", out === input);
console.log("unzip(gzip) out:", out);
