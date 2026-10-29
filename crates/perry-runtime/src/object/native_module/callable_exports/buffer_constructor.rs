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

        let func_ptr = crate::fn_info!(#[realm(Buffer, kind = constructor, capture = [ref(Buffer.prototype)], extends = Uint8Array, attrs = {prototype = writable}, members = { from, attrs = writable | enumerable | configurable; alloc, attrs = writable | enumerable | configurable; allocUnsafe, attrs = writable | enumerable | configurable; allocUnsafeSlow, attrs = writable | enumerable | configurable; concat, attrs = writable | enumerable | configurable; of, attrs = writable | enumerable | configurable; isBuffer, attrs = writable | enumerable | configurable; isEncoding, attrs = writable | enumerable | configurable; byteLength, attrs = writable | enumerable | configurable; compare, attrs = writable | enumerable | configurable; copyBytesFrom, attrs = writable | enumerable | configurable; poolSize = 65536.0, attrs = writable | enumerable | configurable; prototype; [Symbol.species] = accessor(get = "Buffer[\"@@species\"].<get>"); })] buffer_constructor_thunk, 3; with_declared(3));
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

        let proto = crate::realm_decl! { birth js_object_alloc(0, 0); realm { Buffer.prototype, extends = Uint8Array.prototype, members = { constructor; toString, attrs = writable | enumerable | configurable; inspect, attrs = writable | enumerable | configurable; slice, attrs = writable | enumerable | configurable; subarray, attrs = writable | enumerable | configurable; copy, attrs = writable | enumerable | configurable; write, attrs = writable | enumerable | configurable; toJSON, attrs = writable | enumerable | configurable; fill, attrs = writable | enumerable | configurable; equals, attrs = writable | enumerable | configurable; compare, attrs = writable | enumerable | configurable; indexOf, attrs = writable | enumerable | configurable; lastIndexOf, attrs = writable | enumerable | configurable; includes, attrs = writable | enumerable | configurable; swap16, attrs = writable | enumerable | configurable; swap32, attrs = writable | enumerable | configurable; swap64, attrs = writable | enumerable | configurable; toLocaleString, attrs = writable | enumerable | configurable; readUInt8, attrs = writable | enumerable | configurable; readUint8, attrs = writable | enumerable | configurable; readInt8, attrs = writable | enumerable | configurable; readUInt16BE, attrs = writable | enumerable | configurable; readUint16BE, attrs = writable | enumerable | configurable; readUInt16LE, attrs = writable | enumerable | configurable; readUint16LE, attrs = writable | enumerable | configurable; readInt16BE, attrs = writable | enumerable | configurable; readInt16LE, attrs = writable | enumerable | configurable; readUInt32BE, attrs = writable | enumerable | configurable; readUint32BE, attrs = writable | enumerable | configurable; readUInt32LE, attrs = writable | enumerable | configurable; readUint32LE, attrs = writable | enumerable | configurable; readInt32BE, attrs = writable | enumerable | configurable; readInt32LE, attrs = writable | enumerable | configurable; readFloatBE, attrs = writable | enumerable | configurable; readFloatLE, attrs = writable | enumerable | configurable; readDoubleBE, attrs = writable | enumerable | configurable; readDoubleLE, attrs = writable | enumerable | configurable; readBigInt64BE, attrs = writable | enumerable | configurable; readBigInt64LE, attrs = writable | enumerable | configurable; readBigUInt64BE, attrs = writable | enumerable | configurable; readBigUint64BE, attrs = writable | enumerable | configurable; readBigUInt64LE, attrs = writable | enumerable | configurable; readBigUint64LE, attrs = writable | enumerable | configurable; readUIntBE, attrs = writable | enumerable | configurable; readUintBE, attrs = writable | enumerable | configurable; readUIntLE, attrs = writable | enumerable | configurable; readUintLE, attrs = writable | enumerable | configurable; readIntBE, attrs = writable | enumerable | configurable; readIntLE, attrs = writable | enumerable | configurable; writeUInt8, attrs = writable | enumerable | configurable; writeUint8, attrs = writable | enumerable | configurable; writeInt8, attrs = writable | enumerable | configurable; writeUInt16BE, attrs = writable | enumerable | configurable; writeUint16BE, attrs = writable | enumerable | configurable; writeUInt16LE, attrs = writable | enumerable | configurable; writeUint16LE, attrs = writable | enumerable | configurable; writeInt16BE, attrs = writable | enumerable | configurable; writeInt16LE, attrs = writable | enumerable | configurable; writeUInt32BE, attrs = writable | enumerable | configurable; writeUint32BE, attrs = writable | enumerable | configurable; writeUInt32LE, attrs = writable | enumerable | configurable; writeUint32LE, attrs = writable | enumerable | configurable; writeInt32BE, attrs = writable | enumerable | configurable; writeInt32LE, attrs = writable | enumerable | configurable; writeFloatBE, attrs = writable | enumerable | configurable; writeFloatLE, attrs = writable | enumerable | configurable; writeDoubleBE, attrs = writable | enumerable | configurable; writeDoubleLE, attrs = writable | enumerable | configurable; writeBigInt64BE, attrs = writable | enumerable | configurable; writeBigInt64LE, attrs = writable | enumerable | configurable; writeBigUInt64BE, attrs = writable | enumerable | configurable; writeBigUint64BE, attrs = writable | enumerable | configurable; writeBigUInt64LE, attrs = writable | enumerable | configurable; writeBigUint64LE, attrs = writable | enumerable | configurable; writeUIntBE, attrs = writable | enumerable | configurable; writeUintBE, attrs = writable | enumerable | configurable; writeUIntLE, attrs = writable | enumerable | configurable; writeUintLE, attrs = writable | enumerable | configurable; writeIntBE, attrs = writable | enumerable | configurable; writeIntLE, attrs = writable | enumerable | configurable; asciiSlice, attrs = writable | enumerable | configurable; asciiWrite, attrs = writable | enumerable | configurable; base64Slice, attrs = writable | enumerable | configurable; base64Write, attrs = writable | enumerable | configurable; base64urlSlice, attrs = writable | enumerable | configurable; base64urlWrite, attrs = writable | enumerable | configurable; hexSlice, attrs = writable | enumerable | configurable; hexWrite, attrs = writable | enumerable | configurable; latin1Slice, attrs = writable | enumerable | configurable; latin1Write, attrs = writable | enumerable | configurable; ucs2Slice, attrs = writable | enumerable | configurable; ucs2Write, attrs = writable | enumerable | configurable; utf8Slice, attrs = writable | enumerable | configurable; utf8Write, attrs = writable | enumerable | configurable; parent = accessor(get = "Buffer.prototype.parent.<get>"), attrs = enumerable; offset = accessor(get = "Buffer.prototype.offset.<get>"), attrs = enumerable; }; } };
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
                let method_ptr = crate::fn_info!(#[realm(realm_decl! { Buffer.prototype.asciiSlice, constructible = true; Buffer.prototype.asciiWrite, constructible = true; Buffer.prototype.base64Slice, constructible = true; Buffer.prototype.base64Write, constructible = true; Buffer.prototype.base64urlSlice, constructible = true; Buffer.prototype.base64urlWrite, constructible = true; Buffer.prototype.compare, constructible = true; Buffer.prototype.copy, constructible = true; Buffer.prototype.equals, constructible = true; Buffer.prototype.fill, constructible = true; Buffer.prototype.hexSlice, constructible = true; Buffer.prototype.hexWrite, constructible = true; Buffer.prototype.includes, constructible = true; Buffer.prototype.indexOf, constructible = true; Buffer.prototype.inspect, constructible = true; Buffer.prototype.lastIndexOf, constructible = true; Buffer.prototype.latin1Slice, constructible = true; Buffer.prototype.latin1Write, constructible = true; Buffer.prototype.readBigInt64BE, constructible = true; Buffer.prototype.readBigInt64LE, constructible = true; Buffer.prototype.readBigUInt64BE, constructible = true; Buffer.prototype.readBigUInt64LE, constructible = true; Buffer.prototype.readBigUint64BE, constructible = true; Buffer.prototype.readBigUint64LE, constructible = true; Buffer.prototype.readDoubleBE, constructible = true; Buffer.prototype.readDoubleLE, constructible = true; Buffer.prototype.readFloatBE, constructible = true; Buffer.prototype.readFloatLE, constructible = true; Buffer.prototype.readInt16BE, constructible = true; Buffer.prototype.readInt16LE, constructible = true; Buffer.prototype.readInt32BE, constructible = true; Buffer.prototype.readInt32LE, constructible = true; Buffer.prototype.readInt8, constructible = true; Buffer.prototype.readIntBE, constructible = true; Buffer.prototype.readIntLE, constructible = true; Buffer.prototype.readUInt16BE, constructible = true; Buffer.prototype.readUInt16LE, constructible = true; Buffer.prototype.readUInt32BE, constructible = true; Buffer.prototype.readUInt32LE, constructible = true; Buffer.prototype.readUInt8, constructible = true; Buffer.prototype.readUIntBE, constructible = true; Buffer.prototype.readUIntLE, constructible = true; Buffer.prototype.readUint16BE, constructible = true; Buffer.prototype.readUint16LE, constructible = true; Buffer.prototype.readUint32BE, constructible = true; Buffer.prototype.readUint32LE, constructible = true; Buffer.prototype.readUint8, constructible = true; Buffer.prototype.readUintBE, constructible = true; Buffer.prototype.readUintLE, constructible = true; Buffer.prototype.slice, constructible = true; Buffer.prototype.subarray, constructible = true; Buffer.prototype.swap16, constructible = true; Buffer.prototype.swap32, constructible = true; Buffer.prototype.swap64, constructible = true; Buffer.prototype.toJSON, constructible = true; Buffer.prototype.toLocaleString, constructible = true; Buffer.prototype.toString, constructible = true; Buffer.prototype.ucs2Slice, constructible = true; Buffer.prototype.ucs2Write, constructible = true; Buffer.prototype.utf8Slice, constructible = true; Buffer.prototype.utf8Write, constructible = true; Buffer.prototype.write, constructible = true; Buffer.prototype.writeBigInt64BE, constructible = true; Buffer.prototype.writeBigInt64LE, constructible = true; Buffer.prototype.writeBigUInt64BE, constructible = true; Buffer.prototype.writeBigUInt64LE, constructible = true; Buffer.prototype.writeBigUint64BE, constructible = true; Buffer.prototype.writeBigUint64LE, constructible = true; Buffer.prototype.writeDoubleBE, constructible = true; Buffer.prototype.writeDoubleLE, constructible = true; Buffer.prototype.writeFloatBE, constructible = true; Buffer.prototype.writeFloatLE, constructible = true; Buffer.prototype.writeInt16BE, constructible = true; Buffer.prototype.writeInt16LE, constructible = true; Buffer.prototype.writeInt32BE, constructible = true; Buffer.prototype.writeInt32LE, constructible = true; Buffer.prototype.writeInt8, constructible = true; Buffer.prototype.writeIntBE, constructible = true; Buffer.prototype.writeIntLE, constructible = true; Buffer.prototype.writeUInt16BE, constructible = true; Buffer.prototype.writeUInt16LE, constructible = true; Buffer.prototype.writeUInt32BE, constructible = true; Buffer.prototype.writeUInt32LE, constructible = true; Buffer.prototype.writeUInt8, constructible = true; Buffer.prototype.writeUIntBE, constructible = true; Buffer.prototype.writeUIntLE, constructible = true; Buffer.prototype.writeUint16BE, constructible = true; Buffer.prototype.writeUint16LE, constructible = true; Buffer.prototype.writeUint32BE, constructible = true; Buffer.prototype.writeUint32LE, constructible = true; Buffer.prototype.writeUint8, constructible = true; Buffer.prototype.writeUintBE, constructible = true; Buffer.prototype.writeUintLE, constructible = true; })] buffer_prototype_method_thunk, 0);
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
                    crate::fn_info!(#[realm("Buffer.prototype.parent.<get>", constructible = true)] buffer_prototype_parent_getter_thunk, 0; with_declared(0)),
                );
                install_buffer_prototype_getter(
                    proto,
                    "offset",
                    crate::fn_info!(#[realm("Buffer.prototype.offset.<get>", constructible = true)] buffer_prototype_offset_getter_thunk, 0; with_declared(0)),
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
