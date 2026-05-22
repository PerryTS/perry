import * as crypto from "node:crypto";
import { Buffer } from "node:buffer";

async function main() {
  const keyBytes = Buffer.from("000102030405060708090a0b0c0d0e0f", "hex");
  const counter = Buffer.from("f0f1f2f3f4f5f6f7f8f9fafbfcfdfeff", "hex");
  const data = Buffer.from("6bc1bee22e409f96e93d7e117393172a", "hex");
  const key = await crypto.subtle.importKey("raw", keyBytes, "AES-CTR", true, ["encrypt", "decrypt"]);
  const ct = await crypto.subtle.encrypt({ name: "AES-CTR", counter, length: 64 }, key, data);
  console.log("ctr ct hex:", Buffer.from(ct).toString("hex"));
  const pt = await crypto.subtle.decrypt({ name: "AES-CTR", counter, length: 64 }, key, ct);
  console.log("ctr pt hex:", Buffer.from(pt).toString("hex"));
  const raw = await crypto.subtle.exportKey("raw", key);
  console.log("ctr raw len:", Buffer.from(raw).length);
}
await main();
