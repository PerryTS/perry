//! `AsyncLocalStorage.snapshot()` and the context snapshots it and the
//! static bind helpers run callbacks under. Split out of `async_hooks.rs`
//! (file-size gate); items move unchanged.

use super::*;

/// The returned closure owns the snapshot through capture 0. A detached
/// closure releases its native snapshot tokens with ordinary GC reachability.
pub(crate) fn snapshot_for_closure(
    closure: *const ClosureHeader,
) -> crate::async_context::AsyncContextSnapshot {
    let bits = crate::closure::js_closure_get_capture_bits(closure, 0);
    if bits == crate::value::TAG_UNDEFINED {
        return Default::default();
    }
    let holder =
        (bits & crate::value::POINTER_MASK) as *const crate::async_context::AsyncSnapshotHolder;
    unsafe { (*holder).snapshot.clone() }
}

pub fn run_with_context_snapshot(closure: *const ClosureHeader, f: impl FnOnce() -> f64) -> f64 {
    let snapshot = snapshot_for_closure(closure);
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
    run_with_context_snapshot(closure, || {
        // AsyncLocalStorage.snapshot() intentionally invokes the supplied
        // callback as a plain function. The receiver used to call the snapshot
        // wrapper itself is not forwarded.
        call_callback_with_rest(callback_value, TAG_UNDEFINED_F64, rest)
    })
}

pub(crate) fn snapshot_closure(
    snapshot: crate::async_context::AsyncContextSnapshot,
) -> *mut ClosureHeader {
    // No user code or poll occurs while the native snapshot is published.
    let _no_move = crate::gc::GcSuppressScope::new();
    let holder = if snapshot.is_empty() {
        crate::value::TAG_UNDEFINED
    } else {
        crate::value::POINTER_TAG | crate::async_context::alloc_snapshot_holder(snapshot) as u64
    };
    let closure = js_closure_alloc(
        crate::fn_info!(async_local_storage_snapshot_trampoline, 2; with_rest(1)),
        1,
    );
    if !closure.is_null() {
        crate::closure::js_closure_set_capture_bits(closure, 0, holder);
        crate::object::set_builtin_closure_length(closure as usize, 1);
        crate::object::set_bound_native_closure_name(closure, "bound");
    }
    closure
}

pub fn async_local_storage_static_snapshot_value() -> f64 {
    let closure = snapshot_closure(crate::async_context::capture_context());
    if closure.is_null() {
        TAG_UNDEFINED_F64
    } else {
        crate::value::js_nanbox_pointer(closure as i64)
    }
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

// Realm facts owned here; source-only until image emission.
crate::realm_decl!("context_snapshots.realm.json#objects");
