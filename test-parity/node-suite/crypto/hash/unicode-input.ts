import * as crypto from "node:crypto";

console.log("utf8:", crypto.createHash("sha512").update("УТФ-8 text").digest("hex"));
console.log("emoji:", crypto.createHash("sha256").update("hello 🌍").digest("hex"));
