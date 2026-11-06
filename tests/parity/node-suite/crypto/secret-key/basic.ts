import * as crypto from "node:crypto";

const key: any = crypto.createSecretKey("secret");
console.log("key typeof:", typeof key);
console.log("hmac with key:", crypto.createHmac("sha256", key).update("abc").digest("hex"));
