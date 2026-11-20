import { Buffer } from "node:buffer";

const utf8 = Buffer.from("aaaaa");
const hex = Buffer.from("ababab", "hex");
const base64 = Buffer.from("YWFh", "base64");
console.log("utf8 materialized:", utf8.toString("hex"));
console.log("hex materialized:", hex.toString("hex"));
console.log("base64 materialized:", base64.toString("hex"));
