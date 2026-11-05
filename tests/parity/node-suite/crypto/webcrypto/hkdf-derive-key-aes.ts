import * as crypto from "node:crypto";
import { Buffer } from "node:buffer";

async function main() {
  const ec = new TextEncoder();
  const baseKey = await crypto.subtle.importKey("raw", ec.encode("hello"), { name: "HKDF", hash: "SHA-256" }, false, ["deriveKey"]);
  const key = await crypto.subtle.deriveKey(
    { name: "HKDF", hash: "SHA-256", salt: ec.encode("my friend"), info: ec.encode("there") },
    baseKey,
    { name: "AES-GCM", length: 128 },
    true,
    ["encrypt", "decrypt"],
  );
  const raw = await crypto.subtle.exportKey("raw", key);
  console.log("hkdf aes raw hex:", Buffer.from(raw).toString("hex"));
  const iv = Buffer.from("000102030405060708090a0b", "hex");
  const ct = await crypto.subtle.encrypt({ name: "AES-GCM", iv }, key, ec.encode("hkdf aes"));
  const pt = await crypto.subtle.decrypt({ name: "AES-GCM", iv }, key, ct);
  console.log("hkdf aes pt:", Buffer.from(pt).toString());
}
await main();
