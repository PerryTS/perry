import { Buffer } from "node:buffer";

console.log("utf-8:", Buffer.isEncoding("utf-8"));
console.log("base64url:", Buffer.isEncoding("base64url"));
console.log("latin1:", Buffer.isEncoding("latin1"));
console.log("binary:", Buffer.isEncoding("binary"));
console.log("ucs2:", Buffer.isEncoding("ucs2"));
console.log("utf16le:", Buffer.isEncoding("utf16le"));
console.log("empty:", Buffer.isEncoding(""));
