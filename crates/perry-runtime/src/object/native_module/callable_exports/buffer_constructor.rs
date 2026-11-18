//! The native Buffer constructor and its traced intrinsic prototype edge.

use super::*;

/// Buffer's own constructor info: the realm row's (`global_this_tables.rs`),
/// so the constructor setup binds is Buffer's own closure.
pub(crate) const BUFFER_CONSTRUCTOR_INFO: *const crate::closure::JsFunctionInfo =
    crate::fn_info!(#[realm(Buffer, kind = constructor, capture = [ref(Buffer.prototype)], extends = Uint8Array, attrs = {prototype = writable}, members = { from, attrs = writable | enumerable | configurable; alloc, attrs = writable | enumerable | configurable; allocUnsafe, attrs = writable | enumerable | configurable; allocUnsafeSlow, attrs = writable | enumerable | configurable; concat, attrs = writable | enumerable | configurable; of, attrs = writable | enumerable | configurable; isBuffer, attrs = writable | enumerable | configurable; isEncoding, attrs = writable | enumerable | configurable; byteLength, attrs = writable | enumerable | configurable; compare, attrs = writable | enumerable | configurable; copyBytesFrom, attrs = writable | enumerable | configurable; poolSize = 65536.0, attrs = writable | enumerable | configurable; [Symbol.species] = accessor(get = "Buffer[\"@@species\"].<get>"); })] export crate::object::native_module::callable_exports::buffer_constructor_thunk, 3; with_declared(3));

/// `%Buffer%`. Buffer is a lazy family: asking for its constructor value is
/// an observation, and births it (`birth_lazy_family`).
pub(crate) fn buffer_constructor_value() -> f64 {
    let cached = BUFFER_CONSTRUCTOR_VALUE.with(std::cell::Cell::get);
    if cached != 0 {
        return f64::from_bits(cached);
    }
    crate::object::birth_lazy_family(crate::object::BuiltinConstructorIdentity::Buffer);
    match BUFFER_CONSTRUCTOR_VALUE.with(std::cell::Cell::get) {
        0 => f64::from_bits(crate::value::TAG_UNDEFINED),
        bits => f64::from_bits(bits),
    }
}

/// Buffer's declared statics (`from` .. `copyBytesFrom`, then `poolSize`),
/// in declaration order, defined by its birth
/// (`install_builtin_constructor_statics`). #6924: the bound statics dispatch
/// by name through the "buffer.Buffer" namespace (`nm_dispatch_lookup`), and
/// `Buffer` is a global no `buffer` import arms: its birth arms the bucket.
pub(crate) fn buffer_constructor_statics(mut define: impl FnMut(&'static str, f64)) {
    crate::object::native_module_registry::js_nm_install_buffer();
    for method in BUFFER_STATIC_METHODS {
        define(
            method,
            bound_native_callable_export_value("buffer.Buffer", method),
        );
    }
    define("poolSize", buffer_pool_size());
}

/// Buffer's own part of its birth (`birth_family`, after the generic
/// prototype install): its `Buffer.prototype` members, the traced capture of
/// that prototype (instances answer it) and `Buffer[Symbol.species]`
/// (FastBuffer, which shares the prototype).
pub(crate) fn birth_buffer_own_parts(
    closure: *mut crate::closure::ClosureHeader,
    proto: *mut ObjectHeader,
) {
    set_bound_native_closure_name(closure, "Buffer");
    // The member table is the typed-array group's: a program that never
    // reaches a Buffer installs none of it and links none of its bodies.
    crate::typedarray_hook_slots::buffer_prototype_members(proto);
    crate::closure::js_closure_set_capture_f64(
        closure,
        0,
        crate::value::js_nanbox_pointer(proto as i64),
    );
    let value = crate::value::js_nanbox_pointer(closure as i64);
    BUFFER_CONSTRUCTOR_VALUE.with(|slot| slot.set(value.to_bits()));
    // After the slot is set, so a re-entrant read of `Buffer` sees it.
    buffer_species::install_buffer_species(value);
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

/// `Buffer.prototype`'s methods and its `parent` / `offset` accessors, in
/// declaration order after `constructor`. Reached only through the
/// typed-array install's slot (`typedarray_hook_slots::buffer_prototype_members`),
/// as the web groups' member tables are: `buffer_constructor_value` is reached
/// from the always-live `globalThis` builder and prototype walk, so naming the
/// table there kept every member body in every program.
pub(crate) fn install_buffer_prototype_members(proto: *mut ObjectHeader) {
    let scope = crate::gc::RuntimeHandleScope::new();
    let proto = scope.root_raw_mut_ptr(proto);
    for method in BUFFER_PROTOTYPE_METHODS {
        // toString is a real member (its body decodes); the others
        // share the method thunk.
        let method_ptr = if *method == "toString" {
            crate::fn_info!(#[realm(Buffer.prototype.toString, constructible = true, effects = { coerce(arg(0), ToPrimitive); coerce(arg(1), ToPrimitive); coerce(arg(2), ToPrimitive); stores(fresh); returns(prim(str)) })] export native_args crate::object::native_module::callable_exports::buffer_prototype_to_string, 0)
        } else {
            crate::fn_info!(#[realm(realm_decl! { Buffer.prototype.asciiSlice, constructible = true; Buffer.prototype.asciiWrite, constructible = true; Buffer.prototype.base64Slice, constructible = true; Buffer.prototype.base64Write, constructible = true; Buffer.prototype.base64urlSlice, constructible = true; Buffer.prototype.base64urlWrite, constructible = true; Buffer.prototype.compare, constructible = true; Buffer.prototype.copy, constructible = true; Buffer.prototype.equals, constructible = true; Buffer.prototype.fill, constructible = true; Buffer.prototype.hexSlice, constructible = true; Buffer.prototype.hexWrite, constructible = true; Buffer.prototype.includes, constructible = true; Buffer.prototype.indexOf, constructible = true; Buffer.prototype.inspect, constructible = true; Buffer.prototype.lastIndexOf, constructible = true; Buffer.prototype.latin1Slice, constructible = true; Buffer.prototype.latin1Write, constructible = true; Buffer.prototype.readBigInt64BE, constructible = true; Buffer.prototype.readBigInt64LE, constructible = true; Buffer.prototype.readBigUInt64BE, constructible = true; Buffer.prototype.readBigUInt64LE, constructible = true; Buffer.prototype.readBigUint64BE, constructible = true; Buffer.prototype.readBigUint64LE, constructible = true; Buffer.prototype.readDoubleBE, constructible = true; Buffer.prototype.readDoubleLE, constructible = true; Buffer.prototype.readFloatBE, constructible = true; Buffer.prototype.readFloatLE, constructible = true; Buffer.prototype.readInt16BE, constructible = true; Buffer.prototype.readInt16LE, constructible = true; Buffer.prototype.readInt32BE, constructible = true; Buffer.prototype.readInt32LE, constructible = true; Buffer.prototype.readInt8, constructible = true; Buffer.prototype.readIntBE, constructible = true; Buffer.prototype.readIntLE, constructible = true; Buffer.prototype.readUInt16BE, constructible = true; Buffer.prototype.readUInt16LE, constructible = true; Buffer.prototype.readUInt32BE, constructible = true; Buffer.prototype.readUInt32LE, constructible = true; Buffer.prototype.readUInt8, constructible = true; Buffer.prototype.readUIntBE, constructible = true; Buffer.prototype.readUIntLE, constructible = true; Buffer.prototype.readUint16BE, constructible = true; Buffer.prototype.readUint16LE, constructible = true; Buffer.prototype.readUint32BE, constructible = true; Buffer.prototype.readUint32LE, constructible = true; Buffer.prototype.readUint8, constructible = true; Buffer.prototype.readUintBE, constructible = true; Buffer.prototype.readUintLE, constructible = true; Buffer.prototype.slice, constructible = true; Buffer.prototype.subarray, constructible = true; Buffer.prototype.swap16, constructible = true; Buffer.prototype.swap32, constructible = true; Buffer.prototype.swap64, constructible = true; Buffer.prototype.toJSON, constructible = true; Buffer.prototype.toLocaleString, constructible = true; Buffer.prototype.ucs2Slice, constructible = true; Buffer.prototype.ucs2Write, constructible = true; Buffer.prototype.utf8Slice, constructible = true; Buffer.prototype.utf8Write, constructible = true; Buffer.prototype.write, constructible = true; Buffer.prototype.writeBigInt64BE, constructible = true; Buffer.prototype.writeBigInt64LE, constructible = true; Buffer.prototype.writeBigUInt64BE, constructible = true; Buffer.prototype.writeBigUInt64LE, constructible = true; Buffer.prototype.writeBigUint64BE, constructible = true; Buffer.prototype.writeBigUint64LE, constructible = true; Buffer.prototype.writeDoubleBE, constructible = true; Buffer.prototype.writeDoubleLE, constructible = true; Buffer.prototype.writeFloatBE, constructible = true; Buffer.prototype.writeFloatLE, constructible = true; Buffer.prototype.writeInt16BE, constructible = true; Buffer.prototype.writeInt16LE, constructible = true; Buffer.prototype.writeInt32BE, constructible = true; Buffer.prototype.writeInt32LE, constructible = true; Buffer.prototype.writeInt8, constructible = true; Buffer.prototype.writeIntBE, constructible = true; Buffer.prototype.writeIntLE, constructible = true; Buffer.prototype.writeUInt16BE, constructible = true; Buffer.prototype.writeUInt16LE, constructible = true; Buffer.prototype.writeUInt32BE, constructible = true; Buffer.prototype.writeUInt32LE, constructible = true; Buffer.prototype.writeUInt8, constructible = true; Buffer.prototype.writeUIntBE, constructible = true; Buffer.prototype.writeUIntLE, constructible = true; Buffer.prototype.writeUint16BE, constructible = true; Buffer.prototype.writeUint16LE, constructible = true; Buffer.prototype.writeUint32BE, constructible = true; Buffer.prototype.writeUint32LE, constructible = true; Buffer.prototype.writeUint8, constructible = true; Buffer.prototype.writeUintBE, constructible = true; Buffer.prototype.writeUintLE, constructible = true; })] export crate::object::native_module::callable_exports::buffer_prototype_method_thunk, 0)
        };
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
            crate::fn_info!(#[realm("Buffer.prototype.parent.<get>", constructible = true)] export crate::object::native_module::callable_exports::buffer_prototype_parent_getter_thunk, 0; with_declared(0)),
        );
        install_buffer_prototype_getter(
            proto,
            "offset",
            crate::fn_info!(#[realm("Buffer.prototype.offset.<get>", constructible = true)] export crate::object::native_module::callable_exports::buffer_prototype_offset_getter_thunk, 0; with_declared(0)),
        );
    });
}
