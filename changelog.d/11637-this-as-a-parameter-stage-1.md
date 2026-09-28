Every JS body now takes its receiver as a parameter: a compiled closure body,
a function's value wrapper, a class method's value wrapper and every native
builtin installed as a function object are `double body(i64 callee, i64 this,
double a0, ...)` (`perry_abi::JS_BODY_*`; the receiver is NaN-boxed bits in an
integer register, so every floating-point argument register stays free for
JS arguments). This is stage 1 of passing `this` as a parameter instead of
through the per-agent implicit-`this` cell, and it changes no behavior:
every caller passes exactly the receiver the cell holds for the call — the
method-call site its receiver, a call that bound the cell to `undefined`
`undefined`, the runtime's dispatch paths the cell's current value — and
bodies still read the cell.

- The runtime's native bodies declare the receiver (`_this: JsThis`,
  `closure::JsThis`, `repr(transparent)` over `u64`); perry-ffi exports an
  ABI-identical `JsThis` for wrapper crates. The body-call funnel
  (`closure/body_call.rs`) passes it on every route: exact arity, padded
  arity, rest bundling, the wide ladder, hoisted `DirectCallN` sites,
  microtask steps and worker threads.
- Stage 0 (the preceding commit) made one funnel per side for every call of
  a body's code pointer: `closure/body_call.rs` in the runtime,
  `expr::body_call` in codegen, and deleted the dead, wrong
  `js_closure_unbind_this`.
- Codegen spells the ABI once (`expr::body_call::js_body_params` /
  `emit_js_body_call`); a stage-0 miss is closed — the inline
  `Array.prototype.some` loop called its captureless callback body directly.
- `PERRY_THIS_WITNESS=1` (compile-time, in the object-cache key) makes the
  entry of every compiled body that reads the cell compare its `this`
  parameter with the cell and report `PERRY_THIS_WITNESS checks=N
  mismatches=M` at exit. The gap corpus, tsc and the call-route fixture run
  with zero mismatches.
- `scripts/check_js_body_call_funnel.py` now also refuses a body-shaped
  `extern "C" fn` that does not declare the receiver, and a function handed
  straight to a closure allocator that does not.

Cost (instructions per call, LTO-off fast build, same host): a closure value
call through the runtime +9 (the dispatcher reads the cell to pass it), a
method-site hit +3. Stage 3 removes the cell and these reads with it.
