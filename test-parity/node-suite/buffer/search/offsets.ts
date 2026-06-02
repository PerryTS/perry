import { Buffer } from "node:buffer";

const b = Buffer.from("abcabc");
console.log("index offset:", b.indexOf("ab", 1));
console.log("index negative:", b.indexOf("ab", -4));
console.log("includes negative:", b.includes("bc", -5));
console.log("includes infinity:", b.includes("a", Infinity));
