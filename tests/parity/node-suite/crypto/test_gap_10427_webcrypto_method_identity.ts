// #10427: reading a Web Crypto method off `globalThis.crypto` (and
// `crypto.subtle`) allocated a fresh closure on every access, so the method
// had no stable identity (`crypto.randomUUID === crypto.randomUUID` was
// `false`) and every read allocated. Root cause: `webcrypto_method_value` /
// `subtle_crypto_method_value` in
// crates/perry-runtime/src/object/global_this/ctor_thunks.rs called plain
// `js_closure_alloc` instead of the func-ptr-keyed `js_closure_alloc_singleton`
// cache. This covers every member of `globalThis.crypto` (`randomUUID`,
// `getRandomValues`, `subtle` itself, and `subtle`'s methods including the
// KEM pair that shared the same defect) plus `node:crypto`'s default import
// as an already-correct control.
import nodeCrypto from "node:crypto";
import { randomBytes as namedRandomBytes, randomUUID as namedRandomUUID } from "node:crypto";

// The issue's own repro, verbatim.
console.log("randomUUID stable:", globalThis.crypto.randomUUID === globalThis.crypto.randomUUID);
console.log("node:crypto randomUUID stable:", nodeCrypto.randomUUID === nodeCrypto.randomUUID);
const seen = new Set();
for (let i = 0; i < 3; i++) seen.add(globalThis.crypto.randomUUID);
console.log("seen.size:", seen.size);

// The other Web Crypto members the issue asked to check.
console.log("getRandomValues stable:", globalThis.crypto.getRandomValues === globalThis.crypto.getRandomValues);
console.log("crypto namespace stable:", globalThis.crypto === globalThis.crypto);
console.log("subtle namespace stable:", globalThis.crypto.subtle === globalThis.crypto.subtle);
console.log("crypto.subtle === crypto.subtle (2nd read pair):", crypto.subtle === crypto.subtle);

// crypto.subtle's KEM methods went through the same broken per-read thunk as
// randomUUID/getRandomValues.
console.log("subtle.encapsulateBits stable:", crypto.subtle.encapsulateBits === crypto.subtle.encapsulateBits);
console.log("subtle.decapsulateBits stable:", crypto.subtle.decapsulateBits === crypto.subtle.decapsulateBits);
console.log("subtle.encapsulateKey stable:", crypto.subtle.encapsulateKey === crypto.subtle.encapsulateKey);
console.log("subtle.decapsulateKey stable:", crypto.subtle.decapsulateKey === crypto.subtle.decapsulateKey);

// Controls: subtle's other methods were already cached via a different
// mechanism (bound_native_callable_export_value) — must stay stable too.
console.log("subtle.digest stable:", crypto.subtle.digest === crypto.subtle.digest);
console.log("subtle.encrypt stable:", crypto.subtle.encrypt === crypto.subtle.encrypt);
console.log("subtle.generateKey stable:", crypto.subtle.generateKey === crypto.subtle.generateKey);

// Cross-read identity: the SAME closure across DIFFERENT expressions that
// resolve to the same property, not just repeated reads of one expression.
const a = globalThis.crypto.randomUUID;
const b = crypto.randomUUID;
console.log("cross-read identity:", a === b);

// node:crypto (module-level) default vs named import identity — already
// correct before this fix; kept as a control so a future regression there
// shows up in the same test.
console.log("node:crypto default randomUUID === named randomUUID:", nodeCrypto.randomUUID === namedRandomUUID);
console.log("node:crypto randomBytes stable:", nodeCrypto.randomBytes === nodeCrypto.randomBytes);
console.log("node:crypto named randomBytes === default randomBytes:", namedRandomBytes === nodeCrypto.randomBytes);

// Functional sanity: the cached closure still WORKS (shape check only — the
// value itself is random, so no exact value is printed).
const uuidPattern = /^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/;
const u = globalThis.crypto.randomUUID();
console.log("randomUUID() shape ok:", uuidPattern.test(u));
console.log("randomUUID() distinct across calls:", globalThis.crypto.randomUUID() !== globalThis.crypto.randomUUID());
const bytes = globalThis.crypto.getRandomValues(new Uint8Array(8));
console.log("getRandomValues() length ok:", bytes.length === 8);
