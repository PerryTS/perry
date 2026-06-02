import { Buffer } from "node:buffer";

const a = Buffer.from("ref");
const b = Buffer.alloc(3, 0x41);
console.log("from direct:", a.toString("utf8"));
console.log("alloc direct:", b.toString("hex"));
console.log("isBuffer direct:", Buffer.isBuffer(a));
