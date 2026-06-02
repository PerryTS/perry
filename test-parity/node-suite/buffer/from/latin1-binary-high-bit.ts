import { Buffer } from "node:buffer";

const latin1 = Buffer.from("\x00\x7f\x80\x89\xff", "latin1");
const binary = Buffer.from("\x00\x7f\x80\x89\xff", "binary");
const ascii = Buffer.from("\x80\xff", "ascii");

console.log("latin1:", latin1.toString("hex"));
console.log("binary:", binary.toString("hex"));
console.log("ascii:", ascii.toString("hex"));
console.log("roundtrip:", Buffer.from([0x80, 0x89, 0xff]).toString("latin1").charCodeAt(1));
