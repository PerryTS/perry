import * as crypto from "node:crypto";
import { Buffer } from "node:buffer";

async function main() {
  const keyBytes = Buffer.from("000102030405060708090a0b0c0d0e0f", "hex");
  const jwk = { kty: "oct", k: keyBytes.toString("base64url") };
  const key = await crypto.subtle.importKey("jwk", jwk, "AES-GCM", true, ["encrypt", "decrypt"]);
  const exported = await crypto.subtle.exportKey("jwk", key);
  console.log("jwk aes kty:", exported.kty);
  console.log("jwk aes roundtrip:", exported.k === jwk.k);
  const iv = Buffer.from("101112131415161718191a1b", "hex");
  const ct = await crypto.subtle.encrypt({ name: "AES-GCM", iv }, key, new TextEncoder().encode("jwk aes"));
  const pt = await crypto.subtle.decrypt({ name: "AES-GCM", iv }, key, ct);
  console.log("jwk aes pt:", Buffer.from(pt).toString());
}
await main();
