import crypto from "node:crypto";
console.log(crypto.createHmac("sha1", "key").update("hello").digest("hex"));
