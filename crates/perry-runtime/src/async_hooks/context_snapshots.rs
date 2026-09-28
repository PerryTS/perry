//! `AsyncLocalStorage.snapshot()` and the context snapshots it and the
//! static bind helpers run callbacks under. Split out of `async_hooks.rs`
//! (file-size gate); items move unchanged.

use super::*;

pub(super) fn register_context_snapshot(
    snapshot: crate::async_context::AsyncContextSnapshot,
) -> usize {
    let id = NEXT_CONTEXT_SNAPSHOT_ID.fetch_add(1, Ordering::Relaxed);
    CONTEXT_SNAPSHOTS.lock().unwrap().insert(id, snapshot);
    id
}

pub(super) fn run_with_context_snapshot(snapshot_id: usize, f: impl FnOnce() -> f64) -> f64 {
    let snapshot = CONTEXT_SNAPSHOTS
        .lock()
        .unwrap()
        .get(&snapshot_id)
        .cloned()
        .unwrap_or_default();
    let scope = crate::gc::RuntimeHandleScope::new();
    let mut snapshot = snapshot;
    let snapshot_roots = crate::async_context::root_snapshot(&scope, &snapshot);
    let previous = crate::async_context::enter_context(&snapshot);
    // Guard-held (GC-scanned, throw-safe) — see runInAsyncScope (#788).
    crate::async_context::push_context_guard(
        crate::async_context::ContextGuardAction::RestoreSnapshot(previous),
    );
    let result = f();
    let result_handle = scope.root_nanbox_f64(result);
    crate::async_context::refresh_snapshot_from_roots(&mut snapshot, &snapshot_roots);
    if let Some(action) = crate::async_context::pop_context_guard() {
        crate::async_context::apply_context_guard(action);
    }
    result_handle.get_nanbox_f64()
}

pub(super) fn call_callback_with_rest(callback_value: f64, this_arg: f64, rest: f64) -> f64 {
    if !is_callable_value(callback_value) {
        throw_apply_not_function(callback_value);
    }
    let scope = crate::gc::RuntimeHandleScope::new();
    let callback_handle = scope.root_nanbox_f64(callback_value);
    let this_arg_handle = scope.root_nanbox_f64(this_arg);
    let rebound_bits = crate::closure::clone_closure_rebind_this(
        callback_handle.get_nanbox_f64().to_bits(),
        this_arg_handle.get_nanbox_f64(),
    );
    let rebound_handle = scope.root_nanbox_f64(f64::from_bits(rebound_bits));
    let callback = crate::fs::extract_closure_ptr(rebound_handle.get_nanbox_f64());
    if callback.is_null() {
        throw_apply_not_function(callback_handle.get_nanbox_f64());
    }
    let args_array = ptr_from_nanboxed(rest) as *const ArrayHeader;
    let args_array_handle = scope.root_raw_const_ptr(args_array);
    let result = if args_array.is_null() {
        unsafe {
            crate::closure::js_closure_call_array(
                callback as i64,
                crate::closure::JsThis::from_f64(this_arg_handle.get_nanbox_f64()),
                ptr::null(),
                0,
            )
        }
    } else {
        let arr = args_array_handle.get_raw_const_ptr::<ArrayHeader>();
        let len = js_array_length(arr) as i64;
        let data = if arr.is_null() || len == 0 {
            ptr::null()
        } else {
            unsafe { crate::array::array_elements_ptr(arr as *const ArrayHeader) as *const f64 }
        };
        unsafe {
            crate::closure::js_closure_call_array(
                callback as i64,
                crate::closure::JsThis::from_f64(this_arg_handle.get_nanbox_f64()),
                data,
                len,
            )
        }
    };
    result
}

pub(super) extern "C" fn async_local_storage_snapshot_trampoline(
    closure: *const ClosureHeader,
    _this: crate::closure::JsThis,
    callback_value: f64,
    rest: f64,
) -> f64 {
    let snapshot_id = js_closure_get_capture_ptr(closure, 0) as usize;
    run_with_context_snapshot(snapshot_id, || {
        // AsyncLocalStorage.snapshot() intentionally invokes the supplied
        // callback as a plain function. The receiver used to call the snapshot
        // wrapper itself is not forwarded.
        call_callback_with_rest(callback_value, TAG_UNDEFINED_F64, rest)
    })
}

pub(super) fn async_local_storage_static_snapshot_value() -> f64 {
    let snapshot_id = register_context_snapshot(crate::async_context::capture_context());
    let closure = js_closure_alloc(
        crate::fn_info!(async_local_storage_snapshot_trampoline, 2; with_rest(1)),
        1,
    );
    if closure.is_null() {
        return TAG_UNDEFINED_F64;
    }
    js_closure_set_capture_ptr(closure, 0, snapshot_id as i64);
    crate::object::set_builtin_closure_length(closure as usize, 1);
    crate::object::set_bound_native_closure_name(closure, "bound");
    crate::value::js_nanbox_pointer(closure as i64)
}

pub extern "C" fn js_async_local_storage_static_snapshot_method(
    _closure: *const ClosureHeader,
    _this: crate::closure::JsThis,
    _rest: f64,
) -> f64 {
    async_local_storage_static_snapshot_value()
}

#[no_mangle]
pub extern "C" fn js_async_local_storage_static_snapshot_direct(_rest: i64) -> f64 {
    async_local_storage_static_snapshot_value()
}
