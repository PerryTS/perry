import { Buffer } from "node:buffer";

const b = Buffer.from("abc");
const ab = new ArrayBuffer(5);
console.log("buffer:", Buffer.byteLength(b));
console.log("arraybuffer:", Buffer.byteLength(ab));
