import * as crypto from "node:crypto";
import { Buffer } from "node:buffer";

const h = "abcdef";
console.log("sha1 hex:", crypto.createHash("sha1").update(h).digest("hex"));
console.log("sha1 base64:", crypto.createHash("sha1").update(h).digest("base64"));
console.log("sha1 base64url:", crypto.createHash("sha1").update(h).digest("base64url"));
console.log("sha256 buffer hex:", Buffer.from(crypto.createHash("sha256").update(h).digest()).toString("hex"));
