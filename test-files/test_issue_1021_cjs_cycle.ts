// Issue #1021 follow-up: V8-fallback CommonJS wrap must survive a
// readable-stream-style require cycle. Two .js peers each `require()` the
// other — one at module top level (immediate use of `.prototype`), the
// other from inside a function body. Pre-fix, the wrap hoisted both
// requires into static ESM imports, which forced V8 to evaluate one peer
// with the other's bindings in TDZ; the top-level
// `Object.create(A.prototype)` then exploded with "must have a prototype"
// because `A` was a still-loading namespace object. The wrap now skips
// the static import for relative requires that would create a 1-hop cycle
// (detected by reading the peer's source) and falls back to a global
// `__perry_cjs_partial` registry at call time.
//
// Acceptance: byte-for-byte parity with `node --experimental-strip-types`.

// Consume a named export from the entry. The named binding forces Perry to
// realize the V8-fallback module's bindings (which evaluates the IIFE,
// printing the assertion lines via console.log).
// @ts-ignore - .cjs untyped
import { ok } from "./fixtures/issue_1021_cjs_cycle/entry.cjs";
console.log("ok=" + ok);
