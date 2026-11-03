Stopped emitting the runtime derived-`super` scope for constructors that cannot
use it. `js_derived_super_scope_push`/`pop` maintain a thread-local stack whose
only consumers are `js_derived_super_bind_current` and
`js_derived_this_check_current` — the lookups an arrow uses when it compiles as
its own LLVM function and therefore cannot name the constructor's `i1` alloca.
A derived constructor containing no closure paid that thread-local round trip
per construction for a cell nothing could look up.

The gate is deliberately conservative: any closure in the body keeps the shared
form, without asking whether that closure mentions `this` or `super`.

Partial fix for #8648: 27% of the regression on a two-class `new B(x, y)` loop
(1.89x -> 1.67x). The remaining cost is the constructor field store moving from
`js_put_value_set` to `js_typed_feedback_class_field_set_guard` +
`js_class_field_set_fallback`, which is not addressed here.
