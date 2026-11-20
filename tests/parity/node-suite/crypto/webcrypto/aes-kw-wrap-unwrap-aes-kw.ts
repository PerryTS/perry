import * as crypto from "node:crypto";
import { Buffer } from "node:buffer";

async function main() {
  const wrapping = await crypto.subtle.importKey(
    "raw",
    Buffer.from("000102030405060708090a0b0c0d0e0f", "hex"),
    "AES-KW",
    true,
    ["wrapKey", "unwrapKey"],
  );
  const target = await crypto.subtle.importKey(
    "raw",
    Buffer.from("101112131415161718191a1b1c1d1e1f", "hex"),
    "AES-KW",
    true,
    ["wrapKey", "unwrapKey"],
  );
  const wrapped = await crypto.subtle.wrapKey("raw", target, wrapping, "AES-KW");
  const unwrapped = await crypto.subtle.unwrapKey("raw", wrapped, wrapping, "AES-KW", "AES-KW", true, ["wrapKey", "unwrapKey"]);
  const raw = await crypto.subtle.exportKey("raw", unwrapped);
  console.log("aes-kw wrap len:", Buffer.from(wrapped).length);
  console.log("aes-kw unwrap raw:", Buffer.from(raw).toString("hex"));
}
await main();
