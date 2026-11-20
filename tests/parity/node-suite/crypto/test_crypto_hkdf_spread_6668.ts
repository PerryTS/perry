// Regression test for #6668: a spread call of a native crypto method
// (`crypto.hkdf(...args, cb)`) must not silently no-op.
//
// The HIR crypto passthrough used to collapse a spread call into a flat
// `Expr::Call`, passing the spread operand as the array itself instead of
// expanding it. The codegen fast-path (`arm_crypto_hkdf_async_alg`) then saw
// too few positional args and returned `undefined` without dispatching, so the
// callback never fired. Spread calls now fall through to `CallSpread`, which
// dispatches through the same bound-native path the value-read form uses.

import * as crypto from "crypto";
import { hkdf } from "crypto";

const ikm = new Uint8Array([1, 2, 3]);
const salt = new Uint8Array(0);
const info = new Uint8Array([4, 5]);

// Sync spread — returns a value, fully deterministic.
// `as const` makes the arg lists tuples so the spreads satisfy the parameter
// signatures under `tsc` (TS2556); runtime is unaffected.
const sargs = ["sha256", ikm, salt, info, 64] as const;
const syncOut = crypto.hkdfSync(...sargs);
console.log("hkdfSync-spread =", new Uint8Array(syncOut).length);

// Async spread forms — collect, then print in a fixed sorted order once every
// callback has fired, so the output is deterministic regardless of scheduling.
const results: Record<string, number> = {};
let remaining = 4;
function record(label: string, err: unknown, out: ArrayBuffer) {
  results[label] = err ? -1 : new Uint8Array(out).length;
  if (--remaining === 0) {
    for (const k of Object.keys(results).sort()) {
      console.log(k, "=", results[k]);
    }
  }
}

// 1. dotted, full spread + trailing regular callback
const a1 = ["sha256", ikm, salt, info, 64] as const;
crypto.hkdf(...a1, (e, out) => record("dotted-spread", e, out));

// 2. dotted, interleaved: regular, regular, spread, regular, callback
const mid = [salt, info] as const;
crypto.hkdf("sha256", ikm, ...mid, 64, (e, out) => record("dotted-interleaved", e, out));

// 3. named import, full spread
const a3 = ["sha256", ikm, salt, info, 64] as const;
hkdf(...a3, (e, out) => record("named-spread", e, out));

// 4. plain direct call (regression guard for the non-spread fast-path)
crypto.hkdf("sha256", ikm, salt, info, 64, (e, out) => record("direct", e, out));
