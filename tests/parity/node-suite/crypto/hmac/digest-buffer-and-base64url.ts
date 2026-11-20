import * as crypto from "node:crypto";
import { Buffer } from "node:buffer";

const h = crypto.createHmac("sha256", "key").update("abc").digest();
console.log("digest is buffer:", Buffer.isBuffer(h));
console.log("digest buffer hex:", Buffer.from(h).toString("hex"));
const h2 = crypto.createHmac("sha256", "key").update("abc").digest("buffer");
console.log("digest buffer alias:", Buffer.isBuffer(h2));
console.log("digest buffer alias hex:", Buffer.from(h2).toString("hex"));
console.log("digest base64url:", crypto.createHmac("sha256", "key").update("abc").digest("base64url"));
