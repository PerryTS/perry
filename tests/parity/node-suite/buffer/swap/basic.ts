import { Buffer } from "node:buffer";

const a = Buffer.from("00112233", "hex");
const b = Buffer.from("0011223344556677", "hex");
const c = Buffer.from("0011223344556677", "hex");
console.log("swap16 same:", a.swap16() === a);
console.log("swap16:", a.toString("hex"));
b.swap32();
c.swap64();
console.log("swap32:", b.toString("hex"));
console.log("swap64:", c.toString("hex"));
