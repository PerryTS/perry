import * as crypto from "node:crypto";
import { Buffer } from "node:buffer";

console.log("scrypt 32:", crypto.scryptSync("password", "salt", 32).toString("hex"));
console.log("scrypt 16:", crypto.scryptSync("password", "salt", 16).toString("hex"));
console.log("buffer args:", crypto.scryptSync(Buffer.from("password"), Buffer.from("salt"), 16).toString("hex"));
