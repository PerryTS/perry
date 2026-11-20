import * as crypto from "node:crypto";
import { Buffer } from "node:buffer";

console.log("buffer input:", crypto.createHash("sha1").update(Buffer.from("abc")).digest("hex"));
console.log("uint8 input:", crypto.createHash("sha256").update(new Uint8Array([97, 98, 99])).digest("hex"));
