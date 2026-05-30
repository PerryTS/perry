//! `Object.getOwnPropertyDescriptor(s)`, `Object.getOwnPropertyNames`, and
//! `Object.create` (with descriptor bag) — descriptor introspection and
//! creation, split out of `object_ops.rs` to keep that file under the
//! 2000-line cap (#2816/#2817/#2843). Pure relocation; `use super::*` gives
//! the same visibility the parent module has.

use super::*;

/// Object.getOwnPropertyDescriptor(obj, key) — returns a data descriptor
/// `{ value, writable, enumerable, configurable }` for data properties, or an
/// accessor descriptor `{ get, set, enumerable, configurable }` for properties
/// installed via `Object.defineProperty(obj, key, { get, set })`. Returns
/// TAG_UNDEFINED if the property doesn't exist.
#[no_mangle]
pub extern "C" fn js_object_get_own_property_descriptor(obj_value: f64, key_value: f64) -> f64 {
    const TAG_TRUE: u64 = 0x7FFC_0000_0000_0004;
    const TAG_FALSE: u64 = 0x7FFC_0000_0000_0003;
    unsafe {
        // #2818: ToObject(null/undefined) throws TypeError, matching Node.
        let obj_jv = crate::JSValue::from_bits(obj_value.to_bits());
        if obj_jv.is_null() || obj_jv.is_undefined() {
            super::has_own_helpers::throw_to_object_nullish_type_error();
        }

        // #2818: string primitives box to String objects whose own
        // properties are the index keys "0".."len-1" (writable:false,
        // enumerable:true, configurable:false) plus "length"
        // (writable:false, enumerable:false, configurable:false).
        if obj_jv.is_any_string() {
            return string_primitive_descriptor(obj_value, key_value);
        }

        if let Some(class_id) = class_ref_id(obj_value) {
            let method_name = metadata_key_to_string(key_value);
            if let Some(method_name) = method_name {
                if method_name == "constructor" || class_has_own_method(class_id, &method_name) {
                    let value = if method_name == "constructor" {
                        obj_value
                    } else {
                        class_prototype_method_value_for_name(class_id, &method_name)
                    };
                    let packed = b"value\0writable\0enumerable\0configurable";
                    let desc = js_object_alloc_with_shape(
                        0x0D_E5_C2,
                        4,
                        packed.as_ptr(),
                        packed.len() as u32,
                    );
                    let header_size = std::mem::size_of::<ObjectHeader>();
                    let fields = (desc as *mut u8).add(header_size) as *mut f64;
                    // GC_STORE_AUDIT(INIT): descriptor object is freshly allocated; layout is rebuilt before publication.
                    *fields = value;
                    *fields.add(1) = f64::from_bits(TAG_TRUE);
                    *fields.add(2) = f64::from_bits(TAG_FALSE);
                    *fields.add(3) = f64::from_bits(TAG_TRUE);
                    super::rebuild_object_field_layout(desc, 4);
                    return f64::from_bits((desc as u64) | 0x7FFD_0000_0000_0000);
                }
            }
            return f64::from_bits(crate::value::TAG_UNDEFINED);
        }

        // #2059: function objects (closures) are not `ObjectHeader`s — routing
        // them through `extract_obj_ptr`/`own_key_present` below reads an
        // out-of-bounds "keys_array" slot (offset 16, past a 0-capture
        // closure's payload) and segfaults. Resolve their descriptors here:
        // the built-in `name`/`length` slots (non-writable, non-enumerable,
        // configurable per spec) plus any user-attached own data property.
        {
            let jsv = crate::JSValue::from_bits(obj_value.to_bits());
            if jsv.is_pointer() {
                let ptr = jsv.as_pointer::<u8>() as usize;
                if crate::closure::is_closure_ptr(ptr) {
                    let key_str = crate::builtins::js_string_coerce(key_value);
                    if key_str.is_null() {
                        return f64::from_bits(crate::value::TAG_UNDEFINED);
                    }
                    let name_ptr =
                        (key_str as *const u8).add(std::mem::size_of::<crate::StringHeader>());
                    let name_len = (*key_str).byte_len as usize;
                    let name = std::str::from_utf8(std::slice::from_raw_parts(name_ptr, name_len))
                        .unwrap_or("");

                    // (value, writable, configurable). `name`/`length` are the
                    // built-in own data slots; anything else falls back to the
                    // user-attached dynamic-property side table.
                    // Built-in `name`/`length` are spec'd `{ writable: false,
                    // configurable: true }`. A registered descriptor (e.g.
                    // from `install_proto_method`, #3143, or a user
                    // `Object.defineProperty`) overrides those defaults.
                    let registered = super::get_property_attrs(ptr, name);
                    let writable_default = registered.map(|a| a.writable());
                    let configurable_default = registered.map(|a| a.configurable()).unwrap_or(true);
                    let resolved: Option<(f64, bool, bool)> = match name {
                        "length" => {
                            let closure_value = crate::value::js_nanbox_pointer(ptr as i64);
                            let arity = if let Some(arity) =
                                super::native_module::bound_native_callable_value_arity(
                                    closure_value,
                                ) {
                                arity
                            } else if let Some(len) =
                                super::native_module::builtin_closure_length(ptr)
                            {
                                // #3143: per-closure spec length for built-in
                                // proto methods (shared func_ptr can't carry it).
                                len
                            } else {
                                crate::closure::closure_arity(
                                    ptr as *const crate::closure::ClosureHeader,
                                )
                                .unwrap_or(0)
                            };
                            // Numbers are NaN-boxed as their raw f64 bits.
                            Some((
                                arity as f64,
                                writable_default.unwrap_or(false),
                                configurable_default,
                            ))
                        }
                        "name" => {
                            let dynv = crate::closure::closure_get_dynamic_prop(ptr, "name");
                            if dynv.to_bits() != crate::value::TAG_UNDEFINED {
                                // Function `.name` is spec'd non-writable; honor
                                // a registered override but otherwise report
                                // `writable: false` (#3143), not the old default
                                // of `true`.
                                Some((
                                    dynv,
                                    writable_default.unwrap_or(false),
                                    configurable_default,
                                ))
                            } else {
                                let func_ptr = (*(ptr as *const crate::closure::ClosureHeader))
                                    .func_ptr
                                    as usize;
                                let fname = crate::builtins::function_name_for_ptr(func_ptr)
                                    .unwrap_or_default();
                                let s = crate::string::js_string_from_bytes(
                                    fname.as_ptr(),
                                    fname.len() as u32,
                                );
                                Some((crate::js_nanbox_string(s as i64), false, true))
                            }
                        }
                        _ => {
                            let dynv = crate::closure::closure_get_dynamic_prop(ptr, name);
                            if dynv.to_bits() != crate::value::TAG_UNDEFINED {
                                Some((dynv, true, true))
                            } else {
                                None
                            }
                        }
                    };
                    let Some((value, writable, configurable)) = resolved else {
                        return f64::from_bits(crate::value::TAG_UNDEFINED);
                    };
                    // `name`/`length` are non-enumerable; user data props are.
                    let enumerable = !matches!(name, "name" | "length");
                    let packed = b"value\0writable\0enumerable\0configurable";
                    let desc = js_object_alloc_with_shape(
                        0x0D_E5_C0,
                        4,
                        packed.as_ptr(),
                        packed.len() as u32,
                    );
                    let header_size = std::mem::size_of::<ObjectHeader>();
                    let fields = (desc as *mut u8).add(header_size) as *mut f64;
                    // GC_STORE_AUDIT(INIT): descriptor object is freshly allocated; layout is rebuilt before publication.
                    *fields = value;
                    *fields.add(1) = f64::from_bits(if writable { TAG_TRUE } else { TAG_FALSE });
                    *fields.add(2) = f64::from_bits(if enumerable { TAG_TRUE } else { TAG_FALSE });
                    *fields.add(3) =
                        f64::from_bits(if configurable { TAG_TRUE } else { TAG_FALSE });
                    super::rebuild_object_field_layout(desc, 4);
                    return f64::from_bits((desc as u64) | 0x7FFD_0000_0000_0000);
                }
            }
        }

        let obj = extract_obj_ptr(obj_value);
        if obj.is_null() {
            return f64::from_bits(crate::value::TAG_UNDEFINED);
        }
        // Extract key string
        let key_str = crate::builtins::js_string_coerce(key_value);
        if key_str.is_null() {
            return f64::from_bits(crate::value::TAG_UNDEFINED);
        }
        // Extract key as a Rust string for descriptor lookup.
        let key_rust: Option<String> = {
            let name_ptr = (key_str as *const u8).add(std::mem::size_of::<crate::StringHeader>());
            let name_len = (*key_str).byte_len as usize;
            let name_bytes = std::slice::from_raw_parts(name_ptr, name_len);
            std::str::from_utf8(name_bytes).ok().map(|s| s.to_string())
        };

        // Check whether the key is actually present on the object. A property can
        // legitimately hold `undefined`, and accessor descriptors have no value slot,
        // so we check the keys_array directly instead of relying on "value != undefined".
        let present = own_key_present(obj, key_str);
        if !present {
            return f64::from_bits(crate::value::TAG_UNDEFINED);
        }

        // Look up descriptor flags (default: all true).
        let attrs = key_rust
            .as_ref()
            .and_then(|k| get_property_attrs(obj as usize, k))
            .unwrap_or(PropertyAttrs::new(true, true, true));
        let bool_to_f64 = |b: bool| f64::from_bits(if b { TAG_TRUE } else { TAG_FALSE });

        // Accessor descriptor path.
        if let Some(acc) = key_rust
            .as_ref()
            .and_then(|k| get_accessor_descriptor(obj as usize, k))
        {
            let packed = b"get\0set\0enumerable\0configurable";
            let desc =
                js_object_alloc_with_shape(0x0D_E5_C1, 4, packed.as_ptr(), packed.len() as u32);
            let header_size = std::mem::size_of::<ObjectHeader>();
            let fields = (desc as *mut u8).add(header_size) as *mut f64;
            // GC_STORE_AUDIT(INIT): descriptor object is freshly allocated; layout is rebuilt before publication.
            *fields = if acc.get != 0 {
                f64::from_bits(acc.get)
            } else {
                f64::from_bits(crate::value::TAG_UNDEFINED)
            };
            *fields.add(1) = if acc.set != 0 {
                f64::from_bits(acc.set)
            } else {
                f64::from_bits(crate::value::TAG_UNDEFINED)
            };
            // GC_STORE_AUDIT(INIT): descriptor boolean fields are pointer-free and layout is rebuilt below.
            *fields.add(2) = bool_to_f64(attrs.enumerable());
            *fields.add(3) = bool_to_f64(attrs.configurable());
            super::rebuild_object_field_layout(desc, 4);
            return f64::from_bits((desc as u64) | 0x7FFD_0000_0000_0000);
        }

        // Data descriptor path.
        let value = js_object_get_field_by_name(obj, key_str);
        let packed = b"value\0writable\0enumerable\0configurable";
        let desc = js_object_alloc_with_shape(
            0x0D_E5_C0, // unique shape_id for property descriptors
            4,
            packed.as_ptr(),
            packed.len() as u32,
        );
        let header_size = std::mem::size_of::<ObjectHeader>();
        let fields = (desc as *mut u8).add(header_size) as *mut f64;
        // GC_STORE_AUDIT(INIT): descriptor object is freshly allocated; layout is rebuilt before publication.
        *fields = f64::from_bits(value.bits()); // value
        *fields.add(1) = bool_to_f64(attrs.writable()); // writable
        *fields.add(2) = bool_to_f64(attrs.enumerable()); // enumerable
        *fields.add(3) = bool_to_f64(attrs.configurable()); // configurable
        super::rebuild_object_field_layout(desc, 4);
        f64::from_bits((desc as u64) | 0x7FFD_0000_0000_0000)
    }
}

/// Build a `{ value, writable, enumerable, configurable }` data descriptor
/// object. Shared by the string-primitive descriptor path (#2818).
unsafe fn build_data_descriptor(
    value: f64,
    writable: bool,
    enumerable: bool,
    configurable: bool,
) -> f64 {
    const TAG_TRUE: u64 = 0x7FFC_0000_0000_0004;
    const TAG_FALSE: u64 = 0x7FFC_0000_0000_0003;
    let bf = |b: bool| f64::from_bits(if b { TAG_TRUE } else { TAG_FALSE });
    let packed = b"value\0writable\0enumerable\0configurable";
    let desc = js_object_alloc_with_shape(0x0D_E5_C0, 4, packed.as_ptr(), packed.len() as u32);
    let header_size = std::mem::size_of::<ObjectHeader>();
    let fields = (desc as *mut u8).add(header_size) as *mut f64;
    // GC_STORE_AUDIT(INIT): descriptor object is freshly allocated; layout is rebuilt before publication.
    *fields = value;
    *fields.add(1) = bf(writable);
    *fields.add(2) = bf(enumerable);
    *fields.add(3) = bf(configurable);
    super::rebuild_object_field_layout(desc, 4);
    f64::from_bits((desc as u64) | 0x7FFD_0000_0000_0000)
}

/// #2818: own-property descriptor for a string primitive receiver. Index keys
/// in range yield the single-char value descriptor (writable:false,
/// enumerable:true, configurable:false); "length" yields the length value
/// descriptor (writable:false, enumerable:false, configurable:false). Any
/// other key is absent → undefined.
unsafe fn string_primitive_descriptor(str_value: f64, key_value: f64) -> f64 {
    let key_str = crate::builtins::js_string_coerce(key_value);
    if key_str.is_null() {
        return f64::from_bits(crate::value::TAG_UNDEFINED);
    }
    let name_ptr = (key_str as *const u8).add(std::mem::size_of::<crate::StringHeader>());
    let name_len = (*key_str).byte_len as usize;
    let name = match std::str::from_utf8(std::slice::from_raw_parts(name_ptr, name_len)) {
        Ok(s) => s,
        Err(_) => return f64::from_bits(crate::value::TAG_UNDEFINED),
    };

    let mut scratch = [0u8; crate::value::SHORT_STRING_MAX_LEN];
    let (sptr, sblen) = match crate::string::str_bytes_from_jsvalue(str_value, &mut scratch) {
        Some((p, b)) if !p.is_null() => (p, b),
        _ => return f64::from_bits(crate::value::TAG_UNDEFINED),
    };
    let utf16_len = crate::string::compute_utf16_len(sptr, sblen);

    if name == "length" {
        return build_data_descriptor(utf16_len as f64, false, false, false);
    }

    if let Some(index) = super::canonical_array_index(name) {
        if index < utf16_len {
            // Materialize the single UTF-16 unit at `index` as a 1-char string.
            let bytes = std::slice::from_raw_parts(sptr, sblen as usize);
            let s = std::str::from_utf8(bytes).unwrap_or("");
            if let Some(ch) = s.chars().nth(index as usize) {
                let mut buf = [0u8; 4];
                let cs = ch.encode_utf8(&mut buf);
                let cstr = crate::string::js_string_from_bytes(cs.as_ptr(), cs.len() as u32);
                let char_val = f64::from_bits(JSValue::string_ptr(cstr).bits());
                return build_data_descriptor(char_val, false, true, false);
            }
        }
    }
    f64::from_bits(crate::value::TAG_UNDEFINED)
}

/// Object.getOwnPropertyNames(obj) — returns all own property names (including non-enumerable).
/// Takes a NaN-boxed f64 object pointer, returns a NaN-boxed f64 array pointer.
#[no_mangle]
pub extern "C" fn js_object_get_own_property_names(obj_value: f64) -> f64 {
    unsafe {
        // #2818: ToObject(null/undefined) throws TypeError, matching Node.
        let obj_jv = crate::JSValue::from_bits(obj_value.to_bits());
        if obj_jv.is_null() || obj_jv.is_undefined() {
            super::has_own_helpers::throw_to_object_nullish_type_error();
        }
        if let Some(class_id) = class_ref_id(obj_value) {
            let mut names: Vec<String> = vec!["constructor".to_string()];
            if let Ok(registry) = CLASS_VTABLE_REGISTRY.read() {
                if let Some(reg) = registry.as_ref() {
                    if let Some(vtable) = reg.get(&class_id) {
                        let mut methods: Vec<String> = vtable.methods.keys().cloned().collect();
                        methods.sort();
                        names.extend(methods);
                    }
                }
            }
            let result = crate::array::js_array_alloc(names.len() as u32);
            for name in names {
                let str_ptr = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
                crate::array::js_array_push(result, JSValue::string_ptr(str_ptr));
            }
            return f64::from_bits((result as u64) | 0x7FFD_0000_0000_0000);
        }

        // String / array values have no `ObjectHeader.keys_array`; their own
        // property names are the index names `"0".."len-1"` plus `"length"`.
        // Reading a bogus `keys_array` off their header segfaulted (#800).
        {
            const TAG_TRUE_BITS: u64 = 0x7FFC_0000_0000_0004;
            let jv = JSValue::from_bits(obj_value.to_bits());
            let n: Option<u32> = if jv.is_any_string() {
                let mut scratch = [0u8; crate::value::SHORT_STRING_MAX_LEN];
                match crate::string::str_bytes_from_jsvalue(obj_value, &mut scratch) {
                    Some((p, blen)) if !p.is_null() => {
                        Some(crate::string::compute_utf16_len(p, blen))
                    }
                    _ => Some(0),
                }
            } else if crate::array::js_array_is_array(obj_value).to_bits() == TAG_TRUE_BITS {
                let ap = extract_obj_ptr(obj_value) as *const crate::array::ArrayHeader;
                Some(crate::array::js_array_length(ap))
            } else {
                None
            };
            if let Some(n) = n {
                let result = crate::array::js_array_alloc(n + 1);
                for i in 0..n {
                    let s = i.to_string();
                    let k = crate::string::js_string_from_bytes(s.as_ptr(), s.len() as u32);
                    crate::array::js_array_push(result, JSValue::string_ptr(k));
                }
                let lk = crate::string::js_string_from_bytes(b"length".as_ptr(), 6);
                crate::array::js_array_push(result, JSValue::string_ptr(lk));
                return f64::from_bits((result as u64) | 0x7FFD_0000_0000_0000);
            }
        }

        let obj = extract_obj_ptr(obj_value);
        if obj.is_null() {
            let empty = crate::array::js_array_alloc(0);
            return f64::from_bits((empty as u64) | 0x7FFD_0000_0000_0000);
        }
        let keys = (*obj).keys_array;
        if keys.is_null() {
            let empty = crate::array::js_array_alloc(0);
            return f64::from_bits((empty as u64) | 0x7FFD_0000_0000_0000);
        }
        // Clone the keys array — Object.getOwnPropertyNames includes ALL keys (even non-enumerable).
        let len = crate::array::js_array_length(keys) as usize;
        let result = crate::array::js_array_alloc(len as u32);
        for i in 0..len {
            let key_val = crate::array::js_array_get(keys, i as u32);
            crate::array::js_array_push_f64(result, f64::from_bits(key_val.bits()));
        }
        f64::from_bits((result as u64) | 0x7FFD_0000_0000_0000)
    }
}

/// Object.getOwnPropertyDescriptors(obj) — returns a new object whose own
/// property keys (the same set `Object.getOwnPropertyNames` reports, including
/// non-enumerable keys and class-ref method names) each map to the property
/// descriptor produced by `js_object_get_own_property_descriptor`. Spec:
/// "for each own property key K of O, set result[K] = descriptor(O, K)".
///
/// effect's `SchemaAST.annotations` builds a fresh AST node via
/// `Object.create(Object.getPrototypeOf(ast), Object.getOwnPropertyDescriptors(ast))`,
/// so without this the plural call lowered to a null callee and Schema.ts
/// module init threw `TypeError: value is not a function` (#1791/#1758).
#[no_mangle]
pub extern "C" fn js_object_get_own_property_descriptors(obj_value: f64) -> f64 {
    const POINTER_TAG: u64 = 0x7FFD_0000_0000_0000;
    unsafe {
        // Enumerate own keys exactly like Object.getOwnPropertyNames — this
        // handles class refs and plain objects, and includes non-enumerable
        // keys, matching the spec's [[OwnPropertyKeys]] string-key set.
        let names_value = js_object_get_own_property_names(obj_value);
        let names_arr =
            crate::value::js_nanbox_get_pointer(names_value) as *const crate::array::ArrayHeader;

        // Fresh result object that collects { key: descriptor } entries.
        // Like js_object_entries / js_object_get_own_property_names above, the
        // intermediate allocations aren't rooted — Perry's builder helpers
        // follow this convention.
        let result = js_object_alloc(0, 0);

        if !names_arr.is_null() {
            let len = crate::array::js_array_length(names_arr) as usize;
            for i in 0..len {
                let key_val = crate::array::js_array_get(names_arr, i as u32);
                let key_f64 = f64::from_bits(key_val.bits());
                let desc = js_object_get_own_property_descriptor(obj_value, key_f64);
                let key_str = crate::builtins::js_string_coerce(key_f64);
                if !key_str.is_null() {
                    js_object_set_field_by_name(result, key_str, desc);
                }
            }
        }
        f64::from_bits((result as u64) | POINTER_TAG)
    }
}

/// Object.create(proto[, propertiesObject]) — create an object with the given
/// prototype and (optionally) define properties from a descriptor bag.
///
/// `props_value` is the (NaN-boxed) properties object, or `undefined` when the
/// caller passed only one argument. #2816: the prototype argument must be an
/// object or `null`; primitives / `undefined` throw
/// `TypeError: Object prototype may only be an Object or null`.
#[no_mangle]
pub extern "C" fn js_object_create_with_props(proto_value: f64, props_value: f64) -> f64 {
    // #2816 prototype validation: only an object or `null` is permitted.
    let proto_jv = crate::value::JSValue::from_bits(proto_value.to_bits());
    let proto_ok = proto_jv.is_null()
        || unsafe { value_is_object_like(proto_value) }
        || super::class_ref_id(proto_value).is_some();
    if !proto_ok {
        throw_object_type_error(b"Object prototype may only be an Object or null");
    }

    let result = js_object_create(proto_value);

    // #2816: apply the descriptor bag, if one was supplied.
    let props_jv = crate::value::JSValue::from_bits(props_value.to_bits());
    if !props_jv.is_undefined() {
        return js_object_define_properties(result, props_value);
    }
    result
}

#[used]
static KEEP_OBJECT_CREATE_WITH_PROPS: extern "C" fn(f64, f64) -> f64 = js_object_create_with_props;
