import { Buffer } from "node:buffer";

const b = Buffer.from(" aGk=!", "base64");
console.log("base64 noise:", b.toString("utf8"));
console.log("base64 noise hex:", b.toString("hex"));
