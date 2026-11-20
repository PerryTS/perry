import { Buffer } from "node:buffer";

const b = Buffer.from([1, 2, 3]);
console.log("json-compatible data:", Array.from(b).join(","));
console.log("string data:", b.toString("hex"));
