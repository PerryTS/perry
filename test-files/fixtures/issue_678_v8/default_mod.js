// Issue #678 followup: V8-fallback module with a *default function*
// export plus a named export. Pre-fix, the consumer-side codegen for
// `import defaultFn from "./default_mod.js"` formed an undefined
// `perry_fn_<src>__default` extern; the linker failed with
// `Undefined symbols: _perry_fn_..._default`. With the V8 bridge
// fix, the same call lowers to `js_call_v8_export(specifier,
// "default", args, argc)` and the link succeeds.
//
// Paired with the existing `mod.js` fixture (named-only exports) so
// `test_issue_678_v8_fallback_symbols.ts` can exercise every
// codegen site that previously emitted a `perry_fn_<v8src>__<name>`
// symbol — named, default, and namespace-member.

export default function defaultFn(x) {
  return "v8-default: " + x;
}

export function named(x) {
  return "v8-named: " + x;
}
