import * as zlib from "node:zlib";

const gzip = zlib.createGzip();

console.log("close typeof:", typeof gzip.close);
console.log("destroy typeof:", typeof gzip.destroy);
