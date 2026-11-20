One mechanism now forwards every call that has an explicit `this`:
`Function.prototype.call` and `apply` (as values and as methods), bound
functions and `Reflect.apply`. Before the callee runs, two steps can allocate:
boxing a primitive receiver for a sloppy callee, and cloning an object-literal
method that keeps `this` in a capture. The shared forwarder
(`closure::forward_with_explicit_this`) takes the fast path when neither step
can allocate. Otherwise it holds the callee, the receiver and every argument
in handles and re-reads them after each step. It replaces six separate copies
of that logic: the `call` and `apply` thunks, the `call` and `apply` method
arms, bound-function dispatch and `Reflect.apply`. Four of them (the `apply`
thunk, both method arms and bound-function dispatch) kept the arguments in
plain Rust memory across the boxing or the clone.

- **`bind` roots its arguments.** `js_function_bind` read `thisArg` and the
  partial arguments only after receiver boxing, the `Get(target, "name")`
  getter (user code) and the bound-arguments array allocation, all of which
  can collect, and the caller's buffer is not scanned. The arguments are now
  held in handles from the start, and the array is built from them
  (`build_rest_array_rooted`). This also covers the compiled `bind` intrinsic.
- **`call`, `apply` and `bind` as values run the method form's body**
  (`run_function_intrinsic`). `Function.prototype.call.call(C, o)`, uncurried
  `call` on a class constructor, the static bound-method receiver and the
  stream/http construction aliases (#10454, #4973) now behave the same
  whichever way `call` is reached.
- **`apply` builds its argument list before binding the receiver**, as the
  spec orders it. The callee and receiver are held across the generic
  array-like walk, whose getters and Proxy traps can collect. The common
  Array and `arguments` cases root nothing.
- **A Symbol receiver is boxed for a sloppy callee** (`f.call(Symbol())`), on
  the forwarder and on the compiled-body `call` fast path alike. Symbols are
  pointer-tagged, so they used to pass through unboxed.
- **Class-method value reads follow [[Get]].** An own accessor shadowing a
  method now runs its getter. An own `m = undefined` now reads `undefined`.
  Both used to fall back to the prototype method when the receiver was
  class-typed (`this.m` in a method, `x.m` on a `C`-typed parameter). The own
  lookup skips private-name entries (`own_property_get_by_bytes`), and it
  reads an own accessor pair from the slot the holder's shape resolved
  (#12015's holder-shape descriptor facts), with no name search.
  `own_data_field_by_name` uses the same namespace-aware lookup. An own
  property with a lone-surrogate name now shadows the method too.

Named collection points (`explicit_this.coerced`, `explicit_this.rebound`,
`function_bind.coerced`, `function_bind.named`) make the rooting testable.
Runtime tests force a copying minor at each one with young heap arguments and
check what the callee received. Dropping either `root_nanbox_f64_slice` turns
them red.

Not fixed (pre-existing): a class method declared with a lone-surrogate name
(`["\uD800"]() {}`) is registered under a lossy name, so calling it fails on
main as well.

Measured (n=5 interleaved, instructions:u, against main): qs stringify
−14.1%, Zod ×5000 −3.7%, qs parse −3.1%, tsc −0.5%. The rest are flat, and
buffer_heavy is +0.25%: `own_data_field_by_name` now checks the
private-entry attribute on a hit, and the stream hidden-state reads take that
path. On qs parse the method-form `call`/`apply` used to run receiver boxing
and the rebind on every call (1.32 M `coerce_call_this`, 1.55 M
`clone_closure_rebind_this`); the forwarder's fast test leaves 102 and 89.
