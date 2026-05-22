import crypto from "node:crypto";
// AES-128-CBC round-trip (16-byte key) recovers the plaintext.
const key = Buffer.alloc(16, 5);
const iv = Buffer.alloc(16, 9);
const c = crypto.createCipheriv("aes-128-cbc", key, iv);
let enc = c.update("hi", "utf8", "hex");
enc += c.final("hex");
const d = crypto.createDecipheriv("aes-128-cbc", key, iv);
let dec = d.update(enc, "hex", "utf8");
dec += d.final("utf8");
console.log("round-trip:", dec);
