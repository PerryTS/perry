// #1577: a crypto method captured into a variable must stay callable, not
// just report typeof "function". A bound-method call routes through the
// runtime's native-module dispatch, which (post-fix) forwards crypto methods
// to the perry-stdlib impls via a registered dispatcher.
import * as crypto from "node:crypto";

const createHash = crypto.createHash;
const createHmac = crypto.createHmac;
const randomUUID = crypto.randomUUID;
const randomBytes = crypto.randomBytes;
const randomInt = crypto.randomInt;

console.log("hash:", createHash("sha256").update("abc").digest("hex"));
console.log("hmac:", createHmac("sha256", "key").update("abc").digest("hex"));
console.log("uuid len:", randomUUID().length);
console.log("uuid parts:", randomUUID().split("-").length);
console.log("bytes len:", randomBytes(16).length);
console.log("randomInt(5,6):", randomInt(5, 6));
console.log("randomInt(1):", randomInt(1));
