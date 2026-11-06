import * as crypto from "node:crypto";
import { Buffer } from "node:buffer";

async function main() {
  const wrappingKeys = await crypto.subtle.generateKey(
    { name: "RSA-OAEP", modulusLength: 2048, publicExponent: new Uint8Array([1, 0, 1]), hash: "SHA-256" },
    true,
    ["wrapKey", "unwrapKey"],
  );
  const key = await crypto.subtle.importKey(
    "raw",
    Buffer.from("000102030405060708090a0b0c0d0e0f", "hex"),
    { name: "AES-GCM" },
    true,
    ["encrypt", "decrypt"],
  );
  const wrapped = await crypto.subtle.wrapKey("raw", key, wrappingKeys.publicKey, { name: "RSA-OAEP" });
  console.log("rsa oaep wrapped len:", Buffer.from(wrapped).length);
  const unwrapped = await crypto.subtle.unwrapKey(
    "raw",
    wrapped,
    wrappingKeys.privateKey,
    { name: "RSA-OAEP" },
    { name: "AES-GCM" },
    true,
    ["encrypt", "decrypt"],
  );
  const raw = await crypto.subtle.exportKey("raw", unwrapped);
  console.log("rsa oaep unwrap raw hex:", Buffer.from(raw).toString("hex"));
  const iv = Buffer.from("000102030405060708090a0b", "hex");
  const ct = await crypto.subtle.encrypt({ name: "AES-GCM", iv }, unwrapped, new TextEncoder().encode("rsa wrap"));
  const pt = await crypto.subtle.decrypt({ name: "AES-GCM", iv }, unwrapped, ct);
  console.log("rsa oaep unwrap pt:", Buffer.from(pt).toString());
}
await main();
