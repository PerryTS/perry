import { Readable } from "node:stream";
import { arrayBuffer, buffer } from "node:stream/consumers";
// arrayBuffer() and buffer() should yield equivalent byte content.
const data = ["abc", "def"];
const r1 = Readable.from(data);
const r2 = Readable.from(data);
const ab = await arrayBuffer(r1);
const buf = await buffer(r2);
console.log("ab byteLength:", ab.byteLength);
console.log("buf length:", buf.length);
console.log("match:", ab.byteLength === buf.length);
