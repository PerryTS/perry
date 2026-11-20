import * as crypto from "node:crypto";
import { Buffer } from "node:buffer";

console.log("sha256:", Buffer.from(crypto.hkdfSync("sha256", "ikm", "salt", "info", 32)).toString("hex"));
console.log("sha512:", Buffer.from(crypto.hkdfSync("sha512", "ikm", "salt", "info", 42)).toString("hex"));
console.log("buffer args:", Buffer.from(crypto.hkdfSync("sha256", Buffer.from("ikm"), Buffer.from("salt"), Buffer.from("info"), 16)).toString("hex"));
console.log("empty salt/info:", Buffer.from(crypto.hkdfSync("sha256", "ikm", "", "", 16)).toString("hex"));
