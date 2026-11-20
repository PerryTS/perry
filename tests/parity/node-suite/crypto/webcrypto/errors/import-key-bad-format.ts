// `subtle.importKey` with an unsupported `format` must reject — Perry
// previously resolved to `undefined`, so any caller catching with
// `e.name` got `undefined`. Regression cover for #1431; see
// digest-unknown-alg.ts for the no-silent-resolve invariant rationale.
import { webcrypto } from "node:crypto";

const raw = new Uint8Array(32);
let rejected = false;
let nameType = "";
try {
  await webcrypto.subtle.importKey(
    "bogus" as any,
    raw,
    { name: "AES-GCM" },
    false,
    ["encrypt"],
  );
} catch (e: any) {
  rejected = true;
  nameType = typeof e?.name;
}
console.log("rejected:", rejected);
console.log("name-type:", nameType);
