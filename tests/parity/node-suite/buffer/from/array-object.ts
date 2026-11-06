import { Buffer } from "node:buffer";

const a = Buffer.from([0x61, 0x62, 0x1ff]);
const empty = Buffer.from([]);
console.log("array hex:", a.toString("hex"));
console.log("empty array len:", empty.length);
