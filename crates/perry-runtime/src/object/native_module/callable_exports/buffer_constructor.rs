//! The native Buffer constructor and its traced intrinsic prototype edge.

use super::*;

pub(crate) fn buffer_constructor_value() -> f64 {
    BUFFER_CONSTRUCTOR_VALUE.with(|slot| {
        let cached = slot.get();
        if cached != 0 {
            return f64::from_bits(cached);
        }

        // #6924: the statics minted below are BOUND_METHOD closures that
        // dispatch by name through the "buffer.Buffer" namespace, and that
        // dispatch resolves via the per-module registry
        // (`nm_dispatch_lookup`). The registry's soundness rule — a bound
        // export exists only after its module's `js_nm_install_*` ran — is
        // upheld by codegen for IMPORTED modules, but `Buffer` is a global:
        // this mint runs with no `buffer` import anywhere, so nothing armed
        // the bucket and every inherited/captured static (`MyBuf.from`,
        // `const f = B.from`) silently returned `undefined`. Arm it at the
        // mint, mirroring `install_native_module_vtable()` above.
        crate::object::native_module_registry::js_nm_install_buffer();

        let func_ptr = crate::fn_info!(buffer_constructor_thunk, 3; with_declared(3));
        let closure = crate::closure::js_closure_alloc(func_ptr, 1);
        if closure.is_null() {
            return f64::from_bits(crate::value::TAG_UNDEFINED);
        }
        let scope = crate::gc::RuntimeHandleScope::new();
        let closure = scope.root_raw_mut_ptr(closure);
        closure.with_mut_ptr::<crate::closure::ClosureHeader, _>(|ptr| {
            set_bound_native_closure_name(ptr, "Buffer")
        });

        for method in BUFFER_STATIC_METHODS {
            let method_value =
                scope.root_nanbox_f64(bound_native_callable_export_value("buffer.Buffer", method));
            closure.with_mut_ptr(|closure: *mut crate::closure::ClosureHeader| {
                crate::closure::closure_set_dynamic_prop(
                    closure as usize,
                    method,
                    method_value.get_nanbox_f64(),
                )
            });
        }

        closure.with_mut_ptr(|closure: *mut crate::closure::ClosureHeader| {
            crate::closure::closure_set_dynamic_prop(
                closure as usize,
                "poolSize",
                buffer_pool_size(),
            )
        });

        let proto = js_object_alloc(0, 0);
        if !proto.is_null() {
            let proto = scope.root_raw_mut_ptr(proto);
            let constructor = "constructor";
            let constructor_key = scope.root_string_ptr(crate::string::js_string_from_bytes(
                constructor.as_ptr(),
                constructor.len() as u32,
            ));
            proto.with_mut_ptr(|proto: *mut ObjectHeader| {
                constructor_key.with_mut_ptr(|constructor_key| {
                    closure.with_mut_ptr(|closure: *mut crate::closure::ClosureHeader| {
                        js_object_set_field_by_name(
                            proto,
                            constructor_key,
                            crate::value::js_nanbox_pointer(closure as i64),
                        )
                    })
                })
            });
            proto.with_mut_ptr(|proto: *mut ObjectHeader| {
                crate::object::set_builtin_property_attrs(
                    proto as usize,
                    constructor.to_string(),
                    crate::object::PropertyAttrs::new(true, false, true),
                )
            });

            for method in BUFFER_PROTOTYPE_METHODS {
                let method_ptr = crate::fn_info!(buffer_prototype_method_thunk, 0);
                let method_closure = crate::closure::js_closure_alloc(method_ptr, 0);
                if method_closure.is_null() {
                    continue;
                }
                let method_closure = scope.root_raw_mut_ptr(method_closure);
                method_closure.with_mut_ptr::<crate::closure::ClosureHeader, _>(|ptr| {
                    set_bound_native_closure_name(ptr, method)
                });
                let key = scope.root_string_ptr(crate::string::js_string_from_bytes(
                    method.as_ptr(),
                    method.len() as u32,
                ));
                proto.with_mut_ptr(|proto: *mut ObjectHeader| {
                    key.with_mut_ptr(|key| {
                        method_closure.with_mut_ptr(
                            |method_closure: *mut crate::closure::ClosureHeader| {
                                js_object_set_field_by_name(
                                    proto,
                                    key,
                                    crate::value::js_nanbox_pointer(method_closure as i64),
                                )
                            },
                        )
                    })
                });
            }
            proto.with_mut_ptr(|proto: *mut ObjectHeader| {
                install_buffer_prototype_getter(
                    proto,
                    "parent",
                    crate::fn_info!(buffer_prototype_parent_getter_thunk, 0; with_declared(0)),
                );
                install_buffer_prototype_getter(
                    proto,
                    "offset",
                    crate::fn_info!(buffer_prototype_offset_getter_thunk, 0; with_declared(0)),
                );
            });
            let proto_value = proto.with_mut_ptr(|proto: *mut ObjectHeader| {
                crate::value::js_nanbox_pointer(proto as i64)
            });
            closure.with_mut_ptr(|closure: *mut crate::closure::ClosureHeader| {
                crate::closure::js_closure_set_capture_f64(closure, 0, proto_value);
            });
            closure.with_mut_ptr(|closure: *mut crate::closure::ClosureHeader| {
                crate::closure::closure_set_dynamic_prop(closure as usize, "prototype", proto_value)
            });
            closure.with_mut_ptr(|closure: *mut crate::closure::ClosureHeader| {
                crate::object::set_builtin_property_attrs(
                    closure as usize,
                    "prototype".to_string(),
                    crate::object::PropertyAttrs::new(true, false, false),
                )
            });
        }

        let value = closure.with_mut_ptr(|closure: *mut crate::closure::ClosureHeader| {
            crate::value::js_nanbox_pointer(closure as i64)
        });
        slot.set(value.to_bits());
        // #11193: `Buffer[Symbol.species]` (FastBuffer). After the slot is
        // set, so a re-entrant read of `Buffer` during the install sees it.
        buffer_species::install_buffer_species(value);
        f64::from_bits(slot.get())
    })
}

pub(crate) fn is_buffer_constructor_value(value: f64) -> bool {
    BUFFER_CONSTRUCTOR_VALUE.with(|slot| {
        let cached = slot.get();
        cached != 0 && cached == value.to_bits()
    })
}

/// The native constructor owns this intrinsic edge in its traced capture.
/// No observable property, pointer cache or additional root holder is needed.
pub(crate) fn cached_buffer_intrinsic_prototype_value() -> Option<f64> {
    BUFFER_CONSTRUCTOR_VALUE.with(|slot| {
        let ctor = crate::value::JSValue::from_bits(slot.get());
        if !ctor.is_pointer() {
            return None;
        }
        Some(crate::closure::js_closure_get_capture_f64(
            ctor.as_pointer(),
            0,
        ))
    })
}

pub(crate) fn buffer_original_prototype_value() -> f64 {
    buffer_constructor_value();
    cached_buffer_intrinsic_prototype_value()
        .unwrap_or_else(|| f64::from_bits(crate::value::TAG_UNDEFINED))
}

/// Materialize the default parent only when the ordinary property walk needs
/// it. Buffer import and positive metadata reads need no global builtin tower.
pub(crate) fn buffer_intrinsic_prototype_value() -> f64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let proto = scope.root_nanbox_f64(buffer_original_prototype_value());
    let value = crate::value::JSValue::from_bits(proto.get_nanbox_f64().to_bits());
    if value.is_pointer()
        && crate::object::prototype_chain::object_static_prototype(value.as_pointer::<u8>() as usize).is_none()
    {
        let parent = scope.root_nanbox_f64(crate::object::global_this::typed_array_uint8_intrinsic_prototype_value());
        let addr = crate::value::JSValue::from_bits(proto.get_nanbox_f64().to_bits()).as_pointer::<u8>() as usize;
        crate::object::prototype_chain::object_set_static_prototype(addr, parent.get_nanbox_f64().to_bits());
    }
    proto.get_nanbox_f64()
}

pub(crate) fn buffer_intrinsic_prototype_parent(owner: usize) -> Option<f64> {
    let value =
        crate::value::JSValue::from_bits(cached_buffer_intrinsic_prototype_value()?.to_bits());
    if !value.is_pointer() || value.as_pointer::<u8>() as usize != owner {
        return None;
    }
    let proto = crate::value::JSValue::from_bits(buffer_intrinsic_prototype_value().to_bits());
    crate::object::prototype_chain::object_static_prototype(proto.as_pointer::<u8>() as usize)
        .map(f64::from_bits)
}
