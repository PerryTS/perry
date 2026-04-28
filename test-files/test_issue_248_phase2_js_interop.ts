// Issue #248 Phase 2: codegen arms for the perry-jsruntime / V8 interop
// expression family — JsLoadModule, JsGetExport, JsCallFunction,
// JsCallMethod, JsGetProperty, JsSetProperty, JsNew, JsNewFromHandle.
// HIR's `transform_js_imports` pass (in perry-hir/src/js_transform.rs)
// rewrites every call/property-access into a `.js` module file (which the
// resolver classifies as JS-runtime-loaded — see
// perry/src/commands/compile/collect_modules.rs:73) into one of these
// variants. Pre-fix the LLVM backend bailed with
// `expression JsCallFunction not yet supported`.
//
// This test only verifies compile + link + clean-exit. End-to-end V8
// execution depends on perry-jsruntime stub-override link order which
// has a pre-existing tangle (perry-runtime/src/closure.rs ships weak
// stubs for js_load_module / js_call_function / js_get_export /
// js_set_property / js_runtime_init that may or may not get overridden
// by perry-jsruntime depending on link order — separate issue).
//
// `JsCreateCallback` is intentionally NOT exercised here — closure
// marshaling between Perry's `(closure_ptr, arg0, arg1, ...)` calling
// convention and V8's `(closure_env, args_ptr, args_len)` trampoline
// requires either codegen-emitted per-arity adapter thunks or a runtime
// closure-array dispatcher. Tracked as Phase 2B follow-up.

import { greet, capitalize } from "./fixtures/issue_248_jsmod.js";

// JsCallFunction with no args — exercises the JsLoadModule + JsCallFunction
// codegen path, including the args-array null fallback.
const a = greet();
console.log("a-typeof:", typeof a);

// JsCallFunction with one arg — exercises the args alloca + GEP path.
const b = capitalize("hello");
console.log("b-typeof:", typeof b);

console.log("done");
