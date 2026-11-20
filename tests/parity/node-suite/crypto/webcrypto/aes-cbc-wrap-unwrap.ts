import * as crypto from "node:crypto";
import { Buffer } from "node:buffer";

async function main() {
  const target = await crypto.subtle.generateKey({ name: "AES-GCM", length: 128 }, true, ["encrypt", "decrypt"]);
  const wrapping = await crypto.subtle.importKey(
    "raw",
    Buffer.from("000102030405060708090a0b0c0d0e0f", "hex"),
    "AES-CBC",
    true,
    ["wrapKey", "unwrapKey"],
  );
  const iv = Buffer.from("101112131415161718191a1b1c1d1e1f", "hex");
  const wrapped = await crypto.subtle.wrapKey("raw", target, wrapping, { name: "AES-CBC", iv });
  console.log("cbc wrapped len:", Buffer.from(wrapped).length);
  const unwrapped = await crypto.subtle.unwrapKey(
    "raw",
    wrapped,
    wrapping,
    { name: "AES-CBC", iv },
    "AES-GCM",
    true,
    ["encrypt", "decrypt"],
  );
  console.log("cbc unwrap raw match:", Buffer.from(await crypto.subtle.exportKey("raw", target)).equals(Buffer.from(await crypto.subtle.exportKey("raw", unwrapped))));
}
await main();
