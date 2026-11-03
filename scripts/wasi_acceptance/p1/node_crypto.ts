// The crypto module, bundled into the runtime: digests (including the wide
// ones), HMAC, key derivation and random bytes, all with fixed answers.
import * as crypto from "node:crypto";

console.log(crypto.createHash("sha256").update("perry").digest("hex"));
console.log(crypto.createHash("sha512").update("perry").digest("hex").slice(0, 32));
console.log(crypto.createHash("sha384").update("perry").digest("base64").slice(0, 24));
console.log(crypto.createHash("md5").update("a").update("b").digest("hex"));
console.log(crypto.createHmac("sha256", "key").update("data").digest("hex"));
console.log(crypto.pbkdf2Sync("pw", "salt", 1000, 16, "sha256").toString("hex"));
console.log(crypto.randomBytes(16).length, crypto.randomUUID().length);
console.log(crypto.timingSafeEqual(Buffer.from("abc"), Buffer.from("abc")));
