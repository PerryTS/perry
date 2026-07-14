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

/// #6316 — reserved own-key prefix for a native base method DISPLACED by a
/// subclass override.
///
/// The native bases perry models by stamping their method surface onto the
/// instance (`EventEmitter`, every `node:stream` class) install those methods as
/// ORDINARY OWN PROPERTIES. Own properties legitimately shadow class methods, so
/// perry's own-property-override probe (issue #620,
/// `perry-codegen/src/lower_call/method_override.rs`) selected the native
/// closure in preference to the user's `class Bus extends EventEmitter { emit()
/// {…} }` override — inheritance ran BACKWARDS and the override never executed.
///
/// The fix installs a base method under its plain name only when the receiver's
/// class chain does NOT declare it. When it does, the native closure is stashed
/// under `__perry_native_super__<name>` instead: invisible to ordinary property
/// lookup (so the class method wins), but still reachable from
/// `js_super_method_call_dynamic` so `super.emit(…)` lands on the real base
/// implementation. Hidden from own-key enumeration by
/// `object::field_get_set::enumeration::is_internal_runtime_key_bytes`.
pub(crate) const NATIVE_BASE_SUPER_PREFIX: &[u8] = b"__perry_native_super__";

/// `__perry_native_super__<name>` as an interned string key.
fn native_base_super_key(name: &str) -> *mut crate::string::StringHeader {
    let mut buf = Vec::with_capacity(NATIVE_BASE_SUPER_PREFIX.len() + name.len());
    buf.extend_from_slice(NATIVE_BASE_SUPER_PREFIX);
    buf.extend_from_slice(name.as_bytes());
    crate::string::js_string_from_bytes(buf.as_ptr(), buf.len() as u32)
}

/// The native base method that a subclass override displaced, if any (#6316).
/// `js_super_method_call_dynamic` calls this after the class-vtable and
/// prototype-method lookups on the parent chain have both missed — which is
/// always, for a native base, since `EventEmitter`/`Readable`/… are not perry
/// classes and own no registry entry to resolve against.
///
/// Returns `None` for a receiver that is not an object, or one carrying no
/// displaced method of that name, so an ordinary `super.m()` miss still yields
/// `undefined` (the #774 instance-field-shadow contract).
pub(crate) fn displaced_native_base_method(this_value: f64, name: &str) -> Option<f64> {
    unsafe {
        let jsval = JSValue::from_bits(this_value.to_bits());
        if !jsval.is_pointer() {
            return None;
        }
        let raw = (this_value.to_bits() & crate::value::POINTER_MASK) as usize;
        if raw == 0 || crate::value::addr_class::is_small_handle(raw) {
            return None;
        }
        let header = crate::value::addr_class::try_read_gc_header(raw)?;
        if header.obj_type != crate::gc::GC_TYPE_OBJECT {
            return None;
        }
        let obj = raw as *const ObjectHeader;
        let val = js_object_get_field_by_name_f64(obj, native_base_super_key(name));
        if JSValue::from_bits(val.to_bits()).is_pointer() {
            Some(val)
        } else {
            None
        }
    }
}

/// True when the receiver's class chain declares `name` as a real class method —
/// i.e. the user OVERRODE this native base method (#6316). The class registry is
/// populated at module init, long before any `new`, so the vtable is always
/// live by the time a constructor runs `super()`.
fn class_chain_overrides(class_id: u32, name: &str) -> bool {
    class_id != 0 && crate::object::method_owner_class_id(class_id, name).is_some()
}

pub(super) fn install_methods_on_existing_object(
    obj: *mut ObjectHeader,
    this_value: f64,
    methods: &[(&str, StubFn)],
    skip_names: &[&str],
) {
    register_stub_arities();
    // `js_closure_alloc` and the key interning below both allocate and can
    // therefore GC-move the receiver, so root it and re-read the raw pointer at
    // every use rather than trusting the `obj` snapshot across the loop. The
    // NaN-boxed `this` goes in a handle for the same reason: it is copied into
    // each closure's capture slot, and a stale value there would leave the
    // method bound to a dead receiver. (Captures already stored are traced and
    // rewritten by the GC; a bit-pattern parked in a Rust local is not.)
    let scope = crate::gc::RuntimeHandleScope::new();
    let obj_handle = scope.root_raw_mut_ptr(obj);
    let this_handle = scope.root_nanbox_f64(this_value);
    let class_id = crate::object::js_object_get_class_id(obj);

    let mut on_method: Option<f64> = None;
    for (name, func) in methods {
        if skip_names.iter().any(|skip| skip == name) {
            continue;
        }
        // #6316: the subclass declares this method — the native base version
        // must NOT become an own property, or it would shadow the override.
        // Stash it where only `super.<name>()` can find it.
        let overridden = class_chain_overrides(class_id, name);

        // `addListener` is an ALIAS of `EventEmitter.prototype.on` in Node, so
        // it reuses the base `on` closure even when the subclass overrides
        // `on` — `emitter.addListener(…)` must reach the base, not the override.
        if *name == "addListener" {
            if let Some(val) = on_method {
                let key = native_or_plain_key(name, overridden);
                js_object_set_field_by_name(obj_handle.get_raw_mut_ptr::<ObjectHeader>(), key, val);
                continue;
            }
        }
        let closure = js_closure_alloc(*func as *const u8, 1);
        crate::closure::js_closure_set_capture_ptr(
            closure,
            0,
            this_handle.get_nanbox_f64().to_bits() as i64,
        );
        let val = f64::from_bits(JSValue::pointer(closure as *const u8).bits());
        if *name == "on" {
            on_method = Some(val);
        }
        let key = native_or_plain_key(name, overridden);
        js_object_set_field_by_name(obj_handle.get_raw_mut_ptr::<ObjectHeader>(), key, val);
    }
}

/// The key an installed base method lands on: its plain name normally, or the
/// reserved super-only key when the subclass overrides it (#6316).
fn native_or_plain_key(name: &str, overridden: bool) -> *mut crate::string::StringHeader {
    if overridden {
        native_base_super_key(name)
    } else {
        hidden_key(name.as_bytes())
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
