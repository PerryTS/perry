import { Buffer } from "node:buffer";

const a = Buffer.from("XYabcZ");
const b = Buffer.from("--abc--");
console.log("range equal:", a.compare(b, 2, 5, 2, 5));
console.log("range less:", Buffer.from("aa").compare(Buffer.from("bb"), 0, 1, 0, 1));
console.log("range greater:", Buffer.from("bb").compare(Buffer.from("aa"), 0, 1, 0, 1));
