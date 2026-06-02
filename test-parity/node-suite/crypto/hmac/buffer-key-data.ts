import * as crypto from "node:crypto";
import { Buffer } from "node:buffer";

console.log("buffer key:", crypto.createHmac("sha256", Buffer.from("key")).update("abc").digest("hex"));
console.log("buffer data:", crypto.createHmac("sha256", "key").update(Buffer.from("abc")).digest("hex"));
console.log("uint8 key:", crypto.createHmac("sha1", new Uint8Array([107,101,121])).update(new Uint8Array([97,98,99])).digest("hex"));
