import { Buffer } from "node:buffer";

const parts = [Buffer.from("ab"), Buffer.from("cd")];
const joined = Buffer.concat(parts);
console.log("joined:", joined.toString("hex"));
console.log("manual truncate:", joined.slice(0, 3).toString("hex"));
