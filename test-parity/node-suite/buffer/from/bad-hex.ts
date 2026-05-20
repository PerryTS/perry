import { Buffer } from "node:buffer";

const odd = Buffer.from("abc", "hex");
const invalid = Buffer.from("zz", "hex");
const mixed = Buffer.from("abxxcd", "hex");
console.log("odd hex:", odd.length, odd.toString("hex"));
console.log("invalid hex length:", invalid.length);
console.log("mixed hex:", mixed.length, mixed.toString("hex"));
