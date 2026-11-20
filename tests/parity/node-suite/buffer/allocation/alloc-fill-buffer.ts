import { Buffer } from "node:buffer";

const fill = Buffer.from([1, 2]);
const b = Buffer.alloc(5);
b.fill(fill[0]);
console.log("numeric fill from buffer byte:", b.toString("hex"));
console.log("fill source len:", fill.length);
