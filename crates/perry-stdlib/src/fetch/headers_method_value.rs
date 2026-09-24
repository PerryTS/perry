//! Cached bound-method values for Fetch `Headers` handles.
//!
//! WHATWG `Headers` exposes its prototype methods (`entries`, `get`, …) as
//! first-class function values, so `typeof h.entries === "function"` and
//! `h[Symbol.iterator] === h.entries` both hold. This helper allocates (and
//! caches) the bound-method closure that backs those reads. Split out of
//! `mod.rs` to keep that file under the 2,000-line lint gate. The child module
//! sees `mod.rs`'s private items via `use super::*`.
//!
//! The cache holds GC edges (#8163). Its values are NaN-boxed closure pointers
//! owned by the Headers record outside the GC heap, so it is invisible to every
//! heap-side instrument; `super::gc` registers the scanner that marks them and
//! rewrites the slots when the closure moves. Full marking follows these
//! edges only from live handles (`lifecycle`), allowing cache cycles to die.
//! Every read that misses here
//! allocates, so registration happens before the first insert.

use super::*;

extern "C" {
    #[link_name = "js_closure_alloc"]
    fn provider_js_closure_alloc(
        function: *const u8,
        capture_count: u32,
    ) -> *mut perry_runtime::closure::ClosureHeader;
    #[link_name = "js_closure_set_capture_f64"]
    fn provider_js_closure_set_capture_f64(
        closure: *mut perry_runtime::closure::ClosureHeader,
        index: u32,
        value: f64,
    );
    #[link_name = "js_closure_set_capture_ptr"]
    fn provider_js_closure_set_capture_ptr(
        closure: *mut perry_runtime::closure::ClosureHeader,
        index: u32,
        value: i64,
    );
    #[link_name = "js_nanbox_pointer"]
    fn provider_js_nanbox_pointer(pointer: i64) -> f64;
}

pub(crate) fn headers_bound_method_value(headers_id: usize, method_name: &'static str) -> f64 {
    let _fetch_roots = lifecycle::pin_handles(&[handle_to_f64(headers_id)]);
    if let Some(bits) = HEADERS_REGISTRY
        .lock()
        .unwrap()
        .get(&headers_id)
        .and_then(|record| record.method_values.get(method_name))
        .copied()
    {
        return f64::from_bits(bits);
    }

    extern "C" {
        fn js_write_barrier_root_nanbox(value_bits: u64);
    }

    // Register before the allocation below: the closure is unreferenced from
    // JS the moment the caller drops its bound copy, so the cache slot must be
    // a live root by the first collection that can run after the insert.
    super::gc::ensure_gc_registered();
    let closure =
        unsafe { provider_js_closure_alloc(perry_runtime::closure::BOUND_METHOD_FUNC_PTR, 3) };
    unsafe {
        provider_js_closure_set_capture_f64(closure, 0, handle_to_f64(headers_id));
        provider_js_closure_set_capture_ptr(closure, 1, method_name.as_ptr() as i64);
        provider_js_closure_set_capture_ptr(closure, 2, method_name.len() as i64);
    }
    let value = unsafe { provider_js_nanbox_pointer(closure as i64) };
    unsafe { js_write_barrier_root_nanbox(value.to_bits()) };
    if let Some(record) = HEADERS_REGISTRY.lock().unwrap().get_mut(&headers_id) {
        record.method_values.insert(method_name, value.to_bits());
    }
    value
}
