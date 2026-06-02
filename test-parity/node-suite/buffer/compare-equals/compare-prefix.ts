import { Buffer } from "node:buffer";

const a = Buffer.from("abc");
const b = Buffer.from("abcd");
const c = Buffer.from("abb");
console.log("prefix:", a.compare(b));
console.log("greater:", a.compare(c));
console.log("static prefix:", Buffer.compare(a, b));
