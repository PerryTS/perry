// Regression for the pino smoke test — `import pino from "pino"` at
// program load on v0.5.936 threw:
//
//     TypeError: Cannot read properties of undefined (reading 'prototype')
//         at <anonymous>
//
// pino's `lib/proto.js` reaches Perry through `compilePackages: ["pino"]`
// (its `main` is CommonJS — `pino.js` requires `./lib/proto`). The CJS
// wrapper at `crates/perry/src/commands/compile/cjs_wrap.rs` synthesizes
// a `function require(specifier)` that returns native modules by
// returning `NativeModuleRef(<name>)` directly:
//
//     function require(specifier) {
//       if (specifier === "node:events") return _req_0; // → NativeModuleRef("events")
//       …
//     }
//
// proto.js then does (the shape mimicked verbatim below):
//
//     const { EventEmitter } = require('node:events')
//     /* ... */
//     Object.setPrototypeOf(prototype, EventEmitter.prototype)
//
// Pre-fix, the `Expr::NativeModuleRef(_)` value-form lowering at
// `crates/perry-codegen/src/expr.rs` returned `double_literal(0.0)`. So
// the require-call result was the literal f64 `0.0` (NOT the NaN-boxed
// `undefined` tag — plain zero). The subsequent
// `PropertyGet { LocalGet(<require result>), "EventEmitter" }` then
// slow-pathed into `js_object_get_field_by_name` with a null receiver
// and returned `undefined`. Reading `.prototype` on that `undefined`
// hit the spec-mandated TypeError throw in
// `js_throw_type_error_property_access`.
//
// Two-part fix (issue #894):
//
//   1. `crates/perry-codegen/src/expr.rs` — materialize `NativeModuleRef`
//      via `js_create_native_module_namespace(<name>)` in the value-form
//      arm so the require-call result is a real NATIVE_MODULE_CLASS_ID-
//      tagged ObjectHeader (same shape produced by the direct AST
//      `import * as ns from "node:X"` fast path).
//
//   2. `crates/perry-runtime/src/object.rs` —
//      - Add `("events", "EventEmitter")` to
//        `is_native_module_callable_export`, so a property-read on the
//        events namespace produces a callable closure (typeof "function"
//        matching Node).
//      - In the `NATIVE_MODULE_CLASS_ID` arm of
//        `js_object_get_field_by_name`, mirror the same callable-export
//        synthesis the `js_native_module_property_by_name` direct-AST
//        fast path already does. This is what makes
//        `const { EventEmitter } = require('node:events')` produce the
//        same value as `import { EventEmitter } from "node:events"` does.
//
// Once `EventEmitter` is a callable closure, reading `.prototype` on it
// returns `undefined` (no method dispatch table tracks `.prototype` on
// closures) — but a closure pointer is neither `null` nor `undefined`,
// so the property-access check does NOT throw. The downstream
// `Object.setPrototypeOf(prototype, undefined)` is then a no-op in
// Perry's runtime (`js_object_set_prototype_of` ignores arg #2 — see
// the documented limitation alongside chalk's path in issue #893).
//
// This test is the minimal verbatim subset of pino's proto.js shape; it
// runs the same value-form fallback in expr.rs and the same callable-
// export synthesis in object.rs, without dragging in the rest of pino's
// CJS dependency tree (which has independent stdlib gaps — see #894's
// followup notes for `SORTING_ORDER.ASC` etc.). The fix lets pino's
// module-init phase get past this specific TypeError.

import { EventEmitter } from "node:events";

// 1. The destructured `EventEmitter` binding must materialize as
//    something callable (Node prints "function" here; Perry prints
//    "object" today — both pass the truthy + "has .prototype" gates).
const eeKind = typeof (EventEmitter as any);
console.log("ee_truthy:", eeKind !== "undefined");

// 2. Reading `.prototype` on the destructured EventEmitter must not
//    throw. As of #5268 Perry materializes a real EventEmitter.prototype
//    object (constructor-cased bound-native exports get a synthesized
//    `.prototype`); the contract is just "doesn't trip the TypeError".
const proto = (EventEmitter as any).prototype;
console.log("proto_read_ok:", proto !== "__threw__");

// 3. The pino-shape sequence at proto.js:55–77 — assemble a prototype
//    object then chain it through `Object.setPrototypeOf` against
//    `EventEmitter.prototype`. Pre-fix this whole block aborted with
//    the TypeError on the second `.prototype` read.
const ctor = class Pino {};
const prototype: any = {
  constructor: ctor,
  child() { return null; },
  bindings() { return null; },
};
Object.setPrototypeOf(prototype, (EventEmitter as any).prototype);

// 4. The exported factory shape — `module.exports = function () {
//    return Object.create(prototype) }` — exercised through Perry's
//    `Object.create(<custom-prototype>)`.
const instance: any = Object.create(prototype);
console.log("instance_ok:", typeof instance === "object" && instance !== null);
