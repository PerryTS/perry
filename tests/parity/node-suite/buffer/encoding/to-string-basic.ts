import { Buffer } from "node:buffer";

const b = Buffer.from("68656c6c6f", "hex");
console.log("utf8:", b.toString("utf8"));
console.log("hex:", b.toString("hex"));
console.log("base64:", b.toString("base64"));
