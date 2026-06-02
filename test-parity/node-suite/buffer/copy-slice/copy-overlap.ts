import { Buffer } from "node:buffer";

const b = Buffer.from("abcdef");
console.log("copy ret:", b.copy(b, 2, 0, 4));
console.log("overlap:", b.toString());
const c = Buffer.from("abcdef");
console.log("copy empty:", c.copy(c, 0, 2, 2), c.toString());
