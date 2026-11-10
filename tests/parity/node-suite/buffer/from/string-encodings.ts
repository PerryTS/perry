import { Buffer } from "node:buffer";

console.log("utf8:", Buffer.from("hé", "utf8").toString("hex"));
console.log("hex:", Buffer.from("6869", "hex").toString("utf8"));
console.log("base64:", Buffer.from("aGk=", "base64").toString("utf8"));
console.log("base64url:", Buffer.from("aGk", "base64url").toString("utf8"));
