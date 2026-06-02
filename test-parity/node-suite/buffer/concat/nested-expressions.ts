import { Buffer } from "node:buffer";

const b = Buffer.concat([Buffer.from("ab"), Buffer.from("cd")]).slice(1, 3);
console.log("nested concat slice:", b.toString("utf8"));
console.log("nested len:", b.length);
