import { Buffer } from "node:buffer";

const b = Buffer.from("010203040506", "hex");
console.log("uintBE3:", b.readUIntBE(0, 3));
console.log("uintLE3:", b.readUIntLE(0, 3));
console.log("intBE3:", b.readIntBE(3, 3));
console.log("intLE3:", b.readIntLE(3, 3));
