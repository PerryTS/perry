import { Buffer } from "node:buffer";

const b = Buffer.alloc(6);
console.log("ret len:", b.write("longer", 0, 2));
console.log("hex len:", b.toString("hex"));
const h = Buffer.alloc(6);
console.log("ret hex len:", h.write("61626364", 1, 2, "hex"));
console.log("hex hex len:", h.toString("hex"));
const enc = Buffer.alloc(4);
console.log("ret enc:", enc.write("ff", 1, "hex"));
console.log("hex enc:", enc.toString("hex"));
