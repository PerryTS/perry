import * as crypto from "node:crypto";
import { Buffer } from "node:buffer";

const h = crypto.createHmac("sha256", "key").update("abc");
console.log("hex:", h.digest("hex"));
console.log("base64:", crypto.createHmac("sha256", "key").update("abc").digest("base64"));
console.log("base64url:", crypto.createHmac("sha256", "key").update("abc").digest("base64url"));
console.log("buffer hex:", Buffer.from(crypto.createHmac("sha256", "key").update("abc").digest()).toString("hex"));
