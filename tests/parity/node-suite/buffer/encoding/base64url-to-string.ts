import { Buffer } from "node:buffer";

const b = Buffer.from([0xfb, 0xef, 0xbe, 0xfe, 0xff, 0x00]);
console.log("base64url full:", b.toString("base64url"));
console.log("base64url one:", Buffer.from([0xff]).toString("base64url"));
console.log("base64 still padded:", Buffer.from([0xff]).toString("base64"));
