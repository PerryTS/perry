import * as crypto from "node:crypto";
import { Buffer } from "node:buffer";

async function main() {
  const key128 = crypto.createSecretKey(Buffer.from("000102030405060708090a0b0c0d0e0f", "hex"));

  const cbcKey = (key128 as any).toCryptoKey("AES-CBC", true, ["encrypt", "decrypt"]);
  const cbcIv = Buffer.from("101112131415161718191a1b1c1d1e1f", "hex");
  const cbcCt = await crypto.subtle.encrypt({ name: "AES-CBC", iv: cbcIv }, cbcKey, new TextEncoder().encode("aes cbc keyobject"));
  const cbcPt = await crypto.subtle.decrypt({ name: "AES-CBC", iv: cbcIv }, cbcKey, cbcCt);
  console.log("toCryptoKey aes-cbc pt:", Buffer.from(cbcPt).toString());

  const ctrKey = (key128 as any).toCryptoKey("AES-CTR", true, ["encrypt", "decrypt"]);
  const counter = Buffer.from("202122232425262728292a2b2c2d2e2f", "hex");
  const ctrCt = await crypto.subtle.encrypt({ name: "AES-CTR", counter, length: 64 }, ctrKey, new TextEncoder().encode("aes ctr keyobject"));
  const ctrPt = await crypto.subtle.decrypt({ name: "AES-CTR", counter, length: 64 }, ctrKey, ctrCt);
  console.log("toCryptoKey aes-ctr pt:", Buffer.from(ctrPt).toString());

  const kwKey = (key128 as any).toCryptoKey("AES-KW", true, ["wrapKey", "unwrapKey"]);
  const wrapped = await crypto.subtle.wrapKey("raw", cbcKey, kwKey, "AES-KW");
  console.log("toCryptoKey aes-kw wrapped len:", Buffer.from(wrapped).length);
  const unwrapped = await crypto.subtle.unwrapKey("raw", wrapped, kwKey, "AES-KW", "AES-CBC", true, ["encrypt", "decrypt"]);
  const raw = await crypto.subtle.exportKey("raw", unwrapped);
  console.log("toCryptoKey aes-kw unwrap raw hex:", Buffer.from(raw).toString("hex"));

  const pbkdfSecret = crypto.createSecretKey(Buffer.from("hello"));
  const pbkdfKey = (pbkdfSecret as any).toCryptoKey("PBKDF2", false, ["deriveBits", "deriveKey"]);
  const bits = await crypto.subtle.deriveBits({ name: "PBKDF2", hash: "SHA-256", salt: new TextEncoder().encode("there"), iterations: 5 }, pbkdfKey, 128);
  console.log("toCryptoKey pbkdf2 bits hex:", Buffer.from(bits).toString("hex"));

  const hkdfSecret = crypto.createSecretKey(Buffer.from("hello"));
  const hkdfKey = (hkdfSecret as any).toCryptoKey("HKDF", false, ["deriveBits", "deriveKey"]);
  const hbits = await crypto.subtle.deriveBits({ name: "HKDF", hash: "SHA-256", salt: new TextEncoder().encode("my friend"), info: new TextEncoder().encode("there") }, hkdfKey, 128);
  console.log("toCryptoKey hkdf bits hex:", Buffer.from(hbits).toString("hex"));
}
await main();
