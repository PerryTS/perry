Replacing a built-in namespace method is now honored by direct member calls
(#10848). `console.log = fn`, `console[m] = fn`, `Math.max = fn`,
`JSON.stringify = fn`, `Object.defineProperty(Math, "max", …)` and
`globalThis.console = {…}` previously landed on the object — every read saw
the replacement — but `console.log(x)` / `Math.max(a, b)` stayed bound to the
intrinsic and silently ran the original (OpenTUI's console capture printed
straight to the terminal).

- HIR: a whole-program pre-scan (`perry_hir::patched_builtins`) records every
  write to a builtin namespace member; the driver folds each module's scan into
  a program-wide set before lowering it, and re-runs the collect walk when a
  patch surfaces after other modules were already lowered. A direct call to a
  patched member lowers as a dynamic property-get-then-call on the live
  namespace object (`lower/expr_call/patched_builtin_call.rs`), ahead of every
  intrinsic arm; the `.call/.apply/.bind` rewrite is gated too. Programs that
  patch nothing keep every fast path.
- Runtime: the dynamic dispatcher's native-namespace arm now invokes a user
  override with the namespace as `this`
  (`object/native_call_method/namespace_override.rs`). A reentrancy guard sends
  a nested dispatch of the same `(module, method)` native, so restoring or
  forwarding to the original bound builtin does not recurse; a new
  `namespace_override` catch savepoint unwedges the guard when an override
  throws.

Not covered: builtin prototype methods (`Array.prototype.join = fn`) — the
runtime's array method dispatch ignores a replaced prototype method even for a
dynamic `arr[key]()` call. Regression test:
`test-files/test_gap_10848_builtin_method_patch.ts`.
