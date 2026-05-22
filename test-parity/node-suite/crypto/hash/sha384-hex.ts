import crypto from "node:crypto";
console.log(crypto.createHash("sha384").update("hello").digest("hex"));
