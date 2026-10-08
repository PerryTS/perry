//! node:stream — method-table builders, stub-arity registration, pointer/GC helpers (split out of node_stream.rs for the 2000-line
//! file-size gate, #1987). Shares the parent module's constants, hidden-key
//! accessors and state primitives via `use super::*`.
use super::*;
use crate::closure::{js_closure_alloc, ClosureHeader};
use crate::object::{
    js_object_alloc_with_shape, js_object_get_field_by_name_f64, js_object_set_field,
    js_object_set_field_by_name, ObjectHeader,
};
use crate::value::JSValue;

/// A stream-method body's static info: what a method table lists and
/// `build_object` / `install_methods_on_existing_object` allocate from
/// (`crate::fn_info!(body, N)`).
pub(crate) type StubFn = *const crate::closure::JsFunctionInfo;

// ─────────────────────────────────────────────────────────────────
// Build the host object: allocate an ObjectHeader sized to the
// method set, then fill each slot with a closure that captures the
// host object's NaN-boxed value (so `this` chains return identity).
// ─────────────────────────────────────────────────────────────────

pub(super) fn build_object(methods: &[(&str, StubFn)], shape_id: u32) -> *mut ObjectHeader {
    // Pack the method names as a NUL-separated byte sequence, matching
    // the layout `js_object_alloc_with_shape` parses for shape keys.
    let mut packed: Vec<u8> = Vec::new();
    for (name, _) in methods {
        packed.extend_from_slice(name.as_bytes());
        packed.push(0);
    }
    let field_count = methods.len() as u32;
    let scope = crate::gc::RuntimeHandleScope::new();
    let obj = scope.root_raw_mut_ptr(js_object_alloc_with_shape(
        shape_id,
        field_count,
        packed.as_ptr(),
        packed.len() as u32,
    ));

    let mut on_method: Option<crate::gc::RuntimeHandle<'_>> = None;
    for (i, (name, func)) in methods.iter().enumerate() {
        if *name == "addListener" {
            if let Some(on_method) = on_method {
                obj.with_mut_ptr(|obj| {
                    on_method.with_const_ptr(|closure| {
                        js_object_set_field(obj, i as u32, JSValue::pointer(closure))
                    })
                });
                continue;
            }
        }
        let closure = scope.root_raw_mut_ptr(js_closure_alloc(*func, 1));
        // Reuse `set_capture_ptr` (i64 payload). We only need 64 bits
        // and the NaN-boxed pattern fits cleanly when reinterpreted.
        closure.with_mut_ptr(|closure| {
            let this_bits = obj.with_const_ptr(|obj| JSValue::pointer(obj).bits());
            crate::closure::js_closure_set_capture_ptr(closure, 0, this_bits as i64);
        });
        if *name == "on" {
            on_method = Some(closure);
        }
        obj.with_mut_ptr(|obj| {
            closure.with_const_ptr(|closure| {
                js_object_set_field(obj, i as u32, JSValue::pointer(closure))
            })
        });
    }
    obj.with_mut_ptr(|obj| obj)
}

/// True when the receiver's class chain declares `name` as a real class method —
/// i.e. the user OVERRODE this native base method (#6316). The class registry is
/// populated at module init, long before any `new`, so the vtable is always
/// live by the time a constructor runs `super()`.
fn class_chain_overrides(class_id: u32, name: &str) -> bool {
    class_id != 0 && crate::object::method_owner_class_id(class_id, name).is_some()
}

pub(crate) fn install_methods_on_existing_object(
    obj: *mut ObjectHeader,
    this_value: f64,
    methods: &[(&'static str, StubFn)],
    skip_names: &[&str],
) {
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

    let mut on_method: Option<crate::gc::RuntimeHandle<'_>> = None;
    for (name, func) in methods {
        if skip_names.iter().any(|skip| skip == name) {
            continue;
        }
        // A native own property would shadow the subclass override.
        // `super` reads the native prototype directly.
        if class_chain_overrides(class_id, name) {
            continue;
        }

        // `addListener` is an ALIAS of `EventEmitter.prototype.on` in Node, so
        // it reuses the base `on` closure even when the subclass overrides
        // `on` — `emitter.addListener(…)` must reach the base, not the override.
        if *name == "addListener" {
            if let Some(val) = &on_method {
                let key = hidden_key(name.as_bytes());
                js_object_set_field_by_name(
                    obj_handle.get_raw_mut_ptr::<ObjectHeader>(),
                    key,
                    val.get_nanbox_f64(),
                );
                continue;
            }
        }
        let closure = js_closure_alloc(*func, 1);
        crate::closure::js_closure_set_capture_ptr(
            closure,
            0,
            this_handle.get_nanbox_f64().to_bits() as i64,
        );
        let val = scope.root_nanbox_f64(f64::from_bits(
            JSValue::pointer(closure as *const u8).bits(),
        ));
        let key = hidden_key(name.as_bytes());
        js_object_set_field_by_name(
            obj_handle.get_raw_mut_ptr::<ObjectHeader>(),
            key,
            val.get_nanbox_f64(),
        );
        if *name == "on" {
            on_method = Some(val);
        }
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
    let scope = crate::gc::RuntimeHandleScope::new();
    let proto = scope.root_raw_mut_ptr(proto);
    let methods = super::emitter_methods();
    let mut on_method: Option<crate::gc::RuntimeHandle<'_>> = None;
    for (name, func) in methods {
        if name == "addListener" {
            if let Some(val) = &on_method {
                let key = scope.root_string_ptr(hidden_key(name.as_bytes()));
                proto.with_mut_ptr::<ObjectHeader, _>(|proto| {
                    key.with_const_ptr::<crate::StringHeader, _>(|key| {
                        js_object_set_field_by_name(proto, key, val.get_nanbox_f64())
                    })
                });
                continue;
            }
        }
        let closure = js_closure_alloc(func, 1);
        crate::closure::js_closure_set_capture_ptr(closure, 0, crate::value::TAG_UNDEFINED as i64);
        let val = scope.root_nanbox_f64(f64::from_bits(
            JSValue::pointer(closure as *const u8).bits(),
        ));
        if name == "on" {
            on_method = Some(val);
        }
        let key = scope.root_string_ptr(hidden_key(name.as_bytes()));
        proto.with_mut_ptr::<ObjectHeader, _>(|proto| {
            key.with_const_ptr::<crate::StringHeader, _>(|key| {
                js_object_set_field_by_name(proto, key, val.get_nanbox_f64())
            })
        });
    }
}

/// node's `EventEmitter.prototype`: the instance-state defaults
/// (`_events`/`_eventsCount`/`_maxListeners`) then the methods, in the order
/// `lib/events.js` assigns them, with node's two aliases (`addListener` IS
/// `on`, `off` IS `removeListener`). Every closure reads its receiver from the
/// call-site `this` (slot 0 holds `TAG_UNDEFINED`), so ONE set of closures
/// serves every emitter: no instance carries its own copy (#10508).
pub(crate) fn install_event_emitter_prototype(proto: *mut ObjectHeader) {
    let scope = crate::gc::RuntimeHandleScope::new();
    let proto = scope.root_raw_mut_ptr(proto);
    let proto_value = proto.with_const_ptr::<ObjectHeader, _>(|proto| {
        f64::from_bits(JSValue::pointer(proto as *const u8).bits())
    });
    super::install_event_emitter_prototype_state(proto_value);
    let methods = super::emitter_methods();
    let body = |name: &str| {
        methods
            .iter()
            .find(|(method, _)| *method == name)
            .map(|(_, info)| *info)
    };
    let mut values: Vec<(&str, crate::gc::RuntimeHandle<'_>)> = Vec::new();
    for name in [
        "setMaxListeners",
        "getMaxListeners",
        "emit",
        "on",
        "prependListener",
        "once",
        "prependOnceListener",
        "removeListener",
        "removeAllListeners",
        "listeners",
        "rawListeners",
        "listenerCount",
        "eventNames",
    ] {
        let Some(info) = body(name) else { continue };
        let closure = js_closure_alloc(info, 1);
        crate::closure::js_closure_set_capture_ptr(closure, 0, crate::value::TAG_UNDEFINED as i64);
        let value = scope.root_nanbox_f64(f64::from_bits(
            JSValue::pointer(closure as *const u8).bits(),
        ));
        values.push((name, value));
    }
    let value_of = |name: &str| {
        values
            .iter()
            .find(|(method, _)| *method == name)
            .map(|(_, value)| value.get_nanbox_f64())
    };
    for (name, source) in [
        ("setMaxListeners", "setMaxListeners"),
        ("getMaxListeners", "getMaxListeners"),
        ("emit", "emit"),
        ("addListener", "on"),
        ("on", "on"),
        ("prependListener", "prependListener"),
        ("once", "once"),
        ("prependOnceListener", "prependOnceListener"),
        ("removeListener", "removeListener"),
        ("off", "removeListener"),
        ("removeAllListeners", "removeAllListeners"),
        ("listeners", "listeners"),
        ("rawListeners", "rawListeners"),
        ("listenerCount", "listenerCount"),
        ("eventNames", "eventNames"),
    ] {
        let Some(value) = value_of(source) else {
            continue;
        };
        let value = scope.root_nanbox_f64(value);
        let key = scope.root_string_ptr(hidden_key(name.as_bytes()));
        proto.with_mut_ptr::<ObjectHeader, _>(|proto| {
            key.with_const_ptr::<crate::StringHeader, _>(|key| {
                js_object_set_field_by_name(proto, key, value.get_nanbox_f64())
            })
        });
    }
}

/// The `AsyncResource` behind an `EventEmitterAsyncResource` receiver: the
/// public resource object held in its hidden field.
fn event_emitter_async_resource_backing(receiver: f64) -> Option<i64> {
    let scope = crate::gc::RuntimeHandleScope::new();
    let receiver = scope.root_nanbox_f64(receiver);
    let bits = receiver.get_nanbox_f64().to_bits();
    if bits >> 48 != 0x7FFD {
        return None;
    }
    let raw = (bits & crate::value::POINTER_MASK) as usize;
    if !crate::value::addr_class::is_plausible_heap_addr(raw) {
        return None;
    }
    let key = scope.root_string_ptr(hidden_key(EVENT_EMITTER_ASYNC_RESOURCE_KEY));
    let raw = (receiver.get_nanbox_f64().to_bits() & crate::value::POINTER_MASK) as usize;
    let value = key.with_const_ptr::<crate::StringHeader, _>(|key| {
        js_object_get_field_by_name_f64(raw as *const ObjectHeader, key)
    });
    if value.to_bits() >> 48 != 0x7FFD {
        return None;
    }
    let resource = (value.to_bits() & crate::value::POINTER_MASK) as i64;
    // #10926: the hidden field holds what `js_async_resource_new` returned --
    // the public handle OBJECT, not the native backing -- so brand it by
    // resolving, not by backing-registry membership. `resource` stays the
    // public object: the `asyncResource` getter hands it to JS, and every
    // `js_async_resource_*` entry point resolves it.
    crate::async_hooks::resolve_async_resource_handle(resource).map(|_| resource)
}

fn require_event_emitter_async_resource_receiver(
    closure: *const ClosureHeader,
    this: crate::closure::JsThis,
) -> i64 {
    if let Some(backing) = event_emitter_async_resource_backing(this_value(closure, this)) {
        return backing;
    }
    crate::node_submodules::diagnostics::throw_type_error_no_code(
        b"Cannot read private member from an object whose class did not declare it",
    )
}

extern "C" fn ns_ee_async_resource_emit_rest(
    closure: *const ClosureHeader,
    this: crate::closure::JsThis,
    event: f64,
    rest: f64,
) -> f64 {
    let resource = require_event_emitter_async_resource_receiver(closure, this);
    let runtime_async_id = crate::async_hooks::js_async_resource_async_id(resource) as u64;
    if runtime_async_id != 0 {
        crate::async_hooks::js_async_hooks_provider_enter(runtime_async_id);
    }
    let result = ns_emit_rest(closure, this, event, rest);
    if runtime_async_id != 0 {
        crate::async_hooks::js_async_hooks_provider_leave(runtime_async_id);
    }
    result
}

extern "C" fn ns_ee_async_resource_destroy(
    closure: *const ClosureHeader,
    this: crate::closure::JsThis,
) -> f64 {
    let resource = require_event_emitter_async_resource_receiver(closure, this);
    crate::async_hooks::js_async_resource_emit_destroy(resource)
}

/// `EventEmitterAsyncResource.prototype.emit`'s body: `(event, ...args)`.
static NS_EE_ASYNC_RESOURCE_EMIT_REST: crate::closure::JsFunctionInfo =
    crate::closure::JsFunctionInfo::of(
        ns_ee_async_resource_emit_rest as crate::codegen_abi::JsBody2<ClosureHeader>,
    )
    .with_rest(1);

/// `emitDestroy`'s body.
static NS_EE_ASYNC_RESOURCE_DESTROY: crate::closure::JsFunctionInfo =
    crate::closure::JsFunctionInfo::of(
        ns_ee_async_resource_destroy as crate::codegen_abi::JsBody0<ClosureHeader>,
    )
    .with_declared(0);

extern "C" fn ns_ee_async_resource_getter(
    closure: *const ClosureHeader,
    this: crate::closure::JsThis,
) -> f64 {
    let operation = crate::closure::js_closure_get_capture_ptr(closure, 1) as u32;
    let resource = require_event_emitter_async_resource_receiver(closure, this);
    match operation {
        0 => crate::async_hooks::js_async_resource_async_id(resource),
        1 => crate::async_hooks::js_async_resource_trigger_async_id(resource),
        2 => f64::from_bits(crate::value::js_nanbox_pointer(resource).to_bits()),
        _ => f64::from_bits(crate::value::TAG_UNDEFINED),
    }
}

/// Complete the `EventEmitterAsyncResource.prototype` surface. Its inherited
/// EventEmitter methods remain available, while `emit` and the resource
/// accessors enforce Node's private-brand receiver validation.
pub(crate) unsafe fn install_event_emitter_async_resource_prototype(proto: *mut ObjectHeader) {
    let scope = crate::gc::RuntimeHandleScope::new();
    let proto = scope.root_raw_mut_ptr(proto);
    let install_method = |name: &'static str, function: *const crate::closure::JsFunctionInfo| {
        let closure = js_closure_alloc(function, 1);
        crate::closure::js_closure_set_capture_ptr(closure, 0, crate::value::TAG_UNDEFINED as i64);
        let closure = scope.root_raw_mut_ptr(closure);
        let key = scope.root_string_ptr(hidden_key(name.as_bytes()));
        proto.with_mut_ptr::<ObjectHeader, _>(|proto| {
            key.with_const_ptr::<crate::StringHeader, _>(|key| {
                closure.with_const_ptr::<ClosureHeader, _>(|closure| {
                    js_object_set_field_by_name(
                        proto,
                        key,
                        f64::from_bits(JSValue::pointer(closure as *const u8).bits()),
                    );
                });
            });
        });
        proto.with_mut_ptr::<ObjectHeader, _>(|proto| {
            crate::object::set_builtin_property_attrs(
                proto as usize,
                name.to_string(),
                crate::object::PropertyAttrs::new(true, false, true),
            );
        });
    };
    install_method("emit", &NS_EE_ASYNC_RESOURCE_EMIT_REST);
    install_method("emitDestroy", &NS_EE_ASYNC_RESOURCE_DESTROY);

    for (name, operation) in [
        ("asyncId", 0_i64),
        ("triggerAsyncId", 1),
        ("asyncResource", 2),
    ] {
        let closure = js_closure_alloc(
            crate::fn_info!(ns_ee_async_resource_getter, 0; with_declared(0)),
            2,
        );
        crate::closure::js_closure_set_capture_ptr(closure, 0, crate::value::TAG_UNDEFINED as i64);
        crate::closure::js_closure_set_capture_ptr(closure, 1, operation);
        let closure = scope.root_raw_mut_ptr(closure);
        let key = scope.root_string_ptr(hidden_key(name.as_bytes()));
        proto.with_mut_ptr::<ObjectHeader, _>(|proto| {
            key.with_const_ptr::<crate::StringHeader, _>(|key| {
                js_object_set_field_by_name(
                    proto,
                    key,
                    f64::from_bits(crate::value::TAG_UNDEFINED),
                );
            });
        });
        proto.with_mut_ptr::<ObjectHeader, _>(|proto| {
            closure.with_const_ptr::<ClosureHeader, _>(|closure| {
                crate::object::set_builtin_accessor_descriptor(
                    proto as usize,
                    name.to_string(),
                    crate::object::AccessorDescriptor {
                        get: JSValue::pointer(closure as *const u8).bits(),
                        set: 0,
                    },
                    crate::object::PropertyAttrs::new(true, false, true),
                );
            });
        });
    }
}

pub(crate) unsafe fn install_event_emitter_async_resource_instance_methods(
    obj: *mut ObjectHeader,
    this_value: f64,
) {
    let scope = crate::gc::RuntimeHandleScope::new();
    let obj = scope.root_raw_mut_ptr(obj);
    let this_value = scope.root_nanbox_f64(this_value);
    for (name, function) in [
        ("emit", &NS_EE_ASYNC_RESOURCE_EMIT_REST),
        ("emitDestroy", &NS_EE_ASYNC_RESOURCE_DESTROY),
    ] {
        let closure = js_closure_alloc(function, 1);
        crate::closure::js_closure_set_capture_ptr(
            closure,
            0,
            this_value.get_nanbox_f64().to_bits() as i64,
        );
        let closure = scope.root_raw_mut_ptr(closure);
        let key = scope.root_string_ptr(hidden_key(name.as_bytes()));
        obj.with_mut_ptr::<ObjectHeader, _>(|obj| {
            key.with_const_ptr::<crate::StringHeader, _>(|key| {
                closure.with_const_ptr::<ClosureHeader, _>(|closure| {
                    js_object_set_field_by_name(
                        obj,
                        key,
                        f64::from_bits(JSValue::pointer(closure as *const u8).bits()),
                    );
                });
            });
        });
    }

    // Source-compiled subclasses are plain runtime objects rather than
    // external emitter handles. Bind the resource accessors directly to the
    // instance just like emit/emitDestroy; relying only on the native parent
    // prototype leaves dynamic loop/property reads as `undefined`.
    for (name, operation) in [
        ("asyncId", 0_i64),
        ("triggerAsyncId", 1),
        ("asyncResource", 2),
    ] {
        let closure = js_closure_alloc(
            crate::fn_info!(ns_ee_async_resource_getter, 0; with_declared(0)),
            2,
        );
        crate::closure::js_closure_set_capture_ptr(
            closure,
            0,
            this_value.get_nanbox_f64().to_bits() as i64,
        );
        crate::closure::js_closure_set_capture_ptr(closure, 1, operation);
        let closure = scope.root_raw_mut_ptr(closure);
        let key = scope.root_string_ptr(hidden_key(name.as_bytes()));
        obj.with_mut_ptr::<ObjectHeader, _>(|obj| {
            key.with_const_ptr::<crate::StringHeader, _>(|key| {
                // This seeds an own descriptor during native construction;
                // an inherited getter must not turn it into an ordinary Set.
                crate::object::object_ops::define_property_force_store_value(
                    obj,
                    key,
                    f64::from_bits(crate::value::TAG_UNDEFINED),
                );
            });
        });
        obj.with_mut_ptr::<ObjectHeader, _>(|obj| {
            closure.with_const_ptr::<ClosureHeader, _>(|closure| {
                crate::object::set_builtin_accessor_descriptor(
                    obj as usize,
                    name.to_string(),
                    crate::object::AccessorDescriptor {
                        get: JSValue::pointer(closure as *const u8).bits(),
                        set: 0,
                    },
                    crate::object::PropertyAttrs::new(true, false, true),
                );
            });
        });
    }
}

#[inline]
pub(super) fn box_pointer(ptr: *const u8) -> f64 {
    f64::from_bits(JSValue::pointer(ptr).bits())
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
