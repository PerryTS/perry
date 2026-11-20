import { Buffer } from "node:buffer";

const b = Buffer.from("abcdef");
console.log("range 1-4:", b.toString("utf8", 1, 4));
console.log("range 2-end:", b.toString("utf8", 2));
console.log("range empty:", b.toString("utf8", 4, 2));
