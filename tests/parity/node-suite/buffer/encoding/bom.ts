import { Buffer } from "node:buffer";

const b = Buffer.from([0xef, 0xbb, 0xbf, 0x61]);
const s = b.toString("utf8");
console.log("bom length:", s.length);
console.log("bom char code:", s.charCodeAt(0));
console.log("bom suffix:", s.slice(1));
