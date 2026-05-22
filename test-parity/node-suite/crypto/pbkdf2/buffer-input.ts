import * as crypto from "node:crypto";
import { Buffer } from "node:buffer";

console.log("buffer args:", crypto.pbkdf2Sync(Buffer.from("password"), Buffer.from("salt"), 1, 16, "sha256").toString("hex"));
console.log("uint8 args:", crypto.pbkdf2Sync(new Uint8Array([112,97,115,115]), new Uint8Array([115,97,108,116]), 1, 16, "sha256").toString("hex"));
