import crypto from "node:crypto";
console.log(crypto.createHash("sha224").update("hello").digest("hex"));
