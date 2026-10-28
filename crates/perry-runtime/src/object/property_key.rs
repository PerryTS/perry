//! ECMAScript `ToPropertyKey` conversion and the object-literal computed
//! property / `super` helpers built on top of it. Split out of
//! `object_ops.rs` to keep that module under the file-size gate.

use super::*;

/// ECMAScript `ToPropertyKey` for computed member definitions.
///
/// Symbols are valid property keys and must be preserved. Every other value
/// first takes the string-hint ToPrimitive path, then stringifies with Perry's
/// JS string conversion so numeric keys use JS spelling rather than Rust's
/// default formatting.
#[no_mangle]
pub unsafe extern "C" fn js_to_property_key(value: f64) -> f64 {
    if crate::symbol::js_is_symbol(value) != 0 {
        return value;
    }
    let primitive = crate::symbol::js_to_primitive(value, 2);
    if crate::symbol::js_is_symbol(primitive) != 0 {
        return primitive;
    }
    // `js_to_primitive` only consults a user `@@toPrimitive` method; when none
    // is present it returns the object unchanged. Complete `ToPrimitive` here
    // via `OrdinaryToPrimitive(value, "string")` so that a `toString`/`valueOf`
    // returning a Symbol yields that Symbol as the property key rather than
    // being stringified by `js_jsvalue_to_string` below (test262
    // hasOwnProperty/propertyIsEnumerable `symbol_property_{toString,valueOf}`).
    if primitive.to_bits() == value.to_bits() {
        if let Some(p) = ordinary_to_primitive_string_key(primitive) {
            if crate::symbol::js_is_symbol(p) != 0 {
                return p;
            }
            let key = crate::value::js_jsvalue_to_string(p);
            if key.is_null() {
                return f64::from_bits(crate::value::TAG_UNDEFINED);
            }
            return crate::value::js_nanbox_string(key as i64);
        }
    }
    let key = crate::value::js_jsvalue_to_string(primitive);
    if key.is_null() {
        return f64::from_bits(crate::value::TAG_UNDEFINED);
    }
    crate::value::js_nanbox_string(key as i64)
}

/// True when [`js_to_property_key`] provably neither allocates nor calls back
/// into user JS for `key`, so a caller may hold a raw receiver / stored value
/// across it without a [`RuntimeHandleScope`] (#6935).
///
/// Only an already-heap `STRING_TAG` key qualifies. Walk the coercion for one:
/// `js_is_symbol` is a tag/side-table test, `js_to_primitive` returns any
/// non-`POINTER_TAG` value unchanged, `ordinary_to_primitive_string_key` bails
/// immediately (`extract_obj_ptr` is null for a tagged string), and
/// `js_jsvalue_to_string` hands the very same pointer back. Nothing allocates.
///
/// Every other key shape does: numbers / booleans / `null` / `undefined` /
/// BigInt allocate their stringification, SSO short strings materialize onto
/// the heap, and object keys can invoke a user `Symbol.toPrimitive` /
/// `toString` / `valueOf`. Any of those can trigger a GC that **evacuates**
/// live objects — moving the caller's receiver and the value it is about to
/// store — so the callers below must root across the coercion instead.
///
/// [`RuntimeHandleScope`]: crate::gc::RuntimeHandleScope
#[inline]
pub(crate) fn property_key_coercion_is_inert(key: f64) -> bool {
    (key.to_bits() & 0xFFFF_0000_0000_0000) == crate::value::STRING_TAG
}

/// `ToPropertyKey(key)` performed inside an existing `scope`, with both the
/// incoming key and the coerced result rooted (#6935).
///
/// Callers must root their receiver — and any value they are about to store —
/// in the SAME scope *before* calling this, then read those back through their
/// handles: this coercion is the GC-capable step that invalidates raw locals.
#[inline]
pub(crate) unsafe fn to_property_key_rooted<'scope>(
    scope: &'scope crate::gc::RuntimeHandleScope,
    key: f64,
) -> crate::gc::RuntimeHandle<'scope> {
    let key_handle = scope.root_nanbox_f64(key);
    let coerced = js_to_property_key(key_handle.get_nanbox_f64());
    scope.root_nanbox_f64(coerced)
}

/// True when `v` is not a JS Object — i.e. a usable primitive result from
/// `OrdinaryToPrimitive` (undefined/null/boolean/number/string/bigint and,
/// crucially, **Symbol**, the one `POINTER_TAG` primitive).
fn js_value_is_not_object(v: f64) -> bool {
    let bits = v.to_bits();
    if (bits & 0xFFFF_0000_0000_0000) != crate::value::POINTER_TAG {
        return true;
    }
    // A Symbol is the one POINTER_TAG primitive. Use `js_is_symbol` (not the
    // narrower registered-handle table) so a fresh heap `Symbol()` returned
    // from `toString`/`valueOf` is recognized and preserved as the key.
    unsafe { crate::symbol::js_is_symbol(v) != 0 }
}

/// `OrdinaryToPrimitive(O, "string")` with Symbol preservation. The spec
/// fallback (when no `@@toPrimitive` exists) invokes `toString` then `valueOf`
/// and uses the first result whose type is not Object — and a Symbol is not an
/// Object, so it is returned verbatim instead of being coerced to a string.
/// Returns `None` for a non-object receiver, when neither method is a callable
/// closure, or when both yield Objects, so the caller falls back to its
/// ordinary string coercion (preserving prior behavior for those cases — most
/// notably plain objects whose only `toString` is the native
/// `Object.prototype.toString`, which is not stored as a closure field here).
unsafe fn ordinary_to_primitive_string_key(value: f64) -> Option<f64> {
    if extract_obj_ptr(value).is_null() {
        return None;
    }
    let scope = crate::gc::RuntimeHandleScope::new();
    let value_handle = scope.root_nanbox_f64(value);
    for name in [b"toString".as_slice(), b"valueOf".as_slice()] {
        let receiver = value_handle.get_nanbox_f64();
        let recv_ptr = extract_obj_ptr(receiver);
        if recv_ptr.is_null() {
            return None;
        }
        let key = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
        let method = js_object_get_field_by_name(recv_ptr as *const ObjectHeader, key);
        let method_bits = method.bits();
        if (method_bits & 0xFFFF_0000_0000_0000) != crate::value::POINTER_TAG {
            continue;
        }
        let method_ptr = (method_bits & crate::value::POINTER_MASK) as usize;
        if !crate::closure::is_closure_ptr(method_ptr) {
            continue;
        }
        let bound = crate::closure::clone_closure_rebind_this(method_bits, receiver);
        // Re-read: the key allocation, the method read and the clone can all
        // move the receiver.
        let result = crate::closure::native_call_value_this(
            f64::from_bits(bound),
            crate::closure::JsThis::from_f64(value_handle.get_nanbox_f64()),
            std::ptr::null(),
            0,
        );
        if js_value_is_not_object(result) {
            return Some(result);
        }
    }
    None
}

/// `obj[ToPropertyKey(key)] = value` for object-literal computed definitions.
///
/// #6935: `ToPropertyKey` can run a user `Symbol.toPrimitive` / `toString` /
/// `valueOf`, which allocates → GC → **evacuation**. Pre-fix `obj_value` (the
/// receiver) and `value` (what is about to be written *into* it) were raw
/// NaN-boxed Rust locals held across that call — neither a GC root nor a
/// shadow slot. A stale receiver dropped the write onto a forwarding stub, and
/// a stale `value` planted a dangling pointer inside a live object, so the
/// corruption outlived the call. Root both before the coercion and read them
/// back through their handles afterwards.
#[no_mangle]
pub unsafe extern "C" fn js_object_set_property_key(
    obj_value: f64,
    key_value: f64,
    value: f64,
) -> f64 {
    if property_key_coercion_is_inert(key_value) {
        return set_property_key_resolved(obj_value, key_value, value);
    }
    let scope = crate::gc::RuntimeHandleScope::new();
    // `obj_value` may arrive NaN-boxed OR as a bare heap address (module-level
    // object slots store the untagged pointer) OR as an INT32 class-ref — the
    // heap-word slot kind covers all three, rewriting only the real pointers
    // and preserving each encoding.
    let obj_handle = scope.root_nanbox_u64(obj_value.to_bits());
    let value_handle = scope.root_nanbox_f64(value);
    let key_handle = to_property_key_rooted(&scope, key_value);
    set_property_key_resolved(
        f64::from_bits(obj_handle.get_nanbox_u64()),
        key_handle.get_nanbox_f64(),
        value_handle.get_nanbox_f64(),
    )
}

/// Post-`ToPropertyKey` half of [`js_object_set_property_key`]. `key` is
/// already a Symbol or a heap string here, so nothing below can run user JS.
#[inline]
unsafe fn set_property_key_resolved(obj_value: f64, key: f64, value: f64) -> f64 {
    if crate::symbol::js_is_symbol(key) != 0 {
        return crate::symbol::js_object_set_symbol_property(obj_value, key, value);
    }
    let key_str = crate::value::js_jsvalue_to_string(key);
    if key_str.is_null() {
        return value;
    }
    // Class constructor/prototype refs are INT32-tagged values, not real
    // `ObjectHeader`s — `extract_obj_ptr` returns null for them, so a
    // `C.prototype[key] = v` / `C[key] = v` write silently no-op'd here. The
    // get side already passes the raw NaN-boxed bits into the by-name dispatch
    // (which has a dedicated 0x7FFE class-ref branch); mirror that on the set
    // side so static-accessor and prototype instance-setter dispatch run.
    if let Some(class_id) = super::class_ref_id(obj_value) {
        if super::class_prototype_ref_id(obj_value).is_none() {
            let name_ptr = crate::string::string_data(key_str);
            let name_len = (*key_str).byte_len as usize;
            if std::slice::from_raw_parts(name_ptr, name_len) == b"prototype" {
                crate::error::throw_immutable_write(class_id, "prototype");
            }
        }
        js_object_set_field_by_name(obj_value.to_bits() as *mut ObjectHeader, key_str, value);
        return value;
    }
    let obj = extract_obj_ptr(obj_value);
    if !obj.is_null() {
        // §3.3: a key the layout does not name is appended to the overflow.
        let key_value = crate::value::js_nanbox_string(key_str as i64);
        if let Some(stored) = super::shape_store::try_dynamic_store(obj, key_value, value) {
            return stored;
        }
        js_object_set_field_by_name(obj, key_str, value);
    }
    value
}

/// `obj[ToPropertyKey(key)]` using Perry's string and symbol property stores.
///
/// #6935: the receiver is rooted across the GC-capable key coercion — see
/// [`js_object_set_property_key`] for the full reasoning.
#[no_mangle]
pub unsafe extern "C" fn js_object_get_property_key(obj_value: f64, key_value: f64) -> f64 {
    if property_key_coercion_is_inert(key_value) {
        return get_property_key_resolved(obj_value, key_value);
    }
    let scope = crate::gc::RuntimeHandleScope::new();
    let obj_handle = scope.root_nanbox_u64(obj_value.to_bits());
    let key_handle = to_property_key_rooted(&scope, key_value);
    get_property_key_resolved(
        f64::from_bits(obj_handle.get_nanbox_u64()),
        key_handle.get_nanbox_f64(),
    )
}

/// Post-`ToPropertyKey` half of [`js_object_get_property_key`].
#[inline]
unsafe fn get_property_key_resolved(obj_value: f64, key: f64) -> f64 {
    if crate::symbol::js_is_symbol(key) != 0 {
        return crate::symbol::js_object_get_symbol_property(obj_value, key);
    }
    let key_str = crate::value::js_jsvalue_to_string(key);
    if key_str.is_null() {
        return f64::from_bits(crate::value::TAG_UNDEFINED);
    }
    // Class constructor/prototype refs are INT32-tagged, not real
    // `ObjectHeader`s — pass their raw bits into the by-name dispatch (which has
    // a dedicated class-ref branch handling static accessors, static methods,
    // prototype methods, etc.) rather than null'ing them via extract_obj_ptr.
    // Mirrors the set side (`js_object_set_property_key`).
    if super::class_ref_id(obj_value).is_some() {
        return f64::from_bits(
            js_object_get_field_by_name(obj_value.to_bits() as *const ObjectHeader, key_str).bits(),
        );
    }
    // Native handles expose properties through the by-name dispatcher too.
    // extract_obj_ptr deliberately rejects them to protect heap-only callers;
    // applying that guard here silently lost Request.signal (and any dictionary
    // members supplied by a native Request used as init).
    let receiver = crate::value::JSValue::from_bits(obj_value.to_bits());
    if receiver.is_handle() {
        // The by-name read admits a raw id: Proxy ids reach their [[Get]],
        // every other id its native dispatcher.
        let handle = receiver.as_handle() as *const ObjectHeader;
        return f64::from_bits(js_object_get_field_by_name(handle, key_str).bits());
    }
    let obj = extract_obj_ptr(obj_value);
    if obj.is_null() {
        return f64::from_bits(crate::value::TAG_UNDEFINED);
    }
    f64::from_bits(js_object_get_field_by_name(obj as *const ObjectHeader, key_str).bits())
}

/// Install an object-literal method under a computed property key and bind the
/// method's reserved `this` capture slot to the home object.
///
/// #6935: the home object AND the closure being installed are both rooted
/// across the GC-capable key coercion — the closure is the "stored value" here,
/// so a stale one would be written into the object.
#[no_mangle]
pub unsafe extern "C" fn js_object_set_property_key_method(
    obj_value: f64,
    key_value: f64,
    closure: f64,
) -> f64 {
    if property_key_coercion_is_inert(key_value) {
        return set_property_key_method_resolved(obj_value, key_value, closure);
    }
    let scope = crate::gc::RuntimeHandleScope::new();
    let obj_handle = scope.root_nanbox_u64(obj_value.to_bits());
    let closure_handle = scope.root_nanbox_f64(closure);
    let key_handle = to_property_key_rooted(&scope, key_value);
    set_property_key_method_resolved(
        f64::from_bits(obj_handle.get_nanbox_u64()),
        key_handle.get_nanbox_f64(),
        closure_handle.get_nanbox_f64(),
    )
}

/// Post-`ToPropertyKey` half of [`js_object_set_property_key_method`].
#[inline]
unsafe fn set_property_key_method_resolved(obj_value: f64, key: f64, closure: f64) -> f64 {
    if crate::symbol::js_is_symbol(key) != 0 {
        return crate::symbol::js_object_set_symbol_method(obj_value, key, closure);
    }
    crate::symbol::js_object_set_method_by_name(obj_value, key, closure)
}

fn object_ptr_from_value(value: f64) -> usize {
    let bits = value.to_bits();
    let top = bits >> 48;
    if top == 0x7FFD {
        (bits & crate::value::POINTER_MASK) as usize
    } else if top == 0 && bits > 0x10000 {
        bits as usize
    } else {
        0
    }
}

unsafe fn object_super_prototype_value(home: f64) -> Option<f64> {
    let home_ptr = object_ptr_from_value(home);
    if home_ptr == 0 {
        return None;
    }
    let proto_bits = super::prototype_chain::object_static_prototype(home_ptr)?;
    if proto_bits == crate::value::TAG_NULL {
        return None;
    }
    Some(f64::from_bits(proto_bits))
}

/// Resolve `super[key]` for object-literal methods using the method's captured
/// home object and original receiver. The actual prototype is read at call
/// time so `Object.setPrototypeOf(home, proto)` after literal creation is
/// observed.
#[no_mangle]
pub unsafe extern "C" fn js_object_super_get(home: f64, key_value: f64, receiver: f64) -> f64 {
    // ToPropertyKey can call user code and collect. Keep the lexical home
    // object and dynamic receiver live while it runs, then look up the home
    // object's current prototype and preserve the original receiver through
    // the inherited getter or Proxy get trap.
    let scope = crate::gc::RuntimeHandleScope::new();
    let home_handle = scope.root_nanbox_u64(home.to_bits());
    let receiver_handle = scope.root_nanbox_u64(receiver.to_bits());
    let key_input = scope.root_nanbox_f64(key_value);
    let key_handle = scope.root_nanbox_f64(js_to_property_key(key_input.get_nanbox_f64()));
    let base =
        crate::object::js_object_get_prototype_of(f64::from_bits(home_handle.get_nanbox_u64()));
    let base_handle = scope.root_nanbox_f64(base);
    let base_value = crate::value::JSValue::from_bits(base.to_bits());
    if base_value.is_null() || base_value.is_undefined() {
        let key_value = key_handle.get_nanbox_f64();
        let (name_ptr, name_len) = if crate::symbol::js_is_symbol(key_value) != 0 {
            (std::ptr::null(), 0)
        } else {
            let name = crate::builtins::js_string_coerce(key_value);
            if name.is_null() {
                (std::ptr::null(), 0)
            } else {
                let data = crate::string::string_data(name);
                (data, (*name).byte_len as usize)
            }
        };
        crate::error::js_throw_type_error_property_access(
            base_value.is_null() as u32,
            name_ptr,
            name_len,
        );
    }
    crate::proxy::js_reflect_get_with_explicit_receiver(
        base_handle.get_nanbox_f64(),
        key_handle.get_nanbox_f64(),
        f64::from_bits(receiver_handle.get_nanbox_u64()),
    )
}

/// `super.prop` GET for class methods whose home object belongs to class
/// `home_class_id`: `home.[[GetPrototypeOf]]().[[Get]](key, receiver)`.
///
/// Instance reads follow the home prototype's actual parent edge with the
/// original receiver, for both compiled and native parents. Static reads use
/// the parent's constructor properties, including any relinked static chain.
#[no_mangle]
pub unsafe extern "C" fn js_super_accessor_get(home_class_id: u32, key: f64, receiver: f64) -> f64 {
    // ToPropertyKey can call user code and allocate. Resolve the home edge
    // afterward and keep the key and original receiver live through both.
    let scope = crate::gc::RuntimeHandleScope::new();
    let receiver_handle = scope.root_nanbox_u64(receiver.to_bits());
    let key_handle = scope.root_nanbox_f64(js_to_property_key(key));
    let home = super::class_registry::class_decl_prototype_value(home_class_id);
    let base = super::js_object_get_prototype_of(home);
    let base_value = crate::value::JSValue::from_bits(base.to_bits());
    if base_value.is_null() || base_value.is_undefined() {
        let key_hdr = crate::builtins::js_string_coerce(key_handle.get_nanbox_f64());
        let name = if key_hdr.is_null() {
            &[][..]
        } else {
            std::slice::from_raw_parts(
                (key_hdr as *const u8).add(std::mem::size_of::<crate::StringHeader>()),
                (*key_hdr).byte_len as usize,
            )
        };
        crate::error::js_throw_type_error_property_access(
            base_value.is_null() as u32,
            name.as_ptr(),
            name.len(),
        );
    }
    crate::proxy::js_reflect_get(
        base,
        key_handle.get_nanbox_f64(),
        f64::from_bits(receiver_handle.get_nanbox_u64()),
    )
}

/// `super[key] = value` for object-literal methods using the captured home
/// object. The prototype is resolved before the RHS has already been evaluated
/// by codegen; this helper performs the final ordinary [[Set]].
#[no_mangle]
pub unsafe extern "C" fn js_object_super_put_value_set(
    home: f64,
    key_value: f64,
    value: f64,
    receiver: f64,
    strict: i32,
) -> f64 {
    let Some(proto) = object_super_prototype_value(home) else {
        if strict != 0 {
            let key_name = crate::builtins::js_string_coerce(key_value);
            let name = if key_name.is_null() {
                "property".to_string()
            } else {
                let name_ptr =
                    (key_name as *const u8).add(std::mem::size_of::<crate::StringHeader>());
                let name_len = (*key_name).byte_len as usize;
                std::str::from_utf8(std::slice::from_raw_parts(name_ptr, name_len))
                    .unwrap_or("property")
                    .to_string()
            };
            crate::error::throw_immutable_write(0, &name);
        }
        return value;
    };
    crate::proxy::js_put_value_set(proto, key_value, value, receiver, strict)
}

/// Resolve and call `super[key](...)` for object-literal methods.
#[no_mangle]
pub unsafe extern "C" fn js_object_super_call(
    home: f64,
    key_value: f64,
    receiver: f64,
    args_ptr: *const f64,
    args_len: usize,
) -> f64 {
    // #6935: `js_object_super_get` performs the GC-capable `ToPropertyKey`, and
    // `clone_closure_rebind_this` allocates the bound copy. `receiver` — the
    // `this` the bound method runs with — was raw across both.
    let scope = crate::gc::RuntimeHandleScope::new();
    let receiver_handle = scope.root_nanbox_u64(receiver.to_bits());
    let callee = js_object_super_get(home, key_value, receiver);
    if callee.to_bits() == crate::value::TAG_UNDEFINED {
        return callee;
    }
    let callee_handle = scope.root_nanbox_f64(callee);
    let receiver = f64::from_bits(receiver_handle.get_nanbox_u64());
    let bound = crate::closure::clone_closure_rebind_this(callee_handle.get_nanbox_u64(), receiver);
    let bound_handle = scope.root_nanbox_u64(bound);
    let receiver = f64::from_bits(receiver_handle.get_nanbox_u64());
    crate::closure::native_call_value_this(
        f64::from_bits(bound_handle.get_nanbox_u64()),
        crate::closure::JsThis::from_f64(receiver),
        args_ptr,
        args_len,
    )
}

#[cfg(test)]
mod property_key_tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    per_test_global! {
        static SUPER_GET_KEY_CONVERSIONS: AtomicUsize = AtomicUsize::new(0);
    }

    extern "C" fn accessor_getter(
        _closure: *const crate::closure::ClosureHeader,
        _this: crate::closure::JsThis,
    ) -> f64 {
        123.0
    }

    extern "C" fn computed_class_method(_this_arg: f64) -> f64 {
        77.0
    }

    extern "C" fn computed_class_getter(_this_arg: f64) -> f64 {
        88.0
    }

    fn string_value_to_rust(value: f64) -> String {
        unsafe {
            let ptr =
                crate::value::js_get_string_pointer_unified(value) as *const crate::StringHeader;
            assert!(!ptr.is_null(), "expected string value");
            let len = (*ptr).byte_len as usize;
            let data = (ptr as *const u8).add(std::mem::size_of::<crate::StringHeader>());
            String::from_utf8(std::slice::from_raw_parts(data, len).to_vec()).unwrap()
        }
    }

    #[test]
    fn property_key_preserves_symbols_and_stringifies_primitives() {
        unsafe {
            let sym = crate::symbol::js_symbol_new_empty();
            assert_eq!(js_to_property_key(sym).to_bits(), sym.to_bits());

            assert_eq!(string_value_to_rust(js_to_property_key(42.0)), "42");
            let int_key = f64::from_bits(crate::value::JSValue::int32(7).bits());
            assert_eq!(string_value_to_rust(js_to_property_key(int_key)), "7");
            assert_eq!(
                string_value_to_rust(js_to_property_key(f64::from_bits(crate::value::TAG_TRUE))),
                "true"
            );
        }
    }

    #[test]
    fn property_key_object_helpers_use_canonical_key_conversion() {
        unsafe {
            let obj = js_object_alloc(0, 0);
            let obj_value = crate::value::js_nanbox_pointer(obj as i64);

            js_object_set_property_key(obj_value, 7.0, 19.0);
            assert_eq!(js_object_get_property_key(obj_value, 7.0), 19.0);

            let key = crate::string::js_string_from_bytes(b"7".as_ptr(), 1);
            let field = js_object_get_field_by_name(obj, key);
            assert_eq!(field.as_number(), 19.0);
        }
    }

    #[test]
    fn property_key_symbol_accessors_route_through_symbol_storage() {
        unsafe {
            let obj = js_object_alloc(0, 0);
            let obj_value = crate::value::js_nanbox_pointer(obj as i64);
            let sym = crate::symbol::js_symbol_new_empty();
            let getter = crate::closure::js_closure_alloc(crate::fn_info!(accessor_getter, 0), 0);
            let getter_value = crate::value::js_nanbox_pointer(getter as i64);

            js_object_define_accessor(
                obj_value,
                sym,
                getter_value,
                f64::from_bits(crate::value::TAG_UNDEFINED),
            );

            assert_eq!(
                crate::symbol::js_object_get_symbol_property(obj_value, sym),
                123.0
            );
            assert_eq!(js_object_get_property_key(obj_value, sym), 123.0);
        }
    }

    extern "C" fn receiver_tag_getter(
        _closure: *const crate::closure::ClosureHeader,
        this: crate::closure::JsThis,
    ) -> f64 {
        let receiver =
            crate::value::JSValue::from_bits(this.as_f64().to_bits()).as_pointer::<ObjectHeader>();
        let tag = crate::string::canonical_key(b"tag");
        f64::from_bits(js_object_get_field_by_name(receiver, tag).bits())
    }

    extern "C" fn proxy_get_returns_receiver(
        _closure: *const crate::closure::ClosureHeader,
        _this: crate::closure::JsThis,
        _target: f64,
        _key: f64,
        receiver: f64,
    ) -> f64 {
        receiver
    }

    extern "C" fn receiver_value_getter(
        _closure: *const crate::closure::ClosureHeader,
        this: crate::closure::JsThis,
    ) -> f64 {
        this.as_f64()
    }

    extern "C" fn converting_key_once(
        _closure: *const crate::closure::ClosureHeader,
        _this: crate::closure::JsThis,
        _hint: f64,
    ) -> f64 {
        SUPER_GET_KEY_CONVERSIONS.fetch_add(1, Ordering::Relaxed);
        string_value("value")
    }

    fn string_value(text: &str) -> f64 {
        let string = crate::string::js_string_from_bytes(text.as_ptr(), text.len() as u32);
        crate::value::js_nanbox_string(string as i64)
    }

    fn define_receiver_tag_getter(base: f64, key: f64) {
        let getter = crate::closure::js_closure_alloc(crate::fn_info!(receiver_tag_getter, 0), 0);
        crate::object::js_object_define_accessor(
            base,
            key,
            crate::value::js_nanbox_pointer(getter as i64),
            f64::from_bits(crate::value::TAG_UNDEFINED),
        );
    }

    #[test]
    fn object_super_get_uses_original_receiver_and_live_home_prototype() {
        let _global = crate::gc::global_side_table_test_lock();
        let _no_gc = crate::gc::GcSuppressScope::new();
        unsafe {
            let scope = crate::gc::RuntimeHandleScope::new();
            let home = scope.root_nanbox_f64(crate::value::js_nanbox_pointer(
                crate::object::js_object_alloc(0, 0) as i64,
            ));
            let base = scope.root_nanbox_f64(crate::value::js_nanbox_pointer(
                crate::object::js_object_alloc(0, 0) as i64,
            ));
            let receiver = scope.root_nanbox_f64(crate::value::js_nanbox_pointer(
                crate::object::js_object_alloc(0, 0) as i64,
            ));
            let borrowed = scope.root_nanbox_f64(crate::value::js_nanbox_pointer(
                crate::object::js_object_alloc(0, 0) as i64,
            ));
            let key = scope.root_nanbox_f64(string_value("value"));
            let tag = crate::string::canonical_key(b"tag");
            crate::object::js_object_set_field_by_name(
                crate::value::JSValue::from_bits(receiver.get_nanbox_f64().to_bits())
                    .as_pointer::<ObjectHeader>() as *mut ObjectHeader,
                tag,
                string_value("original"),
            );
            crate::object::js_object_set_field_by_name(
                crate::value::JSValue::from_bits(borrowed.get_nanbox_f64().to_bits())
                    .as_pointer::<ObjectHeader>() as *mut ObjectHeader,
                tag,
                string_value("borrowed"),
            );
            define_receiver_tag_getter(base.get_nanbox_f64(), key.get_nanbox_f64());
            crate::object::js_object_set_prototype_of(home.get_nanbox_f64(), base.get_nanbox_f64());

            assert_eq!(
                string_value_to_rust(js_object_super_get(
                    home.get_nanbox_f64(),
                    key.get_nanbox_f64(),
                    receiver.get_nanbox_f64(),
                )),
                "original"
            );
            assert_eq!(
                string_value_to_rust(js_object_super_get(
                    home.get_nanbox_f64(),
                    key.get_nanbox_f64(),
                    borrowed.get_nanbox_f64(),
                )),
                "borrowed",
                "a borrowed method must keep its call-site receiver"
            );

            let relinked = scope.root_nanbox_f64(crate::value::js_nanbox_pointer(
                crate::object::js_object_alloc(0, 0) as i64,
            ));
            let relinked_key = crate::string::canonical_key(b"value");
            crate::object::js_object_set_field_by_name(
                crate::value::JSValue::from_bits(relinked.get_nanbox_f64().to_bits())
                    .as_pointer::<ObjectHeader>() as *mut ObjectHeader,
                relinked_key,
                string_value("relinked"),
            );
            crate::object::js_object_set_prototype_of(
                home.get_nanbox_f64(),
                relinked.get_nanbox_f64(),
            );
            assert_eq!(
                string_value_to_rust(js_object_super_get(
                    home.get_nanbox_f64(),
                    key.get_nanbox_f64(),
                    receiver.get_nanbox_f64(),
                )),
                "relinked",
                "super must read the home object's current prototype"
            );

            let default_home = scope.root_nanbox_f64(crate::value::js_nanbox_pointer(
                crate::object::js_object_alloc(0, 0) as i64,
            ));
            let constructor_key = string_value("constructor");
            let constructor = js_object_super_get(
                default_home.get_nanbox_f64(),
                constructor_key,
                receiver.get_nanbox_f64(),
            );
            assert_ne!(
                constructor.to_bits(),
                crate::value::TAG_UNDEFINED,
                "ordinary object literals inherit Object.prototype by default"
            );
        }
    }

    #[test]
    fn object_super_get_preserves_symbol_proxy_and_key_coercion_semantics() {
        let _global = crate::gc::global_side_table_test_lock();
        let _no_gc = crate::gc::GcSuppressScope::new();
        SUPER_GET_KEY_CONVERSIONS.store(0, Ordering::Relaxed);
        unsafe {
            let scope = crate::gc::RuntimeHandleScope::new();
            let home = scope.root_nanbox_f64(crate::value::js_nanbox_pointer(
                crate::object::js_object_alloc(0, 0) as i64,
            ));
            let receiver = scope.root_nanbox_f64(crate::value::js_nanbox_pointer(
                crate::object::js_object_alloc(0, 0) as i64,
            ));
            let base = scope.root_nanbox_f64(crate::value::js_nanbox_pointer(
                crate::object::js_object_alloc(0, 0) as i64,
            ));

            let tag = crate::string::canonical_key(b"tag");
            crate::object::js_object_set_field_by_name(
                crate::value::JSValue::from_bits(receiver.get_nanbox_f64().to_bits())
                    .as_pointer::<ObjectHeader>() as *mut ObjectHeader,
                tag,
                string_value("symbol receiver"),
            );
            let symbol = crate::symbol::js_symbol_new_empty();
            define_receiver_tag_getter(base.get_nanbox_f64(), symbol);
            crate::object::js_object_set_prototype_of(home.get_nanbox_f64(), base.get_nanbox_f64());
            assert_eq!(
                string_value_to_rust(js_object_super_get(
                    home.get_nanbox_f64(),
                    symbol,
                    receiver.get_nanbox_f64(),
                )),
                "symbol receiver",
                "a Symbol accessor receives the original super receiver"
            );

            let target = scope.root_nanbox_f64(crate::value::js_nanbox_pointer(
                crate::object::js_object_alloc(0, 0) as i64,
            ));
            let handler = scope.root_nanbox_f64(crate::value::js_nanbox_pointer(
                crate::object::js_object_alloc(0, 0) as i64,
            ));
            let trap = crate::closure::js_closure_alloc(
                crate::fn_info!(proxy_get_returns_receiver, 3; with_declared(3)),
                0,
            );
            crate::object::js_object_set_field_by_name(
                crate::value::JSValue::from_bits(handler.get_nanbox_f64().to_bits())
                    .as_pointer::<ObjectHeader>() as *mut ObjectHeader,
                crate::string::canonical_key(b"get"),
                crate::value::js_nanbox_pointer(trap as i64),
            );
            let proxy = scope.root_nanbox_f64(crate::proxy_hooks::proxy_new(
                target.get_nanbox_f64(),
                handler.get_nanbox_f64(),
            ));
            crate::object::js_object_set_prototype_of(
                home.get_nanbox_f64(),
                proxy.get_nanbox_f64(),
            );
            let proxy_result = js_object_super_get(
                home.get_nanbox_f64(),
                string_value("probe"),
                receiver.get_nanbox_f64(),
            );
            assert_eq!(
                proxy_result.to_bits(),
                receiver.get_nanbox_f64().to_bits(),
                "the Proxy get trap receives the original super receiver"
            );

            let converter_object = scope.root_nanbox_f64(crate::value::js_nanbox_pointer(
                crate::object::js_object_alloc(0, 0) as i64,
            ));
            let converter = crate::closure::js_closure_alloc(
                crate::fn_info!(converting_key_once, 1; with_declared(1)),
                0,
            );
            let to_primitive = crate::symbol::well_known_symbol("toPrimitive");
            crate::symbol::js_object_set_symbol_property(
                converter_object.get_nanbox_f64(),
                crate::value::js_nanbox_pointer(to_primitive as i64),
                crate::value::js_nanbox_pointer(converter as i64),
            );
            define_receiver_tag_getter(base.get_nanbox_f64(), string_value("value"));
            crate::object::js_object_set_prototype_of(home.get_nanbox_f64(), base.get_nanbox_f64());
            let converted_result = js_object_super_get(
                home.get_nanbox_f64(),
                converter_object.get_nanbox_f64(),
                receiver.get_nanbox_f64(),
            );
            assert_eq!(
                SUPER_GET_KEY_CONVERSIONS.load(Ordering::Relaxed),
                1,
                "computed super converts an object key once"
            );
            assert_eq!(string_value_to_rust(converted_result), "symbol receiver");
        }
    }

    #[test]
    fn object_super_get_with_null_home_prototype_throws_type_error() {
        let _global = crate::gc::global_side_table_test_lock();
        let _no_gc = crate::gc::GcSuppressScope::new();
        unsafe {
            let scope = crate::gc::RuntimeHandleScope::new();
            let home = scope.root_nanbox_f64(crate::value::js_nanbox_pointer(
                crate::object::js_object_alloc(0, 0) as i64,
            ));
            crate::object::js_object_set_prototype_of(
                home.get_nanbox_f64(),
                f64::from_bits(crate::value::TAG_NULL),
            );
            let thrown = crate::exception::catch_js_throw(|| {
                js_object_super_get(
                    home.get_nanbox_f64(),
                    string_value("missing-property-name-long-enough-to-check-string-data"),
                    f64::from_bits(crate::value::TAG_UNDEFINED),
                )
            })
            .expect_err("a null home prototype must throw");
            let error = crate::value::JSValue::from_bits(thrown.to_bits())
                .as_pointer::<crate::error::ErrorHeader>()
                as *mut crate::error::ErrorHeader;
            let message = crate::error::js_error_to_string(error);
            assert_eq!(
                string_value_to_rust(crate::value::js_nanbox_string(message as i64)),
                "TypeError: Cannot read properties of null (reading 'missing-property-name-long-enough-to-check-string-data')",
                "the null super base throws with the complete property name"
            );
        }
    }

    #[test]
    fn object_super_get_preserves_undefined_receiver() {
        let _global = crate::gc::global_side_table_test_lock();
        let _no_gc = crate::gc::GcSuppressScope::new();
        unsafe {
            let scope = crate::gc::RuntimeHandleScope::new();
            let home = scope.root_nanbox_f64(crate::value::js_nanbox_pointer(
                crate::object::js_object_alloc(0, 0) as i64,
            ));
            let base = scope.root_nanbox_f64(crate::value::js_nanbox_pointer(
                crate::object::js_object_alloc(0, 0) as i64,
            ));
            let key = string_value("value");
            let getter =
                crate::closure::js_closure_alloc(crate::fn_info!(receiver_value_getter, 0), 0);
            crate::object::js_object_define_accessor(
                base.get_nanbox_f64(),
                key,
                crate::value::js_nanbox_pointer(getter as i64),
                f64::from_bits(crate::value::TAG_UNDEFINED),
            );
            crate::object::js_object_set_prototype_of(home.get_nanbox_f64(), base.get_nanbox_f64());

            assert_eq!(
                js_object_super_get(
                    home.get_nanbox_f64(),
                    key,
                    f64::from_bits(crate::value::TAG_UNDEFINED),
                )
                .to_bits(),
                crate::value::TAG_UNDEFINED,
                "super preserves an explicitly undefined receiver"
            );
            assert_eq!(
                crate::proxy::js_reflect_get(
                    base.get_nanbox_f64(),
                    key,
                    f64::from_bits(crate::value::TAG_UNDEFINED),
                )
                .to_bits(),
                base.get_nanbox_f64().to_bits(),
                "public Reflect.get retains its target-default behavior"
            );
        }
    }

    #[test]
    fn property_key_class_computed_symbol_registration() {
        unsafe {
            crate::object::class_registry::test_clear_class_side_table_roots();

            let class_id = 0x3581;
            let sym = crate::symbol::js_symbol_new_empty();
            let sym_key = crate::symbol::sym_key_from_f64(sym);

            crate::object::class_registry::js_register_class_computed_method(
                class_id as i64,
                sym,
                computed_class_method as *const () as usize as i64,
                0,
                0,
                0,
                1,
                0,
                f64::from_bits(crate::value::TAG_UNDEFINED),
            );
            let method = crate::object::class_registry::lookup_class_symbol_method_in_chain(
                class_id, sym_key, false,
            )
            .expect("computed symbol method should be registered");
            assert_eq!(method.1, 0);
            assert!(!method.2);

            crate::object::class_registry::js_register_class_computed_accessor(
                class_id as i64,
                sym,
                computed_class_getter as *const () as usize as i64,
                0,
                0,
                2,
                f64::from_bits(crate::value::TAG_UNDEFINED),
            );
            let value = crate::object::class_registry::class_symbol_getter_value(
                class_id,
                sym_key,
                f64::from_bits(crate::value::TAG_UNDEFINED),
                false,
            )
            .expect("computed symbol getter should be registered");
            assert_eq!(value, 88.0);

            crate::object::class_registry::test_clear_class_side_table_roots();
        }
    }
}
