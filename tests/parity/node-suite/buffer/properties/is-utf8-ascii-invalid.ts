import { Buffer, isAscii, isUtf8 } from "node:buffer";

const valid = Buffer.from("hello");
const utf8 = Buffer.from("hé");
const invalid = Buffer.from([0xc3, 0x28]);
const empty = Buffer.alloc(0);
console.log("ascii valid:", isAscii(valid), isUtf8(valid));
console.log("utf8 non-ascii:", isAscii(utf8), isUtf8(utf8));
console.log("invalid:", isAscii(invalid), isUtf8(invalid));
console.log("empty:", isAscii(empty), isUtf8(empty));
