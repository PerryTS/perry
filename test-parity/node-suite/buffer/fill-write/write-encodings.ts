import { Buffer } from "node:buffer";

const hex = Buffer.alloc(4);
const base64 = Buffer.alloc(4);
console.log("hex n:", hex.write("abcd", 0, "hex"));
console.log("hex data:", hex.toString("hex"));
console.log("base64 n:", base64.write("aGk=", 0, "base64"));
console.log("base64 data:", base64.toString("hex"));
