// parity-env: PERRY_GC_MOVING_LOOP_POLLS=1
// #6559 interpreter global-`this` rooting across a copying minor.
//
// `dyn_function_from_strings` obtained the global from
// `js_get_global_this()` but did NOT root it before calling
// `env_new_root()`, which allocates a scope object. A copying minor
// triggered by that allocation evacuates the global singleton —
// `THREAD_GLOBAL_THIS` (a registered root) is rewritten, but the raw
// `global` local is not. The stale pointer flows into the closure's
// capture slots 3 (global) and 4 (intrinsics), so when the closure is
// called the interpreter's sloppy-mode `this = global` reads a stale
// value. On the `return this` path this returns undefined (the
// evacuated from-space object's fields read back as undefined after
// the nursery flip), and downstream `Object.getPrototypeOf(undefined)`
// throws `TypeError: Cannot convert undefined or null to object`.
//
// The other dyn_eval entry points
// (`function_from_strings_in_with_codegen`, `eval_script_in_with_codegen`)
// already root `global_this` before any allocation; this one missed it.
//
// STRUCTURE IS LOAD-BEARING. The test uses RECURSION rather than a loop
// to call `Function("return this")()` many times. A loop's back-edge is
// a compiled safepoint — with a seeded schedule, a safepoint GC fires
// early and promotes the global to the old generation BEFORE the
// nursery fills, making the unrooted window in `env_new_root()`
// untestable (the global no longer moves). Recursion has no back-edge
// safepoint, so the ONLY GC that can run is the allocation-point
// trigger (`gc_check_trigger`) when the nursery actually fills. That
// trigger fires inside `env_new_root()`'s `js_object_alloc_null_proto`
// call — the exact window where `global` is unrooted.
//
// SYMPTOM BEFORE THE FIX: `bad > 0` (the first nursery-full event that
// lands on `env_new_root()` returns undefined for `this`).
// AFTER THE FIX: `bad 0` — `global` is rooted before `env_new_root()`,
// so the copying minor rewrites the root slot and the capture gets the
// post-move address.
// Clean under `PERRY_GEN_GC=0` (no GC, no stale pointer).

let bad = 0;

function recurse(n: number): void {
  if (n <= 0) return;
  // Function("return this")() must return the global object, not
  // undefined.  The sloppy-mode Function constructor binds `this` to
  // the global when called with no receiver; the interpreter
  // implements this by reading `global` from the closure's capture
  // slot 3.  If that slot holds a stale from-space pointer (because
  // the global was evacuated between `js_get_global_this()` and
  // `env_new_root()` in `dyn_function_from_strings`), the `return
  // this` evaluates to undefined.
  const g = Function("return this")();
  if (g === undefined || g === null) {
    bad++;
  } else {
    // The global must be a real object — `Object.getPrototypeOf` must
    // not throw.  This is the exact call site that fails in the
    // node-machine-id webpack bundle.
    try {
      Object.getPrototypeOf(g);
    } catch {
      bad++;
    }
  }
  recurse(n - 1);
}

recurse(5000);
console.log("bad", bad);
