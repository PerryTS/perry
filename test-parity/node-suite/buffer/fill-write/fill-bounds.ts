import { Buffer } from "node:buffer";

const b = Buffer.from("abcde");
b.fill(0x78, 1, 4);
console.log("fill bounds:", b.toString("utf8"));
b.fill(0x79, 5, 5);
console.log("fill empty range:", b.toString("utf8"));
