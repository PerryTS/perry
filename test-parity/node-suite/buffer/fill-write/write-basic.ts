import { Buffer } from "node:buffer";

const b = Buffer.alloc(8);
console.log("write n:", b.write("hi", 1));
console.log("write hex:", b.toString("hex"));
