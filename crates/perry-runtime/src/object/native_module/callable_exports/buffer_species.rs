//! `Buffer[Symbol.species]`: Node's internal `FastBuffer` (#11193).
//!
//! Node's `Buffer` carries an own `get [Symbol.species]` accessor returning
//! `FastBuffer`, the `Uint8Array` subclass every Buffer is really an instance
//! of. `Buffer.prototype` IS `FastBuffer.prototype`, and
//! `new FastBuffer(arrayBuffer, byteOffset, length)` makes a Buffer VIEW over
//! that memory (no copy), while `new FastBuffer(size)` is zero-filled like
//! `new Uint8Array(size)`.
//!
//! undici 8.9.0 captures it at module load
//! (`const FastBuffer = Buffer[Symbol.species]` in
//! `lib/dispatcher/client-h1.js`) and builds every llhttp callback argument
//! with `new FastBuffer(...)`, so with the accessor missing the first response
//! status line threw `undefined is not a constructor` inside the parser.
//!
//! The `FastBuffer` closure lives in the getter's capture slot, not a static:
//! captures are traced, and the symbol-accessor side table already roots the
//! getter, so no new runtime root holder is needed and the getter answers the
//! same object every time (`Buffer[Symbol.species] === Buffer[Symbol.species]`).

use super::*;

/// `new FastBuffer(...)` / `FastBuffer(...)`.
///
/// A numeric size allocates a ZERO-filled Buffer (Node's `FastBuffer(n)` is
/// `new Uint8Array(n)`; only `Buffer.allocUnsafe` skips the fill). Every other
/// shape (`(arrayBuffer, byteOffset?, length?)`, an array-like, a typed array)
/// is exactly what the `Buffer` constructor thunk already does, including the
/// memory-sharing `ArrayBuffer` view.
extern "C" fn fast_buffer_constructor_thunk(
    closure: *const crate::closure::ClosureHeader,
    value: f64,
    byte_offset: f64,
    length: f64,
) -> f64 {
    let value_js = crate::value::JSValue::from_bits(value.to_bits());
    if value_js.is_int32() || value_js.is_number() {
        let size = if value_js.is_int32() {
            value_js.as_int32()
        } else {
            value as i32
        };
        let buf = crate::buffer::js_buffer_alloc(size, 0);
        return crate::value::js_nanbox_pointer(buf as i64);
    }
    super::buffer_constructor_thunk(closure, value, byte_offset, length)
}

/// `get [Symbol.species]` on `Buffer`: answers the `FastBuffer` held in the
/// getter's own capture slot 0.
extern "C" fn buffer_species_getter_thunk(closure: *const crate::closure::ClosureHeader) -> f64 {
    crate::closure::js_closure_get_capture_f64(closure, 0)
}

/// Install `Buffer[Symbol.species]` on the freshly minted `Buffer`
/// constructor. `buffer_ctor` must be the constructor closure value, with its
/// `prototype` already installed (FastBuffer shares it).
pub(super) fn install_buffer_species(buffer_ctor: f64) {
    let species = crate::symbol::well_known_symbol("species");
    if species.is_null() {
        return;
    }
    let scope = crate::gc::RuntimeHandleScope::new();
    let buffer_ctor = scope.root_nanbox_f64(buffer_ctor);
    let buffer_proto = scope.root_nanbox_f64(crate::closure::closure_get_dynamic_prop(
        (buffer_ctor.get_nanbox_f64().to_bits() & crate::value::POINTER_MASK) as usize,
        "prototype",
    ));

    // FastBuffer itself: named, sharing Buffer.prototype. Node reports
    // `FastBuffer.length === 0`; the thunk still receives three arguments.
    let fast_ptr = fast_buffer_constructor_thunk as *const u8;
    crate::closure::js_register_closure_arity(fast_ptr, 3);
    let fast = crate::closure::js_closure_alloc(fast_ptr, 0);
    if fast.is_null() {
        return;
    }
    let fast = scope.root_raw_mut_ptr(fast);
    fast.with_mut_ptr::<crate::closure::ClosureHeader, _>(|ptr| {
        set_bound_native_closure_name(ptr, "FastBuffer");
        set_builtin_closure_length(ptr as usize, 0);
    });
    if crate::value::JSValue::from_bits(buffer_proto.get_nanbox_f64().to_bits()).is_pointer() {
        fast.with_mut_ptr(|ptr: *mut crate::closure::ClosureHeader| {
            crate::closure::closure_set_dynamic_prop(
                ptr as usize,
                "prototype",
                buffer_proto.get_nanbox_f64(),
            );
            super::super::set_builtin_property_attrs(
                ptr as usize,
                "prototype".to_string(),
                super::super::PropertyAttrs::new(false, false, false),
            );
        });
    }

    // The getter, holding FastBuffer in capture slot 0.
    let getter_ptr = buffer_species_getter_thunk as *const u8;
    crate::closure::js_register_closure_arity(getter_ptr, 0);
    let getter = crate::closure::js_closure_alloc(getter_ptr, 1);
    if getter.is_null() {
        return;
    }
    let getter = scope.root_raw_mut_ptr(getter);
    let fast_value = fast.with_mut_ptr(|ptr: *mut crate::closure::ClosureHeader| {
        crate::value::js_nanbox_pointer(ptr as i64)
    });
    getter.with_mut_ptr(|ptr: *mut crate::closure::ClosureHeader| {
        crate::closure::js_closure_set_capture_f64(ptr, 0, fast_value);
        set_bound_native_closure_name(ptr, "get [Symbol.species]");
    });

    // Own accessor on Buffer: `{ get, set: undefined, enumerable: false,
    // configurable: true }`, as Node's `ObjectDefineProperty(Buffer,
    // SymbolSpecies, { get() { return FastBuffer } })` plus its default flags.
    let getter_bits = getter.with_mut_ptr(|ptr: *mut crate::closure::ClosureHeader| {
        crate::value::js_nanbox_pointer(ptr as i64).to_bits()
    });
    let sym_value = f64::from_bits(crate::value::JSValue::pointer(species as *const u8).bits());
    unsafe {
        crate::symbol::set_symbol_accessor_property(
            buffer_ctor.get_nanbox_f64(),
            sym_value,
            getter_bits,
            0,
        );
        crate::symbol::set_symbol_property_attrs(
            (buffer_ctor.get_nanbox_f64().to_bits() & crate::value::POINTER_MASK) as usize,
            species as usize,
            super::super::PropertyAttrs::new(false, false, true),
        );
    }
}
