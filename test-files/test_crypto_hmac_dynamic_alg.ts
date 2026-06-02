// Issue #1076 — crypto.createHmac(alg, key) silently returned "" when
// `alg` wasn't an inline string literal (const-bound, for-of-bound,
// ternary). All four cases below must print the same 64-char hex digest
// for the sha256/HMAC-SHA256 of "payload" under key "secret".
//
// Reference (Node):
//   sha256("payload", "secret") =>
//     b82fcb791acec57859b989b430a826488ce2e479fdf92326bd0a2e8375a42ba4

import * as crypto from "node:crypto";

const key = "secret";
const data = "payload";

// (1) inline literal — was the only working path
console.log("(1)", crypto.createHmac("sha256", key).update(data).digest("hex"));

// (2) for-of bound — was returning ""
for (const alg of ["sha256", "sha1"]) {
    console.log("(2)", alg, crypto.createHmac(alg, key).update(data).digest("hex"));
}

// (3) const reference — was returning ""
const alg = "sha256";
console.log("(3)", crypto.createHmac(alg, key).update(data).digest("hex"));

// (4) ternary
const useStrong = true;
console.log("(4)", crypto.createHmac(useStrong ? "sha256" : "sha1", key).update(data).digest("hex"));

// (5) bound-then-used (createHmac result stored in a local first, then chained)
const h = crypto.createHmac("sha256", key);
h.update(data);
console.log("(5)", h.digest("hex"));

// (6) ditto for createHash to exercise the same handle-runtime fallback
const alg2 = "sha256";
console.log("(6)", crypto.createHash(alg2).update(data).digest("hex"));
