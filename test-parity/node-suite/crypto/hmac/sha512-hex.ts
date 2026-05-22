import crypto from "node:crypto";
console.log(crypto.createHmac("sha512", "key").update("hello").digest("hex"));
