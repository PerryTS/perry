import { Buffer } from "node:buffer";

const hex = Buffer.from("68656c6c6f", "hex");
const base64 = Buffer.from("aGVsbG8=", "base64");
const latin1 = Buffer.from("héllo", "utf8");
console.log("hex buffer length:", hex.length);
console.log("base64 buffer length:", base64.length);
console.log("utf8 byteLength:", Buffer.byteLength(latin1.toString("utf8")));
