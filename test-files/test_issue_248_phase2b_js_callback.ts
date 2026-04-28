// Issue #248 Phase 2B: Perry closures passed to JS-imported functions
// (JsCreateCallback). HIR's transform_js_imports rewrites any closure
// arg into a JS-runtime-loaded module — `gp.use(cb)`, `arr.forEach(cb)`
// etc. — into `Expr::JsCreateCallback`. Phase 2A bailed at codegen for
// this variant. This test verifies Phase 2B wiring: codegen lowers
// JsCreateCallback by passing `js_closure_call_array`'s address as the
// V8 trampoline `func_ptr`, the unboxed closure pointer as
// `closure_env`, and the static `param_count` — matching the contract
// `native_callback_trampoline` in perry-jsruntime expects. The runtime
// helper `js_closure_call_array(closure_env, args_ptr, args_len)` then
// dispatches to the right Perry `js_closure_callN` per args_len.
//
// Uses methods (not free functions) so `js_call_method` (which has no
// perry-runtime weak stub) forces perry-jsruntime to win the link
// resolution and V8 actually runs end-to-end. With free functions only,
// perry-runtime's weak stubs at `crates/perry-runtime/src/closure.rs:651`
// would shadow the real V8 FFIs (Mach-O first-wins) and the test would
// silently exit with all-zero counters.

import { makeDispatcher } from "./fixtures/issue_248_phase2b_jsmod.js";

const d = makeDispatcher();

// 0-arg callback with mutable capture — verifies js_closure_call0 dispatch
// AND that captured-variable mutations from inside the V8 callback
// propagate back to Perry's captured slot.
let counter0 = 0;
d.call0(() => {
  counter0++;
});
console.log("call0 counter:", counter0);

// 1-arg callback — js_closure_call1 path; captured `received1` is
// updated to the f64 V8 passed in.
let received1 = 0;
d.call1((x: number) => {
  received1 = x;
});
console.log("call1 received:", received1);

// 2-arg callback — js_closure_call2 path
let sum2 = 0;
d.call2((a: number, b: number) => {
  sum2 = a + b;
});
console.log("call2 sum:", sum2);

// 3-arg callback — js_closure_call3 path
let sum3 = 0;
d.call3((a: number, b: number, c: number) => {
  sum3 = a + b + c;
});
console.log("call3 sum:", sum3);

// Same callback fired multiple times — verifies the V8 callback handle
// stays valid across repeated invocations and capture state accumulates.
let twice = 0;
d.callTwice(() => {
  twice++;
});
console.log("callTwice count:", twice);

console.log("done");
