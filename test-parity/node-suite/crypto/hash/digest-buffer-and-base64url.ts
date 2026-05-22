import * as crypto from "node:crypto";
import { Buffer } from "node:buffer";

const h = crypto.createHash("sha256").update("abc").digest();
console.log("digest is buffer:", Buffer.isBuffer(h));
console.log("digest buffer hex:", Buffer.from(h).toString("hex"));
console.log("digest buffer alias:", Buffer.isBuffer(crypto.createHash("sha256").update("abc").digest("buffer")));
console.log("digest base64url:", crypto.createHash("sha256").update("abc").digest("base64url"));
const oneShot = crypto.hash("sha256", "abc", "buffer");
console.log("hash buffer:", Buffer.isBuffer(oneShot));
console.log("hash buffer hex:", Buffer.from(oneShot).toString("hex"));
console.log("hash base64url:", crypto.hash("sha256", "abc", "base64url"));
