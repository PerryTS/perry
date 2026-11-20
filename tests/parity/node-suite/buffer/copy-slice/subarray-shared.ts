import { Buffer } from "node:buffer";

const b = Buffer.from("abcdef");
const s = b.subarray(2, 5);
s[1] = 0x59;
console.log("subarray:", s.toString("utf8"));
console.log("subarray len:", s.length);
