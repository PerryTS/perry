import crypto from "node:crypto";
// AES-256-GCM produces ciphertext + a 16-byte auth tag; decrypting with the
// tag recovers the plaintext.
const key = Buffer.alloc(32, 1);
const iv = Buffer.alloc(12, 2);
const c = crypto.createCipheriv("aes-256-gcm", key, iv);
let enc = c.update("secret", "utf8", "hex");
enc += c.final("hex");
const tag = c.getAuthTag();
const d = crypto.createDecipheriv("aes-256-gcm", key, iv);
d.setAuthTag(tag);
let dec = d.update(enc, "hex", "utf8");
dec += d.final("utf8");
console.log("tag length:", tag.length);
console.log("round-trip:", dec);
