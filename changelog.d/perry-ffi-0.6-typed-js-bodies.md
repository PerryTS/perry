**perry-ffi 0.6 (breaking): one JS body ABI, typed.** Every native body a
function object runs is `body(callee, this, a0, ...)`; its Rust type is
defined once, in perry-abi (`js_body_fn_ty!`, `JsBody0`..`JsBody16`,
`JsThis`), and shared by the runtime and perry-ffi.

- `perry_ffi::alloc_closure`, `register_closure_arity` and the new
  `register_closure_rest` take a typed body (`F: JsBody<ClosureHeader>`)
  instead of a `*const u8`. A body with the wrong signature — no receiver, a
  wrong parameter type, a bare pointer — is a compile error on every
  platform. Migrate with a cast to the body's type:
  `alloc_closure(my_body as perry_ffi::JsBody1, captures)`, where `my_body`
  is `extern "C" fn(*const RawClosureHeader, JsThis, f64) -> f64`.
- Calls into JS take the receiver after the function, `JsThis::UNDEFINED`
  for a plain call: `JsClosure::call0..4(this, ...)`, the new
  `JsClosure::call_slice(this, &args)` and `perry_ffi::call_value(func,
  this, &args)`. The runtime's entries match: `js_closure_call{N}(closure,
  this, ...)`, `js_native_call_value(func, this, args, len)`,
  `js_closure_call_array(closure, this, args, len)`. No native code reads or
  writes an ambient `this`.
- perry-ffi now carries its own version (0.6.0) and depends on perry-abi,
  which is published before it (`scripts/publish_perry_ffi.sh`).
- `scripts/check_js_body_call_funnel.py` refuses an `extern` declaration of a
  call entry without the receiver (a stale declaration compiles and passes
  garbage as `this`), a closure registrar declared outside the runtime,
  stdlib and perry-ffi, and a perry-ffi registration function taking a
  `*const u8`. It found stale entry declarations in perry-stdlib, the UI
  crates and the runtime's own geisterhand registry.
- Every in-tree perry-ffi user (the `perry-ext-*` crates, the UI crates,
  perry-audio-miniaudio) is updated.
