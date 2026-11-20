import { Buffer } from "node:buffer";

const b = Buffer.from([0xe2, 0x28, 0xa1]);
const s = b.toString("utf8");
console.log("invalid length:", s.length);
console.log("replacement first:", s.charCodeAt(0));
console.log("middle:", s.charCodeAt(1));
