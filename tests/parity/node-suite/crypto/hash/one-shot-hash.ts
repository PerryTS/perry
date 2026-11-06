import * as crypto from "node:crypto";
import { hash } from "node:crypto";
import { Buffer } from "node:buffer";

console.log("default sha256:", crypto.hash("sha256", "abc"));
console.log("hex sha1:", crypto.hash("sha1", "abc", "hex"));
console.log("base64 sha256:", crypto.hash("sha256", "abc", "base64"));
console.log("buffer input:", crypto.hash("sha256", Buffer.from("abc"), "hex"));
console.log("named:", hash("sha512", "abc", "hex"));
