// `subtle.digest` with an unknown algorithm name must reject (Perry
// previously resolved to `undefined`, which silently turned consumer
// `.catch(e => e.name === "...")` checks into `undefined.name`).
// Node rejects with `DOMException("NotSupportedError")` here, so we
// assert the rejection occurred and `e.name` is a non-empty string.
// Exact-name parity across all sites is the long-term goal of #1431;
// this test pins down the no-silent-resolve invariant.
import { webcrypto } from "node:crypto";

const data = new Uint8Array([1, 2, 3]);
let rejected = false;
let nameType = "";
try {
  await webcrypto.subtle.digest("BOGUS-256", data);
} catch (e: any) {
  rejected = true;
  nameType = typeof e?.name;
}
console.log("rejected:", rejected);
console.log("name-type:", nameType);
