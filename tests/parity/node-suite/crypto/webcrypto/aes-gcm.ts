import * as crypto from "node:crypto";
import { Buffer } from "node:buffer";

async function main() {
  const keyBytes = Buffer.from("000102030405060708090a0b0c0d0e0f", "hex");
  const iv = Buffer.from("101112131415161718191a1b", "hex");
  const aad = Buffer.from("feedfacedeadbeef", "hex");
  const data = new TextEncoder().encode("hello aes-gcm subtle");
  const key = await crypto.subtle.importKey("raw", keyBytes, "AES-GCM", false, ["encrypt", "decrypt"]);
  const ct = await crypto.subtle.encrypt({ name: "AES-GCM", iv, additionalData: aad }, key, data);
  console.log("ct hex:", Buffer.from(ct).toString("hex"));
  const pt = await crypto.subtle.decrypt({ name: "AES-GCM", iv, additionalData: aad }, key, ct);
  console.log("pt:", Buffer.from(pt).toString());
}
await main();
