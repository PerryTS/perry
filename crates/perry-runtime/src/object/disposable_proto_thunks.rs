//! `DisposableStack` / `AsyncDisposableStack` prototype-method thunks (#4795).
//!
//! The direct instance calls (`stack.use(r)`, `stack.dispose()`, …) are
//! dispatched through the native method table to the `js_disposable_stack_*`
//! runtime helpers and never touch these thunks. The thunks exist for the
//! *reflective* path — `DisposableStack.prototype.use`, method extraction
//! (`const use = stack.use`), `.call`/`.apply`, and Test262's `verifyProperty`
//! descriptor checks. Each reads the `IMPLICIT_THIS` receiver, brand-checks it
//! against the stack class id, throws a `TypeError` on an incompatible
//! receiver, and otherwise dispatches to the shared runtime helper.
//!
//! Installed onto each stack's `.prototype` by
//! `global_this::populate_builtin_prototype_methods`.

use super::*;
use crate::disposable::{
    js_async_disposable_stack_dispose_async, js_async_disposable_stack_use,
    js_disposable_stack_adopt, js_disposable_stack_defer, js_disposable_stack_dispose,
    js_disposable_stack_disposed, js_disposable_stack_move, js_disposable_stack_use,
    CLASS_ID_ASYNC_DISPOSABLE_STACK, CLASS_ID_DISPOSABLE_STACK,
};

fn throw_incompatible(proto: &str, method: &str) -> ! {
    let msg = format!("Method {proto}.{method} called on incompatible receiver");
    let s = crate::string::js_string_from_bytes(msg.as_ptr(), msg.len() as u32);
    let err = crate::error::js_typeerror_new(s);
    crate::exception::js_throw(f64::from_bits(
        crate::value::JSValue::pointer(err as *const u8).bits(),
    ))
}

/// Resolve `IMPLICIT_THIS` to a stack `ObjectHeader` of the expected class id,
/// or throw a `TypeError`.
fn stack_receiver_or_throw(want_async: bool, method: &str) -> *mut ObjectHeader {
    let bits = IMPLICIT_THIS.with(|c| c.get());
    let value = f64::from_bits(bits);
    let proto = if want_async {
        "AsyncDisposableStack.prototype"
    } else {
        "DisposableStack.prototype"
    };
    let want_id = if want_async {
        CLASS_ID_ASYNC_DISPOSABLE_STACK
    } else {
        CLASS_ID_DISPOSABLE_STACK
    };
    let ptr = unsafe { super::native_call_method::object_ptr_from_value(value) };
    match ptr {
        Some(p) if !p.is_null() && unsafe { (*p).class_id } == want_id => p,
        _ => throw_incompatible(proto, method),
    }
}

// --- DisposableStack.prototype -------------------------------------------

pub(super) extern "C" fn ds_proto_use_thunk(
    _c: *const crate::closure::ClosureHeader,
    resource: f64,
) -> f64 {
    let stack = stack_receiver_or_throw(false, "use");
    js_disposable_stack_use(stack, resource)
}

pub(super) extern "C" fn ds_proto_adopt_thunk(
    _c: *const crate::closure::ClosureHeader,
    value: f64,
    on_dispose: f64,
) -> f64 {
    let stack = stack_receiver_or_throw(false, "adopt");
    js_disposable_stack_adopt(stack, value, on_dispose)
}

pub(super) extern "C" fn ds_proto_defer_thunk(
    _c: *const crate::closure::ClosureHeader,
    on_dispose: f64,
) -> f64 {
    let stack = stack_receiver_or_throw(false, "defer");
    js_disposable_stack_defer(stack, on_dispose)
}

pub(super) extern "C" fn ds_proto_dispose_thunk(_c: *const crate::closure::ClosureHeader) -> f64 {
    let stack = stack_receiver_or_throw(false, "dispose");
    js_disposable_stack_dispose(stack)
}

pub(super) extern "C" fn ds_proto_move_thunk(_c: *const crate::closure::ClosureHeader) -> f64 {
    let stack = stack_receiver_or_throw(false, "move");
    js_disposable_stack_move(stack)
}

pub(super) extern "C" fn ds_proto_disposed_getter_thunk(
    _c: *const crate::closure::ClosureHeader,
) -> f64 {
    let stack = stack_receiver_or_throw(false, "disposed");
    js_disposable_stack_disposed(stack)
}

// --- AsyncDisposableStack.prototype --------------------------------------

pub(super) extern "C" fn ads_proto_use_thunk(
    _c: *const crate::closure::ClosureHeader,
    resource: f64,
) -> f64 {
    let stack = stack_receiver_or_throw(true, "use");
    js_async_disposable_stack_use(stack, resource)
}

pub(super) extern "C" fn ads_proto_adopt_thunk(
    _c: *const crate::closure::ClosureHeader,
    value: f64,
    on_dispose: f64,
) -> f64 {
    let stack = stack_receiver_or_throw(true, "adopt");
    js_disposable_stack_adopt(stack, value, on_dispose)
}

pub(super) extern "C" fn ads_proto_defer_thunk(
    _c: *const crate::closure::ClosureHeader,
    on_dispose: f64,
) -> f64 {
    let stack = stack_receiver_or_throw(true, "defer");
    js_disposable_stack_defer(stack, on_dispose)
}

pub(super) extern "C" fn ads_proto_dispose_async_thunk(
    _c: *const crate::closure::ClosureHeader,
) -> f64 {
    let stack = stack_receiver_or_throw(true, "disposeAsync");
    js_async_disposable_stack_dispose_async(stack)
}

pub(super) extern "C" fn ads_proto_move_thunk(_c: *const crate::closure::ClosureHeader) -> f64 {
    let stack = stack_receiver_or_throw(true, "move");
    js_disposable_stack_move(stack)
}

pub(super) extern "C" fn ads_proto_disposed_getter_thunk(
    _c: *const crate::closure::ClosureHeader,
) -> f64 {
    let stack = stack_receiver_or_throw(true, "disposed");
    js_disposable_stack_disposed(stack)
}

/// Install a `disposed` accessor (getter only) onto a stack prototype.
fn install_disposed_getter(proto_obj: *mut ObjectHeader, func_ptr: *const u8) {
    if proto_obj.is_null() {
        return;
    }
    unsafe {
        crate::closure::js_register_closure_arity(func_ptr, 0);
        let closure = crate::closure::js_closure_alloc(func_ptr, 0);
        if closure.is_null() {
            return;
        }
        super::native_module::set_bound_native_closure_name(closure, "get disposed");
        super::native_module::set_builtin_closure_length(closure as usize, 0);
        let getter_bits = crate::value::js_nanbox_pointer(closure as i64).to_bits();
        // #6809: reads have a dedicated native-instance route, while direct
        // prototype reads use the per-owner descriptor flag. Keep startup
        // gate-neutral so this builtin accessor does not poison every dynamic
        // object write in the process.
        super::object_ops::install_builtin_getter(proto_obj, "disposed", getter_bits);
    }
}

/// Install a well-known-symbol method alias (`[Symbol.dispose]` /
/// `[Symbol.asyncDispose]`) pointing at the already-installed `method_value`.
fn install_symbol_dispose_alias(proto_obj: *mut ObjectHeader, short_sym: &str, method_value: f64) {
    if proto_obj.is_null() || method_value.to_bits() == crate::value::TAG_UNDEFINED {
        return;
    }
    let sym = crate::symbol::well_known_symbol(short_sym);
    if sym.is_null() {
        return;
    }
    unsafe {
        crate::symbol::js_object_set_symbol_property(
            crate::value::js_nanbox_pointer(proto_obj as i64),
            f64::from_bits(crate::value::JSValue::pointer(sym as *const u8).bits()),
            method_value,
        );
    }
    crate::symbol::set_symbol_property_attrs(
        proto_obj as usize,
        sym as usize,
        super::PropertyAttrs::new(true, false, true),
    );
}

/// Install the reflectable `.prototype` methods for the disposable stacks.
/// Returns true if `builtin_name` was a stack and was handled.
pub(super) fn install_disposable_proto_methods(
    builtin_name: &str,
    proto_obj: *mut ObjectHeader,
) -> bool {
    use super::global_this::install_proto_method as ipm;
    match builtin_name {
        "DisposableStack" => {
            // #4099: install the `disposed` accessor BEFORE any data method —
            // adding an accessor descriptor onto a prototype that already holds
            // data properties desyncs the accessor/data-field bookkeeping and
            // corrupts a data slot (see the Map arm in `collection_proto_thunks`).
            install_disposed_getter(proto_obj, ds_proto_disposed_getter_thunk as *const u8);
            ipm(proto_obj, "use", ds_proto_use_thunk as *const u8, 1);
            ipm(proto_obj, "adopt", ds_proto_adopt_thunk as *const u8, 2);
            ipm(proto_obj, "defer", ds_proto_defer_thunk as *const u8, 1);
            let dispose_value = ipm(proto_obj, "dispose", ds_proto_dispose_thunk as *const u8, 0);
            ipm(proto_obj, "move", ds_proto_move_thunk as *const u8, 0);
            install_symbol_dispose_alias(proto_obj, "dispose", dispose_value);
            install_toplevel_string_tag(proto_obj, "DisposableStack");
        }
        "AsyncDisposableStack" => {
            install_disposed_getter(proto_obj, ads_proto_disposed_getter_thunk as *const u8);
            ipm(proto_obj, "use", ads_proto_use_thunk as *const u8, 1);
            ipm(proto_obj, "adopt", ads_proto_adopt_thunk as *const u8, 2);
            ipm(proto_obj, "defer", ads_proto_defer_thunk as *const u8, 1);
            let dispose_value = ipm(
                proto_obj,
                "disposeAsync",
                ads_proto_dispose_async_thunk as *const u8,
                0,
            );
            ipm(proto_obj, "move", ads_proto_move_thunk as *const u8, 0);
            install_symbol_dispose_alias(proto_obj, "asyncDispose", dispose_value);
            install_toplevel_string_tag(proto_obj, "AsyncDisposableStack");
        }
        _ => return false,
    }
    true
}

/// `<Stack>.prototype[Symbol.toStringTag]` is the string class name, with the
/// `{ writable:false, enumerable:false, configurable:true }` descriptor.
fn install_toplevel_string_tag(proto_obj: *mut ObjectHeader, tag: &str) {
    if proto_obj.is_null() {
        return;
    }
    let sym = crate::symbol::well_known_symbol("toStringTag");
    if sym.is_null() {
        return;
    }
    let value = string_value(tag);
    unsafe {
        crate::symbol::js_object_set_symbol_property(
            crate::value::js_nanbox_pointer(proto_obj as i64),
            f64::from_bits(crate::value::JSValue::pointer(sym as *const u8).bits()),
            value,
        );
    }
    crate::symbol::set_symbol_property_attrs(
        proto_obj as usize,
        sym as usize,
        super::PropertyAttrs::new(false, false, true),
    );
}

fn string_value(s: &str) -> f64 {
    let ptr = crate::string::js_string_from_bytes(s.as_ptr(), s.len() as u32);
    crate::value::js_nanbox_string(ptr as i64)
}
