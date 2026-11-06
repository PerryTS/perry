// `subtle.decrypt` with an unknown algorithm name must reject with
// NotSupportedError instead of falling through to AES-GCM key validation.
import { webcrypto } from "node:crypto";

const key = await webcrypto.subtle.generateKey(
  { name: "AES-GCM", length: 128 },
  true,
  ["encrypt", "decrypt"],
);

let rejected = false;
let name = "";
try {
  await webcrypto.subtle.decrypt(
    { name: "BOGUS-CIPHER", iv: new Uint8Array(12) } as any,
    key,
    new Uint8Array([1, 2, 3]),
  );
} catch (e: any) {
  rejected = true;
  name = e?.name ?? "";
}
console.log("rejected:", rejected);
console.log("name:", name);
