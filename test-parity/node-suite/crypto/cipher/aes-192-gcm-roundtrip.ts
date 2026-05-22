import * as crypto from "node:crypto";

const key = Buffer.alloc(24, 7);
const iv = Buffer.alloc(12, 8);
const aad = Buffer.from("aes-192-gcm aad");
const plaintext = Buffer.from("aes-192-gcm parity plaintext");

const cipher = crypto.createCipheriv("aes-192-gcm", key, iv);
cipher.setAAD(aad);
const ciphertext = Buffer.concat([cipher.update(plaintext), cipher.final()]);
const tag = cipher.getAuthTag();

const decipher = crypto.createDecipheriv("aes-192-gcm", key, iv);
decipher.setAAD(aad);
decipher.setAuthTag(tag);
const roundtrip = Buffer.concat([decipher.update(ciphertext), decipher.final()]);

console.log("aes192gcm ct len:", ciphertext.length);
console.log("aes192gcm tag len:", tag.length);
console.log("aes192gcm roundtrip:", roundtrip.toString());
