// Issue #678 followup: V8-fallback module imported from a native TS entry.
// The mere fact that this file has a `.js` extension and is NOT in
// `perry.compilePackages` forces it onto the V8 path. The codegen for the
// importing TS entry exercises the `js_call_v8_export` bridge for any
// callsite `transform_js_imports` somehow misses.

export function greet(name) {
  return "hello " + name;
}

export function add(a, b) {
  return a + b;
}
