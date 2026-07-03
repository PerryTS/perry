//! `Object.is`, `Object.hasOwn`, and `Object.prototype.propertyIsEnumerable`.
use super::super::*;
use super::*;

/// Object.is(a, b) — SameValue algorithm
/// Like ===, except: NaN === NaN (true) and +0 !== -0 (false).
/// Returns NaN-boxed boolean.
#[no_mangle]
pub extern "C" fn js_object_is(a: f64, b: f64) -> f64 {
    const TAG_TRUE: u64 = 0x7FFC_0000_0000_0004;
    const TAG_FALSE: u64 = 0x7FFC_0000_0000_0003;
    let a_bits = a.to_bits();
    let b_bits = b.to_bits();

    // Handle NaN: SameValue treats NaN as equal to NaN
    let a_jsval = crate::JSValue::from_bits(a_bits);
    let b_jsval = crate::JSValue::from_bits(b_bits);

    if a_jsval.is_number() && b_jsval.is_number() {
        let an = a_jsval.as_number();
        let bn = b_jsval.as_number();
        if an.is_nan() && bn.is_nan() {
            return f64::from_bits(TAG_TRUE);
        }
        // Distinguish +0 / -0 by bit pattern
        if an == 0.0 && bn == 0.0 {
            if a_bits == b_bits {
                return f64::from_bits(TAG_TRUE);
            }
            return f64::from_bits(TAG_FALSE);
        }
        if an == bn {
            return f64::from_bits(TAG_TRUE);
        }
        return f64::from_bits(TAG_FALSE);
    }

    // For strings, do content comparison. #1781: accept inline SSO short
    // strings on either side. Two SSO operands with equal content already
    // match via the bit-pattern fallback below, but a mixed SSO/heap pair
    // (same content, different representation — e.g. a JSON-parsed value vs
    // a heap literal) would not. Materialize via the unified decoder so the
    // comparison is representation-independent.
    if a_jsval.is_any_string() && b_jsval.is_any_string() {
        let result = crate::string::js_string_equals(
            crate::value::js_get_string_pointer_unified(f64::from_bits(a_bits))
                as *const crate::StringHeader,
            crate::value::js_get_string_pointer_unified(f64::from_bits(b_bits))
                as *const crate::StringHeader,
        );
        if result != 0 {
            return f64::from_bits(TAG_TRUE);
        }
        return f64::from_bits(TAG_FALSE);
    }

    // For everything else, bit-pattern equality
    if a_bits == b_bits {
        f64::from_bits(TAG_TRUE)
    } else {
        f64::from_bits(TAG_FALSE)
    }
}

/// Object.hasOwn(obj, key) - check if obj has its own property `key`.
#[no_mangle]
pub extern "C" fn js_object_has_own(obj_value: f64, key_value: f64) -> f64 {
    const TAG_TRUE: u64 = 0x7FFC_0000_0000_0004;
    const TAG_FALSE: u64 = 0x7FFC_0000_0000_0003;
    unsafe {
        let obj_js = crate::JSValue::from_bits(obj_value.to_bits());
        if obj_js.is_undefined() || obj_js.is_null() {
            super::super::has_own_helpers::throw_to_object_nullish_type_error();
        }

        // ToPropertyKey(V): fold an object argument (e.g. one whose `toString`
        // returns a Symbol) into its canonical key before the symbol/string
        // split. A no-op for keys that are already primitives.
        let key_value = super::super::js_to_property_key(key_value);

        // A Proxy is a small registered id, not a heap object — route
        // `hasOwnProperty` through `[[GetOwnProperty]]` (a present own property
        // is one whose descriptor is not undefined) rather than dereferencing
        // the fake pointer. (Proxy crash cluster.)
        if crate::proxy::js_proxy_is_proxy(obj_value) != 0 {
            let desc = crate::proxy::js_reflect_get_own_property_descriptor(obj_value, key_value);
            return f64::from_bits(if desc.to_bits() != crate::value::TAG_UNDEFINED {
                TAG_TRUE
            } else {
                TAG_FALSE
            });
        }

        // Symbol-keyed lookup: route through SYMBOL_PROPERTIES side table.
        if crate::symbol::js_is_symbol(key_value) != 0 {
            // ClassRef receivers carry class_id in the low 32 bits.
            let bits = obj_value.to_bits();
            if (bits >> 48) == 0x7FFE {
                let class_id = (bits & 0xFFFF_FFFF) as u32;
                let present =
                    crate::symbol::class_static_symbol_lookup(class_id, key_value).is_some();
                return f64::from_bits(if present { TAG_TRUE } else { TAG_FALSE });
            }
            let present = crate::symbol::js_object_has_own_symbol(obj_value, key_value);
            return f64::from_bits(if present { TAG_TRUE } else { TAG_FALSE });
        }

        let key_str = crate::builtins::js_string_coerce(key_value);
        if key_str.is_null() {
            return f64::from_bits(TAG_FALSE);
        }

        if obj_js.is_any_string() {
            let present =
                super::super::has_own_helpers::string_primitive_own_key_present(obj_value, key_str);
            return f64::from_bits(if present { TAG_TRUE } else { TAG_FALSE });
        }

        if let Some(present) = registered_buffer_index_own_property_present(obj_value, key_str) {
            return f64::from_bits(if present { TAG_TRUE } else { TAG_FALSE });
        }

        if let Some(class_id) = super::super::class_ref_id(obj_value) {
            let present = super::super::has_own_helpers::str_from_string_header(key_str)
                .map(|key| {
                    if key.starts_with('#') {
                        // Private static elements are never reflectable own
                        // properties of the class constructor.
                        false
                    } else if super::super::class_registry::class_is_key_deleted(class_id, key) {
                        false
                    } else if matches!(key, "length" | "prototype") {
                        true
                    } else if key == "name"
                        && super::super::class_registry::lookup_static_method_in_chain(class_id, key)
                            .is_none()
                    {
                        super::super::class_registry::class_name_for_id(class_id).is_some()
                    } else {
                        CLASS_DYNAMIC_PROPS.with(|m| {
                            m.borrow()
                                .get(&class_id)
                                .is_some_and(|props| props.contains_key(key))
                        }) || super::super::class_registry::lookup_static_method_in_chain(class_id, key)
                            .is_some()
                            // A static accessor (`static get x()`) is an own
                            // property of the constructor — own-only, mirroring
                            // getOwnPropertyDescriptor (class/definition/
                            // {getters,setters}-prop-desc `staticX`).
                            || super::super::class_registry::class_own_static_accessor_ptrs(class_id, key)
                                .is_some()
                    }
                })
                .unwrap_or(false);
            return f64::from_bits(if present { TAG_TRUE } else { TAG_FALSE });
        }

        if let Some(addr) = crate::typedarray_props::typed_array_addr_from_value(obj_value) {
            let present = crate::typedarray_props::typed_array_has_own_property(
                addr as *const crate::typedarray::TypedArrayHeader,
                key_str,
            );
            return f64::from_bits(if present { TAG_TRUE } else { TAG_FALSE });
        }

        // #3655: functions/closures carry built-in own `name`/`length`
        // (and `prototype` for constructors) plus any user-attached props.
        // Route them here instead of through `extract_obj_ptr`/`own_key_present`,
        // which would read `keys_array` off a closure (out of bounds).
        if obj_js.is_pointer() {
            let ptr = obj_js.as_pointer::<u8>() as usize;
            if crate::buffer::is_registered_buffer(ptr) {
                let present = super::super::has_own_helpers::buffer_own_key_present(
                    ptr as *const crate::buffer::BufferHeader,
                    key_str,
                );
                return f64::from_bits(if present { TAG_TRUE } else { TAG_FALSE });
            }
            // Date / RegExp / Error exotic instances: own expando props
            // (side tables) + per-kind builtin own slots.
            if let Some(kind) = super::super::exotic_expando::exotic_expando_kind(ptr) {
                use super::super::exotic_expando::ExoticKind;
                let present = super::super::has_own_helpers::str_from_string_header(key_str)
                    .map(|key| {
                        super::super::exotic_expando::exotic_has_own_property(kind, ptr, key)
                            || match kind {
                                ExoticKind::RegExp => key == "lastIndex",
                                ExoticKind::Error => crate::error::js_error_has_own_property(
                                    ptr as *mut crate::error::ErrorHeader,
                                    key,
                                ),
                                ExoticKind::Date | ExoticKind::Temporal | ExoticKind::Promise => {
                                    false
                                }
                            }
                    })
                    .unwrap_or(false);
                return f64::from_bits(if present { TAG_TRUE } else { TAG_FALSE });
            }
            if crate::closure::is_closure_ptr(ptr) {
                let present = super::super::has_own_helpers::str_from_string_header(key_str)
                    .map(|k| super::super::has_own_helpers::closure_own_key_present(ptr, k))
                    .unwrap_or(false);
                return f64::from_bits(if present { TAG_TRUE } else { TAG_FALSE });
            }
            if crate::typedarray::lookup_typed_array_kind(ptr).is_some() {
                let present = crate::typedarray_props::typed_array_has_own_property(
                    ptr as *const crate::typedarray::TypedArrayHeader,
                    key_str,
                );
                return f64::from_bits(if present { TAG_TRUE } else { TAG_FALSE });
            }
            if ptr >= crate::gc::GC_HEADER_SIZE + 0x1000
                && crate::object::is_valid_obj_ptr(ptr as *const u8)
            {
                let gc_header =
                    (ptr as *const u8).sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader;
                if (*gc_header).obj_type == crate::gc::GC_TYPE_ERROR {
                    let present = super::super::has_own_helpers::str_from_string_header(key_str)
                        .map(|key| {
                            crate::error::js_error_has_own_property(
                                ptr as *mut crate::error::ErrorHeader,
                                key,
                            )
                        })
                        .unwrap_or(false);
                    return f64::from_bits(if present { TAG_TRUE } else { TAG_FALSE });
                }
            }
        }

        let obj = extract_obj_ptr(obj_value);
        if obj.is_null() || (obj as usize) < 0x10000 {
            return f64::from_bits(TAG_FALSE);
        }

        // `%Function.prototype%` is an ordinary object — `is_closure_ptr` is
        // false for it, so the closure branch above never sees it — but it's
        // installed with a full set of generic `Object.prototype` methods as
        // real own data properties so they dispatch when called directly on
        // it. Per spec (20.2.3) it only *inherits* these from
        // `Object.prototype`; they are not its own (test262 built-ins/
        // Function/prototype/S15.3.4_A4) — UNLESS user code has since
        // overwritten the slot (`Object.defineProperty(Function.prototype,
        // "valueOf", …)`), which per spec DOES create a genuine own property.
        // Distinguish the two by checking whether the currently-installed
        // value is still the exact install-time thunk closure: an explicit
        // redefine always stores a different value (a new closure, or a
        // non-function entirely), never literally the same `func_ptr`.
        if super::super::global_this::is_function_prototype_object_value(obj_value) {
            if let Some(key) = super::super::has_own_helpers::str_from_string_header(key_str) {
                // `install_noop_proto_methods` (the actual installer Function.
                // prototype goes through) backs most of `OBJECT_PROTO_METHODS`
                // with the shared `global_this_builtin_noop_thunk` — only
                // `isPrototypeOf` and the four Annex B accessor helpers get a
                // dedicated per-method thunk there. `object_prototype_has_own_
                // property_thunk` et al. are real thunks too, but they're wired
                // up only for `Object.prototype` itself (a different install
                // call site), never for `Function.prototype` — so comparing
                // against them here would always mismatch and defeat the
                // still-default check entirely.
                let expected_thunk: Option<*const u8> = match key {
                    "hasOwnProperty" | "propertyIsEnumerable" | "toLocaleString" | "valueOf" => {
                        Some(super::super::global_this::global_this_builtin_noop_thunk as *const u8)
                    }
                    "isPrototypeOf" => Some(
                        super::super::global_this::object_prototype_is_prototype_of_thunk
                            as *const u8,
                    ),
                    "__defineGetter__" => Some(
                        super::super::global_this::object_prototype_define_getter_thunk
                            as *const u8,
                    ),
                    "__defineSetter__" => Some(
                        super::super::global_this::object_prototype_define_setter_thunk
                            as *const u8,
                    ),
                    "__lookupGetter__" => Some(
                        super::super::global_this::object_prototype_lookup_getter_thunk
                            as *const u8,
                    ),
                    "__lookupSetter__" => Some(
                        super::super::global_this::object_prototype_lookup_setter_thunk
                            as *const u8,
                    ),
                    _ => None,
                };
                if let Some(expected) = expected_thunk {
                    let current = js_object_get_field_by_name(obj, key_str);
                    let still_default_shim = if current.is_pointer() {
                        let cur_ptr = current.as_pointer::<u8>() as usize;
                        crate::closure::is_closure_ptr(cur_ptr)
                            && crate::closure::get_valid_func_ptr(
                                cur_ptr as *const crate::closure::ClosureHeader,
                            ) == expected
                    } else {
                        false
                    };
                    if still_default_shim {
                        return f64::from_bits(TAG_FALSE);
                    }
                }
            }
        }

        if (*obj).class_id == super::super::native_module::NATIVE_MODULE_CLASS_ID {
            let present = super::super::native_module::read_native_module_name(obj)
                .as_deref()
                .zip(super::super::has_own_helpers::str_from_string_header(
                    key_str,
                ))
                .map(|(module, key)| {
                    super::super::native_module::native_module_vtable()
                        .is_some_and(|vt| (vt.has_enumerable_key)(module, key))
                })
                .unwrap_or(false);
            return f64::from_bits(if present { TAG_TRUE } else { TAG_FALSE });
        }

        if (obj as usize) >= crate::gc::GC_HEADER_SIZE + 0x1000 {
            let gc_header =
                (obj as *const u8).sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader;
            if (*gc_header).obj_type == crate::gc::GC_TYPE_ARRAY {
                let present = super::super::has_own_helpers::array_own_key_present(
                    obj as *const crate::array::ArrayHeader,
                    key_str,
                );
                return f64::from_bits(if present { TAG_TRUE } else { TAG_FALSE });
            }
        }

        if (*obj).class_id == NATIVE_MODULE_CLASS_ID {
            let Some(key_name) = super::super::has_own_helpers::str_from_string_header(key_str)
            else {
                return f64::from_bits(TAG_FALSE);
            };
            let present = read_native_module_name(obj)
                .as_deref()
                .is_some_and(|module_name| {
                    super::super::native_module::native_module_vtable()
                        .is_some_and(|vt| (vt.has_enumerable_key)(module_name, key_name))
                });
            return f64::from_bits(if present { TAG_TRUE } else { TAG_FALSE });
        }

        // Private elements (`#x`) — and perry's hidden `__perry_collection_backing__`
        // runtime-internal field — sit in a class instance's keys_array but are
        // never reflectable own properties, so `Object.hasOwn` must report false
        // for them. Plain literals keep class_id 0.
        if (*obj).class_id != 0 {
            if let Some(key) = super::super::has_own_helpers::str_from_string_header(key_str) {
                if key.starts_with('#') || super::super::field_get_set::is_internal_runtime_key(key)
                {
                    return f64::from_bits(TAG_FALSE);
                }
            }
        }

        if own_key_present(obj, key_str) {
            return f64::from_bits(TAG_TRUE);
        }

        // A class-declaration prototype object: instance accessors (`get x()`)
        // and methods live in the class vtable, not the object's keys_array, yet
        // they ARE own properties of `C.prototype` — `getOwnPropertyDescriptor`
        // already reflects them, so `hasOwnProperty` must agree (test262
        // class/definition/{getters,setters}-prop-desc, which assert via
        // `verifyProperty` → `hasOwnProperty`).
        if let Some(cid) =
            super::super::class_registry::class_id_for_decl_prototype_object(obj as usize)
        {
            if let Some(key) = super::super::has_own_helpers::str_from_string_header(key_str) {
                if !super::super::class_registry::class_is_key_deleted(cid, key)
                    && (key == "constructor"
                        || super::super::class_registry::class_own_accessor_ptrs(cid, key)
                            .is_some()
                        || super::super::native_module::class_has_own_method(cid, key))
                {
                    return f64::from_bits(TAG_TRUE);
                }
            }
        }

        f64::from_bits(TAG_FALSE)
    }
}

/// `Object.prototype.propertyIsEnumerable.call(obj, key)` (#2891).
#[no_mangle]
pub extern "C" fn js_object_property_is_enumerable(obj_value: f64, key_value: f64) -> f64 {
    const TAG_TRUE: u64 = 0x7FFC_0000_0000_0004;
    const TAG_FALSE: u64 = 0x7FFC_0000_0000_0003;
    unsafe {
        let obj_jv = crate::JSValue::from_bits(obj_value.to_bits());
        if obj_jv.is_null() || obj_jv.is_undefined() {
            super::super::has_own_helpers::throw_to_object_nullish_type_error();
        }

        // ToPropertyKey(V): fold an object argument (e.g. one whose `toString`
        // returns a Symbol) into its canonical key before the symbol/string
        // split. A no-op for keys that are already primitives.
        let key_value = super::super::js_to_property_key(key_value);

        // Proxy receiver: resolve the descriptor via `[[GetOwnProperty]]` and
        // report its `enumerable` attribute (absent property → false) rather
        // than dereferencing the fake pointer. (Proxy crash cluster.)
        if crate::proxy::js_proxy_is_proxy(obj_value) != 0 {
            let desc = crate::proxy::js_reflect_get_own_property_descriptor(obj_value, key_value);
            if desc.to_bits() == crate::value::TAG_UNDEFINED {
                return f64::from_bits(TAG_FALSE);
            }
            let desc_ptr = extract_obj_ptr(desc);
            if desc_ptr.is_null() {
                return f64::from_bits(TAG_FALSE);
            }
            let enum_key = crate::string::js_string_from_bytes(b"enumerable".as_ptr(), 10);
            let enum_v = js_object_get_field_by_name(desc_ptr as *const ObjectHeader, enum_key);
            return f64::from_bits(
                if crate::value::js_is_truthy(f64::from_bits(enum_v.bits())) != 0 {
                    TAG_TRUE
                } else {
                    TAG_FALSE
                },
            );
        }

        // Symbol-keyed lookup: route through the SYMBOL_PROPERTIES side
        // table (mirrors js_object_has_own) — string-coercing a Symbol key
        // below would never match and reported every symbol prop as
        // non-enumerable.
        if crate::symbol::js_is_symbol(key_value) != 0 {
            let bits = obj_value.to_bits();
            if (bits >> 48) == 0x7FFE {
                // ClassRef receivers: statics live in the class registry and
                // are non-enumerable like builtin statics.
                return f64::from_bits(TAG_FALSE);
            }
            if !crate::symbol::js_object_has_own_symbol(obj_value, key_value) {
                return f64::from_bits(TAG_FALSE);
            }
            let owner = (obj_value.to_bits() & crate::value::POINTER_MASK) as usize;
            let sym = (key_value.to_bits() & crate::value::POINTER_MASK) as usize;
            let enumerable = crate::symbol::symbol_property_is_enumerable(owner, sym);
            return f64::from_bits(if enumerable { TAG_TRUE } else { TAG_FALSE });
        }

        let key_str = crate::builtins::js_string_coerce(key_value);
        if key_str.is_null() {
            return f64::from_bits(TAG_FALSE);
        }

        // ClassRef receiver (INT32-tagged constructor, not a heap object): the
        // only enumerable own string keys are the static FIELDS recorded in
        // CLASS_DYNAMIC_PROPS — `length`/`name`/`prototype` and static
        // methods/accessors are non-enumerable. `extract_obj_ptr` below would
        // null out on the INT32 payload and report every key non-enumerable, so
        // `verifyProperty(C, "f", …)`'s isEnumerable check failed (test262
        // class/elements static-field-declaration & friends).
        if let Some(class_id) = super::super::class_ref_id(obj_value) {
            if super::super::class_prototype_ref_id(obj_value).is_none() {
                if let Some(key_name) =
                    super::super::has_own_helpers::str_from_string_header(key_str)
                {
                    let is_static_field = !key_name.starts_with('#')
                        && super::super::class_registry::class_own_static_field_value(
                            class_id, key_name,
                        )
                        .is_some();
                    return f64::from_bits(if is_static_field { TAG_TRUE } else { TAG_FALSE });
                }
            }
        }

        // String primitives: index keys in range are enumerable own props;
        // "length" is a non-enumerable own prop; everything else absent.
        if obj_jv.is_any_string() {
            let present =
                super::super::has_own_helpers::string_primitive_own_key_present(obj_value, key_str);
            if !present {
                return f64::from_bits(TAG_FALSE);
            }
            let name_ptr = (key_str as *const u8).add(std::mem::size_of::<crate::StringHeader>());
            let name_len = (*key_str).byte_len as usize;
            let is_length = std::str::from_utf8(std::slice::from_raw_parts(name_ptr, name_len))
                .map(|s| s == "length")
                .unwrap_or(false);
            return f64::from_bits(if is_length { TAG_FALSE } else { TAG_TRUE });
        }

        if let Some(present) = registered_buffer_index_own_property_present(obj_value, key_str) {
            return f64::from_bits(if present { TAG_TRUE } else { TAG_FALSE });
        }

        if let Some(addr) = crate::typedarray_props::typed_array_addr_from_value(obj_value) {
            let enumerable = crate::typedarray_props::typed_array_property_is_enumerable(
                addr as *const crate::typedarray::TypedArrayHeader,
                key_str,
            );
            return f64::from_bits(if enumerable { TAG_TRUE } else { TAG_FALSE });
        }

        // Date / RegExp / Error exotic instances: expando/accessor own props
        // report their side-table enumerability (default true for plain
        // expando writes); builtin own slots are non-enumerable.
        if let Some((addr, kind)) =
            super::super::exotic_expando::exotic_expando_kind_of_value(obj_value)
        {
            let Some(key_name) = super::super::has_own_helpers::str_from_string_header(key_str)
            else {
                return f64::from_bits(TAG_FALSE);
            };
            if !super::super::exotic_expando::exotic_has_own_property(kind, addr, key_name) {
                return f64::from_bits(TAG_FALSE);
            }
            let enumerable = super::super::get_property_attrs(addr, key_name)
                .map(|a| a.enumerable())
                .unwrap_or_else(|| {
                    super::super::exotic_expando::exotic_default_enumerable(kind, key_name)
                });
            return f64::from_bits(if enumerable { TAG_TRUE } else { TAG_FALSE });
        }

        // #3655: functions/closures. Built-in `name`/`length`/`prototype` are
        // non-enumerable; user-attached props default to enumerable.
        if obj_jv.is_pointer() {
            let ptr = obj_jv.as_pointer::<u8>() as usize;
            if crate::closure::is_closure_ptr(ptr) {
                let Some(key_name) = super::super::has_own_helpers::str_from_string_header(key_str)
                else {
                    return f64::from_bits(TAG_FALSE);
                };
                if !super::super::has_own_helpers::closure_own_key_present(ptr, key_name) {
                    return f64::from_bits(TAG_FALSE);
                }
                if matches!(key_name, "name" | "length" | "prototype") {
                    return f64::from_bits(TAG_FALSE);
                }
                let enumerable = super::super::get_property_attrs(ptr, key_name)
                    .map(|attrs| attrs.enumerable())
                    .unwrap_or(true);
                return f64::from_bits(if enumerable { TAG_TRUE } else { TAG_FALSE });
            }
            if crate::typedarray::lookup_typed_array_kind(ptr).is_some() {
                let enumerable = crate::typedarray_props::typed_array_property_is_enumerable(
                    ptr as *const crate::typedarray::TypedArrayHeader,
                    key_str,
                );
                return f64::from_bits(if enumerable { TAG_TRUE } else { TAG_FALSE });
            }
        }

        let obj = extract_obj_ptr(obj_value);
        if obj.is_null() || (obj as usize) < 0x10000 {
            return f64::from_bits(TAG_FALSE);
        }
        let name_ptr = (key_str as *const u8).add(std::mem::size_of::<crate::StringHeader>());
        let name_len = (*key_str).byte_len as usize;
        let key_name = match std::str::from_utf8(std::slice::from_raw_parts(name_ptr, name_len)) {
            Ok(s) => s,
            Err(_) => return f64::from_bits(TAG_FALSE),
        };
        if let Some(result) = super::super::array_property_is_enumerable(obj, key_str, key_name) {
            return result;
        }
        if !is_valid_obj_ptr(obj as *const u8) {
            return f64::from_bits(TAG_FALSE);
        }
        if (*obj).class_id == NATIVE_MODULE_CLASS_ID {
            if let Some(module_name) = read_native_module_name(obj) {
                return f64::from_bits(
                    if native_module_has_enumerable_key(&module_name, key_name) {
                        TAG_TRUE
                    } else {
                        TAG_FALSE
                    },
                );
            }
        }
        // Perry's hidden `__perry_*` runtime-internal own keys (e.g. the
        // `class … extends Map/Set` backing field) physically live in a class
        // instance's keys_array but must never be observable, so report them as
        // non-enumerable like private (`#`) elements.
        if (*obj).class_id != 0 && super::super::field_get_set::is_internal_runtime_key(key_name) {
            return f64::from_bits(TAG_FALSE);
        }
        if !own_key_present(obj, key_str) {
            return f64::from_bits(TAG_FALSE);
        }
        let enumerable = super::super::get_property_attrs(obj as usize, key_name)
            .map(|attrs| attrs.enumerable())
            .unwrap_or(true);
        f64::from_bits(if enumerable { TAG_TRUE } else { TAG_FALSE })
    }
}

#[used]
static KEEP_PROPERTY_IS_ENUMERABLE: extern "C" fn(f64, f64) -> f64 =
    js_object_property_is_enumerable;
