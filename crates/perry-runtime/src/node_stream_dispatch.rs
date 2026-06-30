//! node:stream — method-table builders, stub-arity registration, pointer/GC helpers (split out of node_stream.rs for the 2000-line
//! file-size gate, #1987). Shares the parent module's constants, hidden-key
//! accessors and state primitives via `use super::*`.
#![allow(unused_imports)]
use super::*;
use crate::closure::{
    js_closure_alloc, js_closure_get_capture_f64, js_closure_get_capture_ptr,
    js_closure_set_capture_f64, js_closure_set_capture_ptr, ClosureHeader,
};
use crate::object::{
    js_object_alloc, js_object_alloc_with_shape, js_object_get_field,
    js_object_get_field_by_name_f64, js_object_set_field, js_object_set_field_by_name,
    ObjectHeader,
};
use crate::value::JSValue;
use std::os::raw::c_int;

pub(super) type StubFn = unsafe extern "C" fn();

#[allow(clippy::missing_transmute_annotations)]
pub(super) fn cast0(f: extern "C" fn(*const ClosureHeader) -> f64) -> StubFn {
    unsafe { std::mem::transmute(f) }
}
#[allow(clippy::missing_transmute_annotations)]
pub(super) fn cast1(f: extern "C" fn(*const ClosureHeader, f64) -> f64) -> StubFn {
    unsafe { std::mem::transmute(f) }
}
#[allow(clippy::missing_transmute_annotations)]
pub(super) fn cast2(f: extern "C" fn(*const ClosureHeader, f64, f64) -> f64) -> StubFn {
    unsafe { std::mem::transmute(f) }
}
#[allow(clippy::missing_transmute_annotations)]
pub(super) fn cast3(f: extern "C" fn(*const ClosureHeader, f64, f64, f64) -> f64) -> StubFn {
    unsafe { std::mem::transmute(f) }
}

// ─────────────────────────────────────────────────────────────────
// Build the host object: allocate an ObjectHeader sized to the
// method set, then fill each slot with a closure that captures the
// host object's NaN-boxed value (so `this` chains return identity).
// ─────────────────────────────────────────────────────────────────

pub(super) fn build_object(methods: &[(&str, StubFn)], shape_id: u32) -> *mut ObjectHeader {
    register_stub_arities();

    // Pack the method names as a NUL-separated byte sequence, matching
    // the layout `js_object_alloc_with_shape` parses for shape keys.
    let mut packed: Vec<u8> = Vec::new();
    for (name, _) in methods {
        packed.extend_from_slice(name.as_bytes());
        packed.push(0);
    }
    let field_count = methods.len() as u32;
    let obj =
        js_object_alloc_with_shape(shape_id, field_count, packed.as_ptr(), packed.len() as u32);

    // NaN-box the object pointer — we'll capture it (as raw bits) in each
    // closure's slot 0 so the stub `this_value` helper can reconstruct
    // the f64 form for `return this` semantics.
    let this_bits = JSValue::pointer(obj as *const u8).bits();

    let mut on_method: Option<JSValue> = None;
    for (i, (name, func)) in methods.iter().enumerate() {
        if *name == "addListener" {
            if let Some(val) = on_method {
                js_object_set_field(obj, i as u32, val);
                continue;
            }
        }
        let closure = js_closure_alloc(*func as *const u8, 1);
        // Reuse `set_capture_ptr` (i64 payload). We only need 64 bits
        // and the NaN-boxed pattern fits cleanly when reinterpreted.
        crate::closure::js_closure_set_capture_ptr(closure, 0, this_bits as i64);
        let val = JSValue::pointer(closure as *const u8);
        if *name == "on" {
            on_method = Some(val);
        }
        js_object_set_field(obj, i as u32, val);
    }
    obj
}

pub(super) fn install_methods_on_existing_object(
    obj: *mut ObjectHeader,
    this_value: f64,
    methods: &[(&str, StubFn)],
    skip_names: &[&str],
) {
    register_stub_arities();
    let this_bits = this_value.to_bits();
    let mut on_method: Option<f64> = None;
    for (name, func) in methods {
        if skip_names.iter().any(|skip| skip == name) {
            continue;
        }
        if *name == "addListener" {
            if let Some(val) = on_method {
                js_object_set_field_by_name(obj, hidden_key(name.as_bytes()), val);
                continue;
            }
        }
        let closure = js_closure_alloc(*func as *const u8, 1);
        crate::closure::js_closure_set_capture_ptr(closure, 0, this_bits as i64);
        let val = f64::from_bits(JSValue::pointer(closure as *const u8).bits());
        if *name == "on" {
            on_method = Some(val);
        }
        js_object_set_field_by_name(obj, hidden_key(name.as_bytes()), val);
    }
}

/// Install the EventEmitter prototype methods (`on`/`once`/`emit`/
/// `removeListener`/`removeAllListeners`/…) on a *prototype* object as named
/// own properties, each a closure that reads its receiver from the call-site
/// `this` (IMPLICIT_THIS) rather than a captured instance.
///
/// readable-stream's `Readable.prototype.on` is `function (ev, fn) { var res =
/// Stream.prototype.on.call(this, ev, fn); … }` — the legacy `Stream.prototype`
/// must therefore expose `.on` (and siblings) as VALUES so the `.call(this)`
/// borrow dispatches the EventEmitter `on` against the real stream instance.
/// Before this, `Stream.prototype.on` was `undefined` and the `.call` threw
/// "Function.prototype.call was called on a value that is not a function".
///
/// The closures capture `TAG_UNDEFINED` in slot 0; `this_value` treats that as
/// "no fixed receiver — read IMPLICIT_THIS", which `Function.prototype.call`/
/// `.apply` sets to the borrowed `this`.
pub(crate) fn install_event_emitter_prototype_methods(proto: *mut ObjectHeader) {
    register_stub_arities();
    let methods = super::emitter_methods();
    let mut on_method: Option<f64> = None;
    for (name, func) in methods {
        if name == "addListener" {
            if let Some(val) = on_method {
                js_object_set_field_by_name(proto, hidden_key(name.as_bytes()), val);
                continue;
            }
        }
        let closure = js_closure_alloc(func as *const u8, 1);
        crate::closure::js_closure_set_capture_ptr(closure, 0, crate::value::TAG_UNDEFINED as i64);
        let val = f64::from_bits(JSValue::pointer(closure as *const u8).bits());
        if name == "on" {
            on_method = Some(val);
        }
        js_object_set_field_by_name(proto, hidden_key(name.as_bytes()), val);
    }
}

pub(super) fn register_stub_arities() {
    let register = |func: *const u8, arity: u32| {
        crate::closure::js_register_closure_arity(func, arity);
    };
    register(ns_chain0 as *const u8, 0);
    register(ns_chain1 as *const u8, 1);
    register(ns_wrap1 as *const u8, 1);
    register(ns_wrap_data as *const u8, 1);
    register(ns_wrap_end as *const u8, 0);
    register(ns_wrap_error as *const u8, 1);
    register(ns_wrap_close as *const u8, 0);
    register(ns_destroy_error_microtask as *const u8, 0);
    register(ns_stream_abort_listener as *const u8, 0);
    register(ns_destroy1 as *const u8, 1);
    register(ns_chain2 as *const u8, 2);
    register(ns_chain3 as *const u8, 3);
    register(ns_on2 as *const u8, 2);
    register(ns_once2 as *const u8, 2);
    register(ns_prepend_listener2 as *const u8, 2);
    register(ns_prepend_once_listener2 as *const u8, 2);
    register(ns_remove_listener2 as *const u8, 2);
    register(ns_off2 as *const u8, 2);
    register(ns_remove_all_listeners1 as *const u8, 1);
    register(ns_readable_from_drain as *const u8, 0);
    register(ns_readable_event_microtask as *const u8, 0);
    register(ns_readable_end_microtask as *const u8, 0);
    register(ns_writable_finish_microtask as *const u8, 0);
    register(ns_construct_callback_done as *const u8, 1);
    register(ns_writable_final_callback_done as *const u8, 1);
    register(ns_capture_rejection as *const u8, 1);
    register(ns_emit2 as *const u8, 2);
    crate::closure::js_register_closure_rest(ns_emit_rest as *const u8, 1);
    register(ns_resume0 as *const u8, 0);
    register(ns_async_dispose as *const u8, 0);
    register(ns_read1 as *const u8, 1);
    register(ns_pipe2 as *const u8, 2);
    register(ns_writable_write_done as *const u8, 1);
    register(pipe_unpipe_callback as *const u8, 1);
    register(pipe_error_callback as *const u8, 1);
    register(pipe_close_callback as *const u8, 0);
    register(pipe_finish_callback as *const u8, 0);
    register(pipe_drain_callback as *const u8, 0);
    register(pipe_finish_destination_callback as *const u8, 0);
    register(writable_write_callback_noop as *const u8, 0);
    register(duplex_pair_write_callback as *const u8, 3);
    register(duplex_pair_final_callback as *const u8, 1);
    register(transform_write_callback as *const u8, 2);
    register(transform_flush_callback as *const u8, 2);
    register(pipeline_success_callback as *const u8, 0);
    register(pipeline_error_callback as *const u8, 1);
    register(pipeline_close_callback as *const u8, 0);
    register(compose_stage_error_callback as *const u8, 1);
    register(compose_source_data_callback as *const u8, 1);
    register(compose_source_end_callback as *const u8, 0);
    register(compose_source_error_callback as *const u8, 1);
    register(compose_duplex_write_callback as *const u8, 3);
    register(compose_duplex_final_callback as *const u8, 1);
    register(ns_write3 as *const u8, 3);
    register(ns_end3 as *const u8, 3);
    register(ns_cork0 as *const u8, 0);
    register(ns_uncork0 as *const u8, 0);
    register(ns_set_max_listeners as *const u8, 1);
    register(ns_get_max_listeners as *const u8, 0);
    register(ns_event_names as *const u8, 0);
    register(ns_listener_count as *const u8, 1);
    register(ns_listeners as *const u8, 1);
    register(ns_raw_listeners as *const u8, 1);
    register(ns_undefined0 as *const u8, 0);
    register(ns_push1 as *const u8, 1);
    register(ns_unshift1 as *const u8, 1);
    register(ns_compose1 as *const u8, 1);
    register(ns_pause0 as *const u8, 0);
    register(ns_is_paused0 as *const u8, 0);
    register(ns_unpipe1 as *const u8, 1);
    register(ns_readable_resume_microtask as *const u8, 0);
    register(
        super::readable_from_promises::ns_readable_from_promise_fulfilled as *const u8,
        1,
    );
    register(
        super::readable_from_promises::ns_readable_from_promise_rejected as *const u8,
        1,
    );
    register(ns_finished_error_false_close as *const u8, 0);
    register(ns_finished_signal_abort as *const u8, 0);
    register(ns_iter_to_array as *const u8, 1);
    register(ns_iter_map as *const u8, 2);
    register(ns_iter_filter as *const u8, 2);
    register(ns_iter_reduce as *const u8, 3);
    register(ns_iter_for_each as *const u8, 2);
    register(ns_iter_find as *const u8, 2);
    register(ns_iter_some as *const u8, 2);
    register(ns_iter_every as *const u8, 2);
    register(ns_iter_flat_map as *const u8, 2);
    register(ns_iter_take as *const u8, 1);
    register(ns_iter_drop as *const u8, 1);
    register_consume_arities();
    async_iterator::register_arities();
}

#[inline]
pub(super) fn box_pointer(ptr: *const u8) -> f64 {
    f64::from_bits(JSValue::pointer(ptr).bits())
}

pub(super) fn install_stream_async_dispose_symbol(stream: f64) {
    let async_dispose = crate::symbol::well_known_symbol("asyncDispose");
    if async_dispose.is_null() {
        return;
    }
    let closure = js_closure_alloc(ns_async_dispose as *const u8, 1);
    crate::closure::js_closure_set_capture_ptr(closure, 0, stream.to_bits() as i64);
    set_hidden_value(
        stream,
        hidden_key(b"__perry_async_dispose__"),
        box_pointer(closure as *const u8),
    );
    unsafe {
        crate::symbol::js_object_set_symbol_property(
            stream,
            box_pointer(async_dispose as *const u8),
            box_pointer(closure as *const u8),
        );
    }
}

#[inline]
#[cfg(test)]
pub(super) fn box_string(ptr: *mut crate::string::StringHeader) -> f64 {
    f64::from_bits(JSValue::string_ptr(ptr).bits())
}

#[inline]
pub(super) fn raw_ptr_from_value(value: f64) -> usize {
    let bits = value.to_bits();
    let jsval = JSValue::from_bits(bits);
    if jsval.is_pointer() || jsval.is_string() || jsval.is_bigint() {
        return (bits & crate::value::POINTER_MASK) as usize;
    }
    if bits != 0 && bits < 0x0001_0000_0000_0000 {
        return bits as usize;
    }
    0
}

#[inline]
pub(super) unsafe fn gc_type_for_ptr(raw: usize) -> Option<u8> {
    if raw < crate::gc::GC_HEADER_SIZE + 0x1000 {
        return None;
    }
    let header = (raw as *const u8).sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader;
    let gc_type = (*header).obj_type;
    if gc_type <= crate::gc::GC_TYPE_MAX {
        Some(gc_type)
    } else {
        None
    }
}
