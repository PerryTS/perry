import { Buffer } from "node:buffer";

console.log("hex odd:", Buffer.byteLength("abc", "hex"));
console.log("hex invalid counts pairs:", Buffer.byteLength("abxxcd", "hex"));
console.log("base64url:", Buffer.byteLength("aGVsbG8", "base64url"));
console.log("latin1:", Buffer.byteLength("Il était tué", "latin1"));
console.log("ascii:", Buffer.byteLength("Il était tué", "ascii"));
console.log("utf16le:", Buffer.byteLength("Il était tué", "utf16le"));
