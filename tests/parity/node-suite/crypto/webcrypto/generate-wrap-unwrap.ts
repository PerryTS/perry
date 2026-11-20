import * as crypto from "node:crypto";
import { Buffer } from "node:buffer";

async function main() {
  const wrappingKey = await crypto.subtle.importKey("raw", Buffer.from("000102030405060708090a0b0c0d0e0f", "hex"), "AES-KW", true, ["wrapKey", "unwrapKey"]);
  const key = await crypto.subtle.generateKey({ name: "AES-GCM", length: 128 }, true, ["encrypt", "decrypt"]);
  const wrapped = await crypto.subtle.wrapKey("raw", key, wrappingKey, { name: "AES-KW" });
  console.log("wrapped len:", Buffer.from(wrapped).length);
  const unwrapped = await crypto.subtle.unwrapKey("raw", wrapped, wrappingKey, { name: "AES-KW" }, "AES-GCM", true, ["encrypt", "decrypt"]);
  const iv = Buffer.from("101112131415161718191a1b", "hex");
  const ct = await crypto.subtle.encrypt({ name: "AES-GCM", iv }, unwrapped, new TextEncoder().encode("wrapped key"));
  const pt = await crypto.subtle.decrypt({ name: "AES-GCM", iv }, unwrapped, ct);
  console.log("roundtrip:", Buffer.from(pt).toString());
}
await main();
