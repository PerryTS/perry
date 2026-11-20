import * as crypto from "node:crypto";
import { Buffer } from "node:buffer";

async function main() {
  const keyBytes = Buffer.from("000102030405060708090a0b0c0d0e0f", "hex");
  const iv = Buffer.from("101112131415161718191a1b1c1d1e1f", "hex");
  const data = new TextEncoder().encode("hello aes-cbc subtle");
  const key = await crypto.subtle.importKey("raw", keyBytes, "AES-CBC", true, ["encrypt", "decrypt"]);
  const ct = await crypto.subtle.encrypt({ name: "AES-CBC", iv }, key, data);
  console.log("cbc ct len:", Buffer.from(ct).length);
  console.log("cbc ct hex:", Buffer.from(ct).toString("hex"));
  const pt = await crypto.subtle.decrypt({ name: "AES-CBC", iv }, key, ct);
  console.log("cbc pt:", Buffer.from(pt).toString());
  const raw = await crypto.subtle.exportKey("raw", key);
  console.log("cbc raw len:", Buffer.from(raw).length);
}
await main();
