A body that reads its dynamic `this` now reads its receiver PARAMETER
(stage 2 of passing `this` as a parameter). The thread-local implicit-`this`
cell is still written by every caller in this commit, so stage 2 can be
measured on its own; stage 3 deletes it.

- A function expression or object-literal method that reads `this` stores
  `%js_this` into its rooted entry `this` slot before the body's first
  safepoint. A sloppy body applies OrdinaryCallBindThis once, in place: an
  object receiver is tested inline, anything else goes through
  `js_this_coerce_sloppy` (nullish -> globalThis, primitive -> wrapper).
- A top-level function that reads `this` is compiled as
  `perry_fn_X$this(i64 %js_this, ...)`. The public `perry_fn_X` symbol that
  direct calls and other modules name is a forwarder passing `undefined`; the
  function's value wrapper passes the receiver it was given. Such functions
  are not arena-threaded.
- `__perry_wrap_<method>` forwards its `this` parameter as the method's
  receiver, and the runtime's native bodies read their `this` parameter
  instead of the cell.

Instructions per call (LTO-off build, same host) against stage 1: an
object-literal method reading `this` 227 -> 143, an ES5 prototype method
217 -> 134, `forEach` with a `thisArg` 828 -> 744.
