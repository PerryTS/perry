// `subtle.sign` with a value that isn't a registered CryptoKey must
// reject — Perry previously resolved to `undefined`. Regression cover
// for #1431; see digest-unknown-alg.ts for rationale.
import { webcrypto } from "node:crypto";

const fakeKey = new Uint8Array(32);
const data = new Uint8Array([1, 2, 3]);
let rejected = false;
let nameType = "";
try {
  await webcrypto.subtle.sign("HMAC", fakeKey as any, data);
} catch (e: any) {
  rejected = true;
  nameType = typeof e?.name;
}
console.log("rejected:", rejected);
console.log("name-type:", nameType);
