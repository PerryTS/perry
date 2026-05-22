import crypto from "node:crypto";
console.log(crypto.createHash("md5").update("hello").digest("hex"));
