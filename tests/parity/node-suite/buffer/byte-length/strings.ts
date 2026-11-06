import { Buffer } from "node:buffer";

console.log("ascii:", Buffer.byteLength("hello", "utf8"));
console.log("unicode:", Buffer.byteLength("héllo", "utf8"));
console.log("emoji:", Buffer.byteLength("😀", "utf8"));
