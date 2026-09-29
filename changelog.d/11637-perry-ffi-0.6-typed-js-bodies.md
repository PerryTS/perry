**perry-ffi 0.6 (breaking): one JS body ABI, typed.** Every native body a
function object runs is `body(callee, this, a0, ...)`; its Rust type is
defined once, in perry-abi (`js_body_fn_ty!`, `JsBody0`..`JsBody16`,
`JsThis`), and shared by the runtime and perry-ffi.

- `perry_ffi::alloc_closure(info, captures)` takes the body's
  `&'static JsFunctionInfo`, built by `perry_ffi::js_function_info!(body, n)`
  from the body's typed pointer (`JsBody0`..`JsBody16`) instead of a
  `*const u8`. A body with the wrong signature — no receiver, a wrong
  parameter type, a bare pointer — is a compile error on every platform.
  `register_closure_arity` is removed: the parameter count comes from the
  body's type, and other facts are builder calls on the info
  (`js_function_info!(body, 1; with_rest(0))`). `my_body` is
  `extern "C" fn(*const RawClosureHeader, JsThis, f64) -> f64`.
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
  garbage as `this`), a closure allocator declared outside the runtime,
  stdlib and perry-ffi, and a perry-ffi `alloc_closure` taking a
  `*const u8`. It found stale entry declarations in perry-stdlib, the UI
  crates and the runtime's own geisterhand registry.
- Every in-tree perry-ffi user (the `perry-ext-*` crates, the UI crates,
  perry-audio-miniaudio) is updated.
