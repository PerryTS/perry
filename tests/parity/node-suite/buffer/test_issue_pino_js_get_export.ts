// Regression for the pino smoke `[js_get_export]` namespace failure
// downstream of #903 (v0.5.945). After the same-file default-import
// collision was fixed, `import pino from "pino"` advanced past
// `SORTING_ORDER.ASC` into `thread-stream/index.js`, which evaluates
//
//     const MAX_STRING = buffer.constants.MAX_STRING_LENGTH
//
// at top-level module init. thread-stream is loaded through Perry's
// V8 fallback (perry-jsruntime), and its `require('buffer')` resolves
// to the `node:buffer` stub at `crates/perry-jsruntime/src/modules.rs`
// (the `get_builtin_stub("buffer")` arm).
//
// Pre-fix that stub exported `Buffer` only — no `constants` namespace.
// `buffer.constants` was therefore `undefined`, and reading
// `.MAX_STRING_LENGTH` on `undefined` threw `TypeError: Cannot read
// properties of undefined (reading 'MAX_STRING_LENGTH')` at top-level.
// V8 marked thread-stream's module as failed-to-evaluate, so the next
// `state.runtime.get_module_namespace(module_id)` call inside
// `js_get_export` (`crates/perry-jsruntime/src/interop.rs:1072`)
// bubbled the same TypeError back to native code as
//
//     [js_get_export] failed to get namespace: TypeError: Cannot read
//     properties of undefined (reading 'MAX_STRING_LENGTH')
//
// pino then sees `undefined` for the thread-stream namespace and its
// downstream evaluation aborts (the next blocker — `(boolean).
// tracingChannel` — is an unrelated `diagnostics_channel` gap, tracked
// separately).
//
// Fix: extend the V8-fallback `node:buffer` stub to export Node's
// real `buffer.constants` object as both a named export and a default-
// export member. Values mirror Node 20+:
//   - MAX_LENGTH        = Number.MAX_SAFE_INTEGER (= 2^53 - 1)
//   - MAX_STRING_LENGTH = 2^29 - 24 = 536870888
//
// The actual byte-level regression is guarded by the Rust unit test
// `test_buffer_stub_exposes_constants` at
// `crates/perry-jsruntime/src/modules.rs`, which asserts the stub
// source emits both the named `constants` export and its presence on
// the default export. The V8 fallback path isn't reachable from a
// single standalone .ts file (it only fires for V8-evaluated modules
// inside `compilePackages` targets like pino+thread-stream), so this
// file documents the shape and pins the public-API expectations the
// fix puts in place. The pino smoke at the top of this comment is
// what verifies the wiring end-to-end during release validation.
//
// Sanity-only checks below — they exercise the natively-compiled
// `node:buffer` import surface, not the V8 fallback. They guard
// against the `buffer` module ceasing to import at all and against
// the existing `Buffer` export becoming undefined.

import * as buffer from "node:buffer";
import { Buffer } from "node:buffer";

// 1. `node:buffer` resolves without throwing. The shape is whatever
//    Perry's native module returns; we just confirm the import didn't
//    explode at module-init.
console.log("buffer_module_defined:", typeof (buffer as any) !== "undefined");

// 2. The `Buffer` named export must still be reachable — pino's
//    upstream chain reads `Buffer.alloc`, `Buffer.from`, etc. on it.
//    Perry reports `typeof Buffer === "object"` today while Node says
//    "function"; we just want a non-undefined result here.
console.log("Buffer_defined:", typeof Buffer !== "undefined");

// 3. Round-trip a Buffer through `.from` / `.toString` so we know the
//    native side is fully wired (not just module-load wired).
const bb = Buffer.from("hi", "utf8");
console.log("Buffer.from('hi').toString:", bb.toString("utf8"));
