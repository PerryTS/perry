// `subtle.wrapKey` must reject with InvalidAccessError when the wrapping
// algorithm does not match the wrapping CryptoKey. Regression coverage for
// #1431's inner wrap failure paths.
import { webcrypto } from "node:crypto";

const key = await webcrypto.subtle.generateKey(
  { name: "AES-GCM", length: 128 },
  true,
  ["encrypt", "decrypt"],
);
const aesKwKey = await webcrypto.subtle.generateKey(
  { name: "AES-KW", length: 128 },
  true,
  ["wrapKey", "unwrapKey"],
);

let rejected = false;
let name = "";
try {
  await webcrypto.subtle.wrapKey(
    "raw",
    key,
    aesKwKey,
    { name: "AES-GCM", iv: new Uint8Array(12) },
  );
} catch (e: any) {
  rejected = true;
  name = e?.name ?? "";
}
console.log("rejected:", rejected);
console.log("name:", name);
