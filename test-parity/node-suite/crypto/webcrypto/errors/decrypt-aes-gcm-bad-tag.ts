// AES-GCM authentication/tag failures are operation-specific failures and
// should reject with OperationError instead of resolving `undefined`.
import { webcrypto } from "node:crypto";

const data = new Uint8Array([1, 2, 3]);
const iv = new Uint8Array(12);
const key = await webcrypto.subtle.generateKey(
  { name: "AES-GCM", length: 128 },
  true,
  ["encrypt", "decrypt"],
);
const ciphertext = new Uint8Array(
  await webcrypto.subtle.encrypt({ name: "AES-GCM", iv }, key, data),
);
ciphertext[ciphertext.length - 1] ^= 1;

let rejected = false;
let name = "";
try {
  await webcrypto.subtle.decrypt({ name: "AES-GCM", iv }, key, ciphertext);
} catch (e: any) {
  rejected = true;
  name = e?.name ?? "";
}
console.log("rejected:", rejected);
console.log("name:", name);
