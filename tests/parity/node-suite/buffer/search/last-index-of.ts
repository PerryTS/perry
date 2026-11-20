import { Buffer } from "node:buffer";

const b = Buffer.from("abcabc");
console.log("last str:", b.lastIndexOf("bc"));
console.log("last offset:", b.lastIndexOf("bc", 2));
console.log("last missing:", b.lastIndexOf("zz"));
