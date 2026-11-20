import { Buffer } from "node:buffer";

const source = Buffer.from("copy");
const copy = Buffer.from(source);
source[0] = 0x43;
console.log("source:", source.toString("utf8"));
console.log("copy:", copy.toString("utf8"));
console.log("same object:", source === copy);
