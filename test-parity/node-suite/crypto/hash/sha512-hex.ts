import crypto from "node:crypto";
console.log(crypto.createHash("sha512").update("hello").digest("hex"));
