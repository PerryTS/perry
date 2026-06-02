// Regression for the named-`export default function` cross-module
// lowering gap that blocked uuid's `v4()` under `perry.compilePackages`.
// Reproduces the producer/consumer shape of uuid's `rng.js`/`v4.js`:
// a sibling module exports a named-default function, the consumer
// imports it as `default` and invokes it. Pre-fix the call returned
// undefined and the downstream `.length` access threw.
//
// Output must match `node --experimental-strip-types` byte-for-byte.

import rng from "./fixtures/issue_uuid_cross_module/producer.ts";

const r = rng();
console.log(r.length);
