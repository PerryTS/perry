import { Buffer } from "node:buffer";

const a = Buffer.from("abc");
const b = Buffer.from("abd");
console.log("a b:", Buffer.compare(a, b));
console.log("a a:", Buffer.compare(a, a));
console.log("b a:", Buffer.compare(b, a));
