import { Buffer } from "node:buffer";

const src = Buffer.from("abcdef");
const dst = Buffer.alloc(6);
const n = src.copy(dst, 1, 2, 5);
console.log("copied:", n);
console.log("dst:", dst.toString("hex"));
