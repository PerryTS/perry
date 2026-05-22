import crypto from "node:crypto";
// createCipheriv / createDecipheriv round-trip: decrypting the ciphertext with
// the same key+iv recovers the plaintext.
const key = Buffer.alloc(32, 7);
const iv = Buffer.alloc(16, 3);
const c = crypto.createCipheriv("aes-256-cbc", key, iv);
let enc = c.update("secret", "utf8", "hex");
enc += c.final("hex");
const d = crypto.createDecipheriv("aes-256-cbc", key, iv);
let dec = d.update(enc, "hex", "utf8");
dec += d.final("utf8");
console.log("round-trip:", dec);
