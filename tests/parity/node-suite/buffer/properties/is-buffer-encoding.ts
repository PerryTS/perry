import { Buffer } from "node:buffer";

console.log("isBuffer true:", Buffer.isBuffer(Buffer.from("x")));
console.log("isBuffer false string:", Buffer.isBuffer("x"));
console.log("isBuffer false null:", Buffer.isBuffer(null));
console.log("isEncoding utf8:", Buffer.isEncoding("utf8"));
console.log("isEncoding hex:", Buffer.isEncoding("hex"));
console.log("isEncoding bogus:", Buffer.isEncoding("bogus"));
