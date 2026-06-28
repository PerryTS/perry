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
        let prev_this = crate::object::js_implicit_this_set(receiver);
        let result =
            crate::closure::js_native_call_value(f64::from_bits(bound), std::ptr::null(), 0);
        crate::object::js_implicit_this_set(prev_this);
        if js_value_is_not_object(result) {
            return Some(result);
        }
    }
    None
}

/// `obj[ToPropertyKey(key)] = value` for object-literal computed definitions.
#[no_mangle]
pub unsafe extern "C" fn js_object_set_property_key(
    obj_value: f64,
    key_value: f64,
    value: f64,
) -> f64 {
    let key = js_to_property_key(key_value);
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
    if super::class_ref_id(obj_value).is_some() {
        js_object_set_field_by_name(obj_value.to_bits() as *mut ObjectHeader, key_str, value);
        return value;
    }
    let obj = extract_obj_ptr(obj_value);
    if !obj.is_null() {
        js_object_set_field_by_name(obj, key_str, value);
    }
    value
}

/// `obj[ToPropertyKey(key)]` using Perry's string and symbol property stores.
#[no_mangle]
pub unsafe extern "C" fn js_object_get_property_key(obj_value: f64, key_value: f64) -> f64 {
    let key = js_to_property_key(key_value);
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
    let obj = extract_obj_ptr(obj_value);
    if obj.is_null() {
        return f64::from_bits(crate::value::TAG_UNDEFINED);
    }
    f64::from_bits(js_object_get_field_by_name(obj as *const ObjectHeader, key_str).bits())
}

/// Install an object-literal method under a computed property key and bind the
/// method's reserved `this` capture slot to the home object.
#[no_mangle]
pub unsafe extern "C" fn js_object_set_property_key_method(
    obj_value: f64,
    key_value: f64,
    closure: f64,
) -> f64 {
    let key = js_to_property_key(key_value);
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
/// home object. The actual prototype is read at call time so
/// `Object.setPrototypeOf(home, proto)` after literal creation is observed.
#[no_mangle]
pub unsafe extern "C" fn js_object_super_get(home: f64, key_value: f64, _receiver: f64) -> f64 {
    let Some(proto) = object_super_prototype_value(home) else {
        return f64::from_bits(crate::value::TAG_UNDEFINED);
    };
    js_object_get_property_key(proto, key_value)
}

/// `super.prop` GET for class methods: walk the parent class chain from
/// `parent_class_id` for an accessor (getter) named `key` and invoke it with
/// `receiver` as `this` (lookup starts at the super prototype, but the getter
/// runs with the current `this`). If no getter is found, read a data property
/// off the parent prototype object (`B.prototype.x = 42` then `super.x`).
/// Refs class/super/in-{constructor,getter,methods,setter}.
#[no_mangle]
pub unsafe extern "C" fn js_super_accessor_get(
    parent_class_id: u32,
    key: f64,
    receiver: f64,
) -> f64 {
    let key_hdr = crate::builtins::js_string_coerce(key);
    let key_name: Option<String> = if key_hdr.is_null() {
        None
    } else {
        let p = (key_hdr as *const u8).add(std::mem::size_of::<crate::StringHeader>());
        let n = (*key_hdr).byte_len as usize;
        std::str::from_utf8(std::slice::from_raw_parts(p, n))
            .ok()
            .map(|s| s.to_string())
    };
    // Static-context super (`super.x` inside a `static` method/getter): the
    // receiver is the class constructor (a ClassRef), so resolve against the
    // PARENT's static side — a static getter, then a static data field —
    // rather than the parent prototype/instance vtable below. Refs
    // class/super/in-static-{getter,methods,setter}.
    if super::class_ref_id(receiver).is_some() {
        if let Some(key_name) = key_name.as_ref() {
            // (a) parent static getter, walking the class_id chain.
            if let Ok(guard) = crate::object::CLASS_STATIC_ACCESSORS.read() {
                if let Some(reg) = guard.as_ref() {
                    let mut cid = parent_class_id;
                    let mut depth = 0usize;
                    while cid != 0 && depth < 32 {
                        if let Some(getter_ptr) =
                            reg.get(&cid).and_then(|m| m.get(key_name)).map(|&(g, _)| g)
                        {
                            if getter_ptr != 0 {
                                let f: extern "C" fn(f64) -> f64 = std::mem::transmute(getter_ptr);
                                let prev = crate::object::js_implicit_this_set(receiver);
                                let r = f(receiver);
                                crate::object::js_implicit_this_set(prev);
                                return r;
                            }
                        }
                        match crate::object::get_parent_class_id(cid) {
                            Some(p) if p != 0 && p != cid => {
                                cid = p;
                                depth += 1;
                            }
                            _ => break,
                        }
                    }
                }
            }
            // (b) parent static data field (CLASS_DYNAMIC_PROPS), same walk.
            let mut cid = parent_class_id;
            let mut depth = 0usize;
            while cid != 0 && depth < 32 {
                if let Some(v) = crate::object::CLASS_DYNAMIC_PROPS
                    .with(|m| m.borrow().get(&cid).and_then(|f| f.get(key_name)).copied())
                {
                    return v;
                }
                match crate::object::get_parent_class_id(cid) {
                    Some(p) if p != 0 && p != cid => {
                        cid = p;
                        depth += 1;
                    }
                    _ => break,
                }
            }
        }
        return f64::from_bits(crate::value::TAG_UNDEFINED);
    }
    if let Some(key_name) = key_name {
        if let Ok(registry) = crate::object::CLASS_VTABLE_REGISTRY.read() {
            if let Some(reg) = registry.as_ref() {
                let mut cid = parent_class_id;
                let mut depth = 0usize;
                while cid != 0 && depth < 32 {
                    if let Some(vtable) = reg.get(&cid) {
                        let getter_alias = format!("__get_{}", key_name);
                        if let Some(&getter_ptr) = vtable
                            .getters
                            .get(&key_name)
                            .or_else(|| vtable.getters.get(&getter_alias))
                        {
                            let f: extern "C" fn(f64) -> f64 = std::mem::transmute(getter_ptr);
                            let prev = crate::object::js_implicit_this_set(receiver);
                            let r = f(receiver);
                            crate::object::js_implicit_this_set(prev);
                            return r;
                        }
                    }
                    match crate::object::get_parent_class_id(cid) {
                        Some(parent) if parent != 0 && parent != cid => {
                            cid = parent;
                            depth += 1;
                        }
                        _ => break,
                    }
                }
            }
        }
    }
    // Prefer the *declared* prototype object (stable heap identity). A dynamic
    // write `Parent.prototype.foo = v` lands on that object, whereas the older
    // overloaded `CLASS_PROTOTYPE_OBJECTS` table may hold a distinct synthetic
    // prototype that never sees such writes — so reading through it returned
    // `undefined` for data properties added to a parent prototype after the
    // class declaration (test262 super/prop-{dot,expr}-cls-val). Falls back to
    // the older table for synthetic-prototype sources that lack a decl entry.
    let mut proto = crate::object::class_decl_prototype_object(parent_class_id);
    if proto.is_null() {
        proto = crate::object::class_prototype_object(parent_class_id);
    }
    if !proto.is_null() {
        let target = crate::value::js_nanbox_pointer(proto as i64);
        return js_object_get_property_key(target, key);
    }
    f64::from_bits(crate::value::TAG_UNDEFINED)
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
    let callee = js_object_super_get(home, key_value, receiver);
    if callee.to_bits() == crate::value::TAG_UNDEFINED {
        return callee;
    }
    let bound = crate::closure::clone_closure_rebind_this(callee.to_bits(), receiver);
    let prev_this = crate::object::js_implicit_this_set(receiver);
    let result = crate::closure::js_native_call_value(f64::from_bits(bound), args_ptr, args_len);
    crate::object::js_implicit_this_set(prev_this);
    result
}

#[cfg(test)]
mod property_key_tests {
    use super::*;

    extern "C" fn accessor_getter(_closure: *const crate::closure::ClosureHeader) -> f64 {
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
            crate::symbol::test_clear_symbol_side_table_roots();

            let obj = js_object_alloc(0, 0);
            let obj_value = crate::value::js_nanbox_pointer(obj as i64);
            let sym = crate::symbol::js_symbol_new_empty();
            let getter = crate::closure::js_closure_alloc(accessor_getter as *const u8, 0);
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

            crate::symbol::test_clear_symbol_side_table_roots();
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
