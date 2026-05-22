import crypto from "node:crypto";
console.log(crypto.createHash("sha1").update("hello").digest("hex"));
