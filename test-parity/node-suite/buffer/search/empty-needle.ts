import { Buffer } from "node:buffer";

const b = Buffer.from("abc");
console.log("index empty:", b.indexOf(""));
console.log("index empty offset:", b.indexOf("", 2));
console.log("includes empty:", b.includes(""));
