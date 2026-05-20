import { Buffer } from "node:buffer";

const plain = Buffer.from("aGVsbG8", "base64url");
const urlSafe = Buffer.from("--__", "base64url");
console.log("base64url plain:", plain.toString("utf8"));
console.log("base64url safe hex:", urlSafe.toString("hex"));
