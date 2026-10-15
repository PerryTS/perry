//! Fixed-name extension accesses: ordinary JavaScript `Get` and method calls
//! through the runtime's one read path. Nothing is kept per call site.

extern "C-unwind" {
    fn js_runtime_state_key_get(value: f64, key: *const u8, len: usize) -> f64;
}
extern "C-unwind" {
    fn js_runtime_state_key_call(
        value: f64,
        key: *const u8,
        len: usize,
        args: *const f64,
        argc: usize,
    ) -> f64;
}

/// `value[name]` with ordinary JavaScript semantics.
/// # Safety
/// Called on a JS agent; `value` is a live JavaScript value.
pub unsafe fn runtime_get(value: f64, name: &'static [u8]) -> f64 {
    js_runtime_state_key_get(value, name.as_ptr(), name.len())
}

/// Call `value[name]` with `value` as `this`.
/// # Safety
/// As [`runtime_get`]; every argument is a live JavaScript value.
pub unsafe fn runtime_call(value: f64, name: &'static [u8], args: &[f64]) -> f64 {
    js_runtime_state_key_call(value, name.as_ptr(), name.len(), args.as_ptr(), args.len())
}
