// `subtle.sign` must reject with InvalidAccessError when the algorithm
// does not match the provided CryptoKey. Regression coverage for #1431:
// Perry used to resolve `undefined` for this inner key/algorithm mismatch.
import { webcrypto } from "node:crypto";

const data = new Uint8Array([1, 2, 3]);
const aesKey = await webcrypto.subtle.generateKey(
  { name: "AES-GCM", length: 128 },
  true,
  ["encrypt", "decrypt"],
);

let rejected = false;
let name = "";
try {
  await webcrypto.subtle.sign("HMAC", aesKey, data);
} catch (e: any) {
  rejected = true;
  name = e?.name ?? "";
}
console.log("rejected:", rejected);
console.log("name:", name);
