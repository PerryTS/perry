import { Buffer } from "node:buffer";

// Regression for ascii/latin1/binary byteLength: Node returns the string's
// UTF-16 code-unit length, not its scalar (`chars().count()`) length, so an
// astral char (😀, U+1F600) counts as 2.
const emoji = "😀";
console.log("utf8:", Buffer.byteLength(emoji, "utf8"));
console.log("ascii:", Buffer.byteLength(emoji, "ascii"));
console.log("latin1:", Buffer.byteLength(emoji, "latin1"));
console.log("binary:", Buffer.byteLength(emoji, "binary"));
console.log("utf16le:", Buffer.byteLength(emoji, "utf16le"));

const mixed = "a😀b";
console.log("mixed ascii:", Buffer.byteLength(mixed, "ascii"));
console.log("mixed utf16le:", Buffer.byteLength(mixed, "utf16le"));
