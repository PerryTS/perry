import { Buffer } from "node:buffer";

const b = Buffer.from("abcdef");
const s = b.slice(1, 4);
s[0] = 0x5a;
console.log("slice:", s.toString("utf8"));
console.log("slice len:", s.length);
