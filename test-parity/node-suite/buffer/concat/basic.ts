import { Buffer } from "node:buffer";

const a = Buffer.from("ab");
const b = Buffer.from("cd");
console.log("concat:", Buffer.concat([a, b]).toString("utf8"));
console.log("empty:", Buffer.concat([]).length);
console.log("single equal:", Buffer.concat([a]).toString("utf8"));
