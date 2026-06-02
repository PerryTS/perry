// `subtle.encrypt` must reject with InvalidAccessError when the requested
// cipher does not match the CryptoKey's algorithm. Regression coverage for
// #1431's remaining inner failure audit.
import { webcrypto } from "node:crypto";

const data = new Uint8Array([1, 2, 3]);
const aesKwKey = await webcrypto.subtle.generateKey(
  { name: "AES-KW", length: 128 },
  true,
  ["wrapKey", "unwrapKey"],
);

let rejected = false;
let name = "";
try {
  await webcrypto.subtle.encrypt(
    { name: "AES-GCM", iv: new Uint8Array(12) },
    aesKwKey,
    data,
  );
} catch (e: any) {
  rejected = true;
  name = e?.name ?? "";
}
console.log("rejected:", rejected);
console.log("name:", name);
