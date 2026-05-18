// Issue #678 followup: exhaustive coverage of the codegen sites that
// previously emitted bare `perry_fn_<v8src>__<name>` extern references
// for imports landing in `ModuleKind::Interpreted`. The V8 fallback
// path never emits those native symbols (the module is loaded through
// `js_load_module`), so the link failed with `Undefined symbols:
// _perry_fn_..._<name>`.
//
// The fix at crates/perry-codegen/src/{lower_call,expr,codegen}.rs
// threads `import_function_v8_specifiers` through every call site that
// builds a `perry_fn_<src>__<name>` symbol and short-circuits to
// `js_call_v8_export(specifier, name, args, argc)` for V8-routed
// imports. This regression locks in the link + runtime behavior for
// the four codegen shapes the bridge has to cover:
//
//   1. Named import + direct call             →  lower_call.rs:1197
//   2. Default import + direct call           →  lower_call.rs:1197 (name="default")
//   3. Namespace import + member call         →  lower_call.rs:560
//   4. Named import + function-as-value       →  expr.rs `js_closure_alloc_singleton`
//                                                wrapper (`__perry_wrap_perry_fn_<src>__<name>`)
//
// Acceptance: byte-for-byte parity with `node --experimental-strip-types`.

import defaultFn, { named } from "./fixtures/issue_678_v8/default_mod.js";
import { greet, add } from "./fixtures/issue_678_v8/mod.js";
import * as ns from "./fixtures/issue_678_v8/mod.js";

// 1. Named-from-V8 + direct call.
console.log("named:", greet("perry"));
console.log("named-arity2:", add(40, 2));

// 2. Default-from-V8 + direct call. Pre-fix this site formed
// `perry_fn_<default_mod_js>__default` and link-failed.
console.log("default:", defaultFn("hi"));
console.log("default-mod-named:", named("ho"));

// 3. Namespace member call. Pre-fix `ns.greet(...)` formed
// `perry_fn_<mod_js>__greet` through the member-access codegen path.
console.log("ns-named:", ns.greet("ns"));
console.log("ns-arity2:", ns.add(10, 20));

// 4. Function-as-value: hand the imported binding to another helper
// so the codegen takes the `js_closure_alloc_singleton(
// @__perry_wrap_perry_fn_<src>__<name>)` path. Pre-fix the wrapper
// stub symbol referenced here was also missing.
function apply1(fn: (s: string) => string, value: string): string {
  return fn(value);
}
function apply2(fn: (a: number, b: number) => number, a: number, b: number): number {
  return fn(a, b);
}
console.log("apply-named:", apply1(greet, "world"));
console.log("apply-default:", apply1(defaultFn, "world"));
console.log("apply-add:", apply2(add, 100, 23));

console.log("done");
