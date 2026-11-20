// `subtle.unwrapKey` should reject AES-GCM authentication/tag failures with
// OperationError instead of silently resolving `undefined`.
import { webcrypto } from "node:crypto";

const key = await webcrypto.subtle.generateKey(
  { name: "AES-GCM", length: 128 },
  true,
  ["encrypt", "decrypt"],
);
const wrappingKey = await webcrypto.subtle.generateKey(
  { name: "AES-GCM", length: 128 },
  true,
  ["wrapKey", "unwrapKey"],
);
const iv = new Uint8Array(12);
const wrapped = new Uint8Array(
  await webcrypto.subtle.wrapKey("raw", key, wrappingKey, { name: "AES-GCM", iv }),
);
wrapped[wrapped.length - 1] ^= 1;

let rejected = false;
let name = "";
try {
  await webcrypto.subtle.unwrapKey(
    "raw",
    wrapped,
    wrappingKey,
    { name: "AES-GCM", iv },
    { name: "AES-GCM" },
    true,
    ["encrypt"],
  );
} catch (e: any) {
  rejected = true;
  name = e?.name ?? "";
}
console.log("rejected:", rejected);
console.log("name:", name);
