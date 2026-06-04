//! `Object.*` static methods and descriptor machinery:
//! `Object.fromEntries`/`groupBy`/`is`/`hasOwn`/`create`/`freeze`/`seal`/
//! `defineProperty`/`getOwnPropertyDescriptor`/`getPrototypeOf`/... plus
//! the `js_object_*` helpers backing them.
use super::*;
fn throw_from_entries_type_error(message: &[u8]) -> ! {
    let msg = crate::string::js_string_from_bytes(message.as_ptr(), message.len() as u32);
    let err = crate::error::js_typeerror_new(msg);
    crate::exception::js_throw(crate::value::js_nanbox_pointer(err as i64))
}
/// Throw a `TypeError` with the given UTF-8 message bytes. Used by the
/// `Object.defineProperty` / `Object.create` descriptor + invariant validation
/// paths (#2817 / #2843 / #2816).
pub(crate) fn throw_object_type_error(message: &[u8]) -> ! {
    let msg = crate::string::js_string_from_bytes(message.as_ptr(), message.len() as u32);
    let err = crate::error::js_typeerror_new(msg);
    crate::exception::js_throw(crate::value::js_nanbox_pointer(err as i64))
}
/// Throw `TypeError: <prefix><suffix>` where `suffix` is a runtime-built
/// string (e.g. the offending descriptor value rendered with the same
/// formatting Node uses in its messages). #2817.
fn throw_object_type_error_with_suffix(prefix: &str, suffix: &str) -> ! {
    let full = format!("{prefix}{suffix}");
    let msg = crate::string::js_string_from_bytes(full.as_ptr(), full.len() as u32);
    let err = crate::error::js_typeerror_new(msg);
    crate::exception::js_throw(crate::value::js_nanbox_pointer(err as i64))
}

/// Render a value the way Node does inside its `Object.defineProperty`
/// descriptor TypeError messages (e.g. `Property description must be an
/// object: 1` / `... : undefined` / `Getter must be a function: 1`).
/// Primitives render via their natural string form; objects render as
/// `[object Object]` etc. — but in practice these error paths only fire on
/// primitives, so a simple coercion suffices.
unsafe fn describe_value_for_type_error(value: f64) -> String {
    let jv = crate::value::JSValue::from_bits(value.to_bits());
    if jv.is_undefined() {
        return "undefined".to_string();
    }
    if jv.is_null() {
        return "null".to_string();
    }
    let s = crate::value::js_jsvalue_to_string(value);
    if s.is_null() {
        return String::new();
    }
    let len = (*s).byte_len as usize;
    let data = (s as *const u8).add(std::mem::size_of::<crate::string::StringHeader>());
    let bytes = std::slice::from_raw_parts(data, len);
    std::str::from_utf8(bytes).unwrap_or("").to_string()
}

/// Is `value` a non-nullish object reference that `Object.defineProperty` /
/// `Object.create` accepts as a descriptor / properties bag? (#2817)
/// Functions/closures count as objects too.
pub(crate) unsafe fn value_is_object_like(value: f64) -> bool {
    let jv = crate::value::JSValue::from_bits(value.to_bits());
    if !jv.is_pointer() {
        // Module-level raw-I64 object pointers (top16 == 0) — accept if it
        // resolves to a real heap object.
        let bits = value.to_bits();
        if bits != 0 && bits <= 0x0000_FFFF_FFFF_FFFF && bits > 0x10000 {
            return is_valid_obj_ptr(bits as *const u8)
                || crate::closure::is_closure_ptr(bits as usize);
        }
        return false;
    }
    let ptr = jv.as_pointer::<u8>() as usize;
    if ptr < 0x10000 {
        return false;
    }
    is_valid_obj_ptr(ptr as *const u8) || crate::closure::is_closure_ptr(ptr)
}

/// Is `value` callable (a closure / function) — used to validate `get`/`set`
/// descriptor fields. Per spec, an *omitted* (undefined) accessor is allowed;
/// only a present non-callable value throws. (#2817)
unsafe fn value_is_callable(value: f64) -> bool {
    let jv = crate::value::JSValue::from_bits(value.to_bits());
    if jv.is_pointer() {
        let ptr = jv.as_pointer::<u8>() as usize;
        return ptr >= 0x1000 && crate::closure::is_closure_ptr(ptr);
    }
    // Class refs (INT32-tagged, top16 == 0x7FFE) are callable constructors.
    (value.to_bits() >> 48) == 0x7FFE
}

/// Validate a property descriptor object per ES `ToPropertyDescriptor`
/// invariants that Node surfaces as `TypeError`s (#2817). Assumes
/// `descriptor_value` is already known to be an object. Throws on:
///   - mixing accessor (`get`/`set`) and data (`value`/`writable`) fields,
///   - a present, non-callable `get`,
///   - a present, non-callable `set`.
unsafe fn validate_property_descriptor(descriptor_value: f64) {
    let desc_ptr = extract_obj_ptr(descriptor_value);
    if desc_ptr.is_null() {
        return;
    }
    let desc = desc_ptr as *const ObjectHeader;

    let has_field = |name: &[u8]| -> bool {
        let key = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
        own_key_present(desc_ptr, key)
    };
    let read = |name: &[u8]| -> crate::value::JSValue {
        let key = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
        js_object_get_field_by_name(desc, key)
    };

    let has_get = has_field(b"get");
    let has_set = has_field(b"set");
    let has_value = has_field(b"value");
    let has_writable = has_field(b"writable");

    if (has_get || has_set) && (has_value || has_writable) {
        // Node renders the offending descriptor object after the message; for
        // the plain-object descriptors that hit this path it prints `#<Object>`.
        throw_object_type_error(
            b"Invalid property descriptor. Cannot both specify accessors and a value or writable attribute, #<Object>",
        );
    }

    if has_get {
        let g = read(b"get");
        if !g.is_undefined() && !value_is_callable(f64::from_bits(g.bits())) {
            let s = describe_value_for_type_error(f64::from_bits(g.bits()));
            throw_object_type_error_with_suffix("Getter must be a function: ", &s);
        }
    }
    if has_set {
        let s_field = read(b"set");
        if !s_field.is_undefined() && !value_is_callable(f64::from_bits(s_field.bits())) {
            let s = describe_value_for_type_error(f64::from_bits(s_field.bits()));
            throw_object_type_error_with_suffix("Setter must be a function: ", &s);
        }
    }
}

/// #2843: enforce frozen / sealed / non-extensible invariants for
/// `Object.defineProperty`. `obj` is the resolved heap object, `key` the
/// coerced key string. Throws the Node `TypeError` when the definition would
/// violate an invariant; returns normally when the definition is permitted.
///
/// Rules (matching Node v25):
///   - Adding a NEW key to a non-extensible object:
///       `Cannot define property <k>, object is not extensible`
///   - Redefining an EXISTING non-configurable key (frozen, or sealed when
///     the descriptor changes more than a writable data value):
///       `Cannot redefine property: <k>`
///   - A sealed (but not frozen) object still allows rewriting an existing
///     writable data property's value, so that case is permitted.
unsafe fn enforce_define_property_invariants(
    obj: *mut ObjectHeader,
    key: *const crate::StringHeader,
    key_name: &str,
    descriptor_value: f64,
) {
    if obj.is_null() || (obj as usize) <= 0x10000 {
        return;
    }
    let gc = gc_header_for(obj);
    let flags = (*gc)._reserved;
    let frozen = flags & crate::gc::OBJ_FLAG_FROZEN != 0;
    let sealed = flags & crate::gc::OBJ_FLAG_SEALED != 0;
    let no_extend = flags & crate::gc::OBJ_FLAG_NO_EXTEND != 0;
    if !frozen && !sealed && !no_extend {
        return;
    }

    let exists = own_key_present(obj, key);

    if !exists {
        // Adding a new property to a non-extensible object always throws.
        if no_extend {
            throw_object_type_error_with_suffix(
                "Cannot define property ",
                &format!("{key_name}, object is not extensible"),
            );
        }
        return;
    }

    // Redefining an existing property. The property is non-configurable iff
    // the object is frozen or sealed (both drop `configurable` on every key).
    let attrs =
        get_property_attrs(obj as usize, key_name).unwrap_or(PropertyAttrs::new(true, true, true));
    if attrs.configurable() {
        return; // still configurable — redefinition allowed
    }

    // Non-configurable existing property. Node permits exactly one mutation:
    // changing the *value* of a still-writable data property (sealed-but-not-
    // frozen objects keep `writable`). Any attempt to change configurability,
    // enumerability, writability (to true), turn it into an accessor, or
    // write to a non-writable property is rejected with "Cannot redefine".
    let desc_ptr = extract_obj_ptr(descriptor_value);
    let is_accessor_desc = if desc_ptr.is_null() {
        false
    } else {
        let get_key = crate::string::js_string_from_bytes(b"get".as_ptr(), 3);
        let set_key = crate::string::js_string_from_bytes(b"set".as_ptr(), 3);
        own_key_present(desc_ptr, get_key) || own_key_present(desc_ptr, set_key)
    };

    let read_desc_bool = |name: &[u8]| -> Option<bool> {
        if desc_ptr.is_null() {
            return None;
        }
        let key = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
        if !own_key_present(desc_ptr, key) {
            return None;
        }
        let v = js_object_get_field_by_name(desc_ptr as *const ObjectHeader, key);
        Some(crate::value::js_is_truthy(f64::from_bits(v.bits())) != 0)
    };

    let wants_configurable = read_desc_bool(b"configurable").unwrap_or(false);
    let wants_writable = read_desc_bool(b"writable");
    let wants_enumerable = read_desc_bool(b"enumerable");

    // A bare value-only redefinition of a still-writable data property is the
    // only allowed mutation on a non-configurable property.
    let only_value_change = !is_accessor_desc
        && !wants_configurable
        && wants_enumerable
            .map(|e| e == attrs.enumerable())
            .unwrap_or(true)
        && wants_writable
            .map(|w| w == attrs.writable())
            .unwrap_or(true)
        && attrs.writable();
    if only_value_change {
        return;
    }

    throw_object_type_error_with_suffix("Cannot redefine property: ", key_name);
}

fn throw_from_entries_not_iterable() -> ! {
    throw_from_entries_type_error(b"undefined is not iterable")
}

fn throw_from_entries_non_object_entry() -> ! {
    throw_from_entries_type_error(b"Iterator value is not an entry object")
}

unsafe fn object_from_entries_gc_type(raw_ptr: i64) -> Option<u8> {
    if raw_ptr < crate::gc::GC_HEADER_SIZE as i64 + 0x1000 {
        return None;
    }
    let addr = raw_ptr as usize;
    if crate::symbol::is_registered_symbol(addr) {
        return None;
    }
    if crate::set::is_registered_set(addr) {
        return Some(crate::gc::GC_TYPE_SET);
    }
    if crate::map::is_registered_map(addr) {
        return Some(crate::gc::GC_TYPE_MAP);
    }
    let ptr = raw_ptr as *const u8;
    if !crate::object::is_valid_obj_ptr(ptr) {
        return None;
    }
    let gc_header = ptr.sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader;
    Some((*gc_header).obj_type)
}

unsafe fn object_from_entries_array_ptr(value: f64) -> *mut ArrayHeader {
    let raw = crate::value::js_nanbox_get_pointer(value);
    let gc_type = object_from_entries_gc_type(raw);
    if gc_type != Some(crate::gc::GC_TYPE_ARRAY) && gc_type != Some(crate::gc::GC_TYPE_LAZY_ARRAY) {
        throw_from_entries_not_iterable();
    }
    raw as *mut ArrayHeader
}

unsafe fn object_from_entries_has_iterator(value: f64, raw: i64, gc_type: Option<u8>) -> bool {
    let jv = crate::value::JSValue::from_bits(value.to_bits());
    if jv.is_any_string() {
        return true;
    }
    match gc_type {
        Some(crate::gc::GC_TYPE_ARRAY)
        | Some(crate::gc::GC_TYPE_LAZY_ARRAY)
        | Some(crate::gc::GC_TYPE_MAP)
        | Some(crate::gc::GC_TYPE_SET) => return true,
        Some(crate::gc::GC_TYPE_OBJECT) => {
            let obj = raw as *mut ObjectHeader;
            if crate::url::try_read_as_search_params(obj).is_some() {
                return true;
            }
            if !obj.is_null() && (*obj).class_id == crate::array::ARRAY_ITERATOR_CLASS_ID {
                return true;
            }
        }
        _ => {}
    }

    let iter_sym = crate::symbol::well_known_symbol("iterator");
    if !iter_sym.is_null() {
        let sym_value =
            f64::from_bits(crate::value::JSValue::pointer(iter_sym as *const u8).bits());
        let iter_fn = crate::symbol::js_object_get_symbol_property(value, sym_value);
        let iter_fn_ptr = crate::value::js_nanbox_get_pointer(iter_fn);
        if iter_fn_ptr != 0 && crate::closure::is_closure_ptr(iter_fn_ptr as usize) {
            return true;
        }
    }

    crate::array::has_iterator_next(value)
}

unsafe fn object_from_entries_materialize_entries(entries_value: f64) -> *mut ArrayHeader {
    let jv = crate::value::JSValue::from_bits(entries_value.to_bits());
    if jv.is_null() || jv.is_undefined() || jv.is_bool() || jv.is_number() || jv.is_int32() {
        throw_from_entries_not_iterable();
    }
    if jv.is_bigint() {
        throw_from_entries_not_iterable();
    }

    let raw = crate::value::js_nanbox_get_pointer(entries_value);
    let gc_type = object_from_entries_gc_type(raw);

    if !jv.is_any_string() && raw == 0 {
        throw_from_entries_not_iterable();
    }

    if !object_from_entries_has_iterator(entries_value, raw, gc_type) {
        throw_from_entries_not_iterable();
    }

    if gc_type == Some(crate::gc::GC_TYPE_MAP) {
        return crate::map::js_map_entries(raw as *const crate::map::MapHeader);
    }

    if gc_type == Some(crate::gc::GC_TYPE_OBJECT) {
        let obj = raw as *mut ObjectHeader;
        if crate::url::try_read_as_search_params(obj).is_some() {
            let boxed = crate::url::js_url_search_params_entries_arr(obj);
            return object_from_entries_array_ptr(boxed);
        }
    }

    let boxed = crate::array::js_for_of_to_array(entries_value);
    object_from_entries_array_ptr(boxed)
}

unsafe fn object_from_entries_entry_values(entry_val: f64) -> (f64, f64) {
    let jv = crate::value::JSValue::from_bits(entry_val.to_bits());
    if jv.is_null()
        || jv.is_undefined()
        || jv.is_bool()
        || jv.is_number()
        || jv.is_int32()
        || jv.is_any_string()
        || jv.is_bigint()
    {
        throw_from_entries_non_object_entry();
    }

    let raw = crate::value::js_nanbox_get_pointer(entry_val);
    let gc_type = object_from_entries_gc_type(raw);
    if raw == 0 {
        throw_from_entries_non_object_entry();
    }

    if gc_type == Some(crate::gc::GC_TYPE_ARRAY) || gc_type == Some(crate::gc::GC_TYPE_LAZY_ARRAY) {
        let arr = raw as *const ArrayHeader;
        return (
            crate::array::js_array_get_f64(arr, 0),
            crate::array::js_array_get_f64(arr, 1),
        );
    }

    let obj = raw as *const ObjectHeader;
    if obj.is_null() {
        throw_from_entries_non_object_entry();
    }
    let key0 = crate::string::js_string_from_bytes(b"0".as_ptr(), 1);
    let key1 = crate::string::js_string_from_bytes(b"1".as_ptr(), 1);
    (
        js_object_get_field_by_name_f64(obj, key0),
        js_object_get_field_by_name_f64(obj, key1),
    )
}

/// Object.fromEntries(entries) — build an object from iterable [key, value] entries.
#[no_mangle]
pub extern "C" fn js_object_from_entries(entries_value: f64) -> f64 {
    unsafe {
        let arr_ptr = object_from_entries_materialize_entries(entries_value);
        let length = crate::array::js_array_length(arr_ptr) as usize;

        // Allocate empty object — class_id 0 = generic object
        let obj = js_object_alloc(0, length as u32);
        if obj.is_null() {
            return f64::from_bits(crate::value::TAG_UNDEFINED);
        }

        for i in 0..length {
            let entry_val = crate::array::js_array_get_f64(arr_ptr, i as u32);
            let (key_val, val_val) = object_from_entries_entry_values(entry_val);
            let key_str = crate::builtins::js_string_coerce(key_val);
            if key_str.is_null() {
                continue;
            }
            js_object_set_field_by_name(obj, key_str, val_val);
        }

        crate::value::js_nanbox_pointer(obj as i64)
    }
}

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
            super::has_own_helpers::throw_to_object_nullish_type_error();
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
                super::has_own_helpers::string_primitive_own_key_present(obj_value, key_str);
            return f64::from_bits(if present { TAG_TRUE } else { TAG_FALSE });
        }

        if let Some(class_id) = super::class_ref_id(obj_value) {
            let present = super::has_own_helpers::str_from_string_header(key_str)
                .map(|key| {
                    if super::class_registry::class_is_key_deleted(class_id, key) {
                        false
                    } else if matches!(key, "length" | "prototype") {
                        true
                    } else if key == "name"
                        && super::class_registry::lookup_static_method_in_chain(class_id, key)
                            .is_none()
                    {
                        super::class_registry::class_name_for_id(class_id).is_some()
                    } else {
                        CLASS_DYNAMIC_PROPS.with(|m| {
                            m.borrow()
                                .get(&class_id)
                                .is_some_and(|props| props.contains_key(key))
                        }) || super::class_registry::lookup_static_method_in_chain(class_id, key)
                            .is_some()
                    }
                })
                .unwrap_or(false);
            return f64::from_bits(if present { TAG_TRUE } else { TAG_FALSE });
        }

        // #3655: functions/closures carry built-in own `name`/`length`
        // (and `prototype` for constructors) plus any user-attached props.
        // Route them here instead of through `extract_obj_ptr`/`own_key_present`,
        // which would read `keys_array` off a closure (out of bounds).
        if obj_js.is_pointer() {
            let ptr = obj_js.as_pointer::<u8>() as usize;
            if crate::closure::is_closure_ptr(ptr) {
                let present = super::has_own_helpers::str_from_string_header(key_str)
                    .map(|k| super::has_own_helpers::closure_own_key_present(ptr, k))
                    .unwrap_or(false);
                return f64::from_bits(if present { TAG_TRUE } else { TAG_FALSE });
            }
            if ptr >= crate::gc::GC_HEADER_SIZE + 0x1000
                && crate::object::is_valid_obj_ptr(ptr as *const u8)
            {
                let gc_header =
                    (ptr as *const u8).sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader;
                if (*gc_header).obj_type == crate::gc::GC_TYPE_ERROR {
                    let present = super::has_own_helpers::str_from_string_header(key_str)
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

        if (*obj).class_id == super::native_module::NATIVE_MODULE_CLASS_ID {
            let present = super::native_module::read_native_module_name(obj)
                .as_deref()
                .zip(super::has_own_helpers::str_from_string_header(key_str))
                .map(|(module, key)| {
                    super::native_module::native_module_has_enumerable_key(module, key)
                })
                .unwrap_or(false);
            return f64::from_bits(if present { TAG_TRUE } else { TAG_FALSE });
        }

        if (obj as usize) >= crate::gc::GC_HEADER_SIZE + 0x1000 {
            let gc_header =
                (obj as *const u8).sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader;
            if (*gc_header).obj_type == crate::gc::GC_TYPE_ARRAY {
                let present = super::has_own_helpers::array_own_key_present(
                    obj as *const crate::array::ArrayHeader,
                    key_str,
                );
                return f64::from_bits(if present { TAG_TRUE } else { TAG_FALSE });
            }
        }

        if (*obj).class_id == NATIVE_MODULE_CLASS_ID {
            let Some(key_name) = super::has_own_helpers::str_from_string_header(key_str) else {
                return f64::from_bits(TAG_FALSE);
            };
            let present = read_native_module_name(obj)
                .as_deref()
                .is_some_and(|module_name| native_module_has_enumerable_key(module_name, key_name));
            return f64::from_bits(if present { TAG_TRUE } else { TAG_FALSE });
        }

        if own_key_present(obj, key_str) {
            f64::from_bits(TAG_TRUE)
        } else {
            f64::from_bits(TAG_FALSE)
        }
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
            super::has_own_helpers::throw_to_object_nullish_type_error();
        }

        let key_str = crate::builtins::js_string_coerce(key_value);
        if key_str.is_null() {
            return f64::from_bits(TAG_FALSE);
        }

        // String primitives: index keys in range are enumerable own props;
        // "length" is a non-enumerable own prop; everything else absent.
        if obj_jv.is_any_string() {
            let present =
                super::has_own_helpers::string_primitive_own_key_present(obj_value, key_str);
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

        // #3655: functions/closures. Built-in `name`/`length`/`prototype` are
        // non-enumerable; user-attached props default to enumerable.
        if obj_jv.is_pointer() {
            let ptr = obj_jv.as_pointer::<u8>() as usize;
            if crate::closure::is_closure_ptr(ptr) {
                let Some(key_name) = super::has_own_helpers::str_from_string_header(key_str) else {
                    return f64::from_bits(TAG_FALSE);
                };
                if !super::has_own_helpers::closure_own_key_present(ptr, key_name) {
                    return f64::from_bits(TAG_FALSE);
                }
                if matches!(key_name, "name" | "length" | "prototype") {
                    return f64::from_bits(TAG_FALSE);
                }
                let enumerable = super::get_property_attrs(ptr, key_name)
                    .map(|attrs| attrs.enumerable())
                    .unwrap_or(true);
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
        if let Some(result) = super::array_property_is_enumerable(obj, key_str, key_name) {
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
        if !own_key_present(obj, key_str) {
            return f64::from_bits(TAG_FALSE);
        }
        let enumerable = super::get_property_attrs(obj as usize, key_name)
            .map(|attrs| attrs.enumerable())
            .unwrap_or(true);
        f64::from_bits(if enumerable { TAG_TRUE } else { TAG_FALSE })
    }
}

#[used]
static KEEP_PROPERTY_IS_ENUMERABLE: extern "C" fn(f64, f64) -> f64 =
    js_object_property_is_enumerable;

/// Helper: extract object pointer from NaN-boxed f64. Returns null on failure.
pub(crate) unsafe fn extract_obj_ptr(value: f64) -> *mut ObjectHeader {
    let jsval = crate::JSValue::from_bits(value.to_bits());
    if jsval.is_pointer() {
        jsval.as_pointer::<ObjectHeader>() as *mut ObjectHeader
    } else {
        let bits = value.to_bits();
        // Raw-I64-pointer fallback (module-level array/object vars store the
        // untagged pointer directly). Every GC allocation is `align.max(8)`-
        // aligned, so a real object pointer always has its low 3 bits clear.
        // Requiring alignment here rejects non-object values whose raw bits
        // merely *land* in the address range — e.g. a native-module namespace
        // sentinel (`require('buffer')`) reaching a generic object op like
        // `hasOwnProperty`. Without it, callers deref `[ptr-8]` for the
        // GcHeader on a misaligned garbage address → SIGBUS (#3527).
        if bits != 0 && bits <= 0x0000_FFFF_FFFF_FFFF && bits > 0x10000 && bits & 0x7 == 0 {
            bits as *mut ObjectHeader
        } else {
            ptr::null_mut()
        }
    }
}

/// Helper: get GcHeader for an object pointer
pub(super) unsafe fn gc_header_for(obj: *const ObjectHeader) -> *mut crate::gc::GcHeader {
    (obj as *mut u8).sub(crate::gc::GC_HEADER_SIZE) as *mut crate::gc::GcHeader
}

/// #2159 helper: install a `target_cid.method` entry from an
/// `Object.defineProperty(C.prototype, name, descriptor)` call.
///
/// The descriptor's `value` came in two main shapes in practice:
///
/// 1. A `BOUND_METHOD_FUNC_PTR` closure returned by `getOwnPropertyDescriptor`
///    on a sibling class (drizzle's `applyMixins(Base, [Mixin])`: the
///    `getOwnPropertyDescriptor(Mixin.prototype, name)` value reads as
///    `js_class_method_bind(Mixin_class_ref, name)`). Dispatching that bound
///    closure would re-enter `js_native_call_method` against the class-ref —
///    a class object reaches the *static* dispatch arm, not the instance
///    method, so calling it would return the wrong thing. Instead we look up
///    the raw vtable entry on the source class and copy it onto the target
///    class's vtable directly, so future `inst.method(args)` dispatches via
///    the regular chain walk with `this = inst`.
///
/// 2. A user-supplied closure (e.g. `Object.defineProperty(C.prototype, "m",
///    { value: function () { … } })`). Route through the same per-class
///    prototype-method side table that `js_register_prototype_method` (#838)
///    uses, so the `inst.m` / `inst.m()` lookup paths in
///    `field_get_set.rs` / `native_call_method.rs` find it after the regular
///    vtable miss.
unsafe fn define_class_prototype_method(target_cid: u32, name: &str, value_bits: u64) {
    use crate::closure::{ClosureHeader, BOUND_METHOD_FUNC_PTR, CLOSURE_MAGIC};
    use crate::object::class_registry::{ClassVTable, VTableMethodEntry, CLASS_VTABLE_REGISTRY};

    // Reject undefined / null / numeric values up front — those aren't
    // methods and shouldn't make it onto the prototype side tables.
    let value = f64::from_bits(value_bits);
    let jsv = crate::JSValue::from_bits(value_bits);
    if !jsv.is_pointer() {
        return;
    }
    let ptr = jsv.as_pointer::<u8>() as usize;
    if ptr < 0x1000 {
        return;
    }

    // Shape (1): BOUND_METHOD closure. Extract source class-ref + method
    // name from the captures (see `js_class_method_bind`), then copy the
    // source class's vtable entry (or any inherited entry up the parent
    // chain) onto `target_cid`.
    if crate::closure::is_closure_ptr(ptr) {
        let closure = ptr as *const ClosureHeader;
        if (*closure).type_tag == CLOSURE_MAGIC && (*closure).func_ptr == BOUND_METHOD_FUNC_PTR {
            let recv = crate::closure::js_closure_get_capture_f64(closure, 0);
            if let Some(source_cid) = super::class_ref_id(recv) {
                if let Some((func_ptr, param_count, has_synthetic_arguments)) =
                    super::lookup_class_method_in_chain(source_cid, name)
                {
                    let mut guard = CLASS_VTABLE_REGISTRY.write().unwrap();
                    if guard.is_none() {
                        *guard = Some(std::collections::HashMap::new());
                    }
                    let reg = guard.as_mut().unwrap();
                    let vtable = reg.entry(target_cid).or_insert_with(|| ClassVTable {
                        methods: std::collections::HashMap::new(),
                        getters: std::collections::HashMap::new(),
                        setters: std::collections::HashMap::new(),
                    });
                    vtable.methods.insert(
                        name.to_string(),
                        VTableMethodEntry {
                            func_ptr,
                            param_count,
                            has_synthetic_arguments,
                        },
                    );
                    drop(guard);
                    super::class_registry::js_register_class_id(target_cid);
                    crate::typed_feedback::invalidate_method_change(target_cid);
                    return;
                }
            }
        }
    }

    // Shape (2): any other callable value (user closure, regular function).
    // Mirror the `Class.prototype.method = fn` direct-assignment path so the
    // existing `lookup_prototype_method` walks find it.
    super::class_registry::js_register_prototype_method(
        target_cid,
        name.as_ptr(),
        name.len(),
        value,
    );
}

/// Object.defineProperty(obj, key, descriptor) — set the value AND record the
/// `writable` / `enumerable` / `configurable` attribute flags in the side table.
/// Returns the object (NaN-boxed pointer).
///
/// IMPORTANT: writes the value via `js_object_set_field_by_name` BEFORE recording
/// the descriptor — otherwise a `writable: false` descriptor would block its own
/// initial value from being stored.
#[no_mangle]
pub extern "C" fn js_object_define_property(
    obj_value: f64,
    key_value: f64,
    descriptor_value: f64,
) -> f64 {
    unsafe {
        // #2817: ES Object.defineProperty validation.
        //   1. Target must be an object (or class-ref / function — all objects
        //      in Node). Primitives / null / undefined throw.
        //   2. Descriptor must be an object; otherwise
        //      `Property description must be an object: <desc>`.
        //   3. Accessor + data fields can't be mixed.
        //   4. Present `get`/`set` must be callable.
        let target_is_class_ref = super::class_ref_id(obj_value).is_some();
        if !target_is_class_ref && !value_is_object_like(obj_value) {
            throw_object_type_error(b"Object.defineProperty called on non-object");
        }
        if !value_is_object_like(descriptor_value) {
            let desc = describe_value_for_type_error(descriptor_value);
            throw_object_type_error_with_suffix("Property description must be an object: ", &desc);
        }
        validate_property_descriptor(descriptor_value);

        // TypedArrays are Integer-Indexed exotic objects: a canonical numeric
        // index key bypasses ordinary define entirely (validate the index, then
        // either write the element or reject with a TypeError).
        match super::typed_array_define_own_property(obj_value, key_value, descriptor_value) {
            super::TypedArrayDefineOutcome::Defined => return obj_value,
            super::TypedArrayDefineOutcome::Rejected => {
                throw_object_type_error(b"Cannot redefine property")
            }
            super::TypedArrayDefineOutcome::NotTypedArray => {}
        }

        // #2159: when the receiver is a class-ref (`Class.prototype` evaluates
        // back to the class itself in Perry — see `class_ref_id` /
        // `js_object_get_own_property_descriptor`'s class-ref arm), route the
        // descriptor through the class-vtable / prototype-method side tables
        // so instance lookups (`new C().method`) see the new entry. Drizzle's
        // `applyMixins(Base, [Mixin])` copies methods between class
        // prototypes via `Object.defineProperty(Base.prototype, name,
        // Object.getOwnPropertyDescriptor(Mixin.prototype, name))` — pre-fix
        // the call hit `extract_obj_ptr → null` (a class-ref isn't a pointer)
        // and silently dropped the descriptor, so `await
        // db.select().from(x)` saw `instance.then === undefined` and `await`
        // unwrapped the builder unchanged.
        if let Some(target_cid) = super::class_ref_id(obj_value) {
            if let Some(name) = super::metadata_key_to_string(key_value) {
                let desc_ptr = extract_obj_ptr(descriptor_value);
                if !desc_ptr.is_null() {
                    let value_key = crate::string::js_string_from_bytes(b"value".as_ptr(), 5);
                    let value_field =
                        js_object_get_field_by_name(desc_ptr as *const ObjectHeader, value_key);
                    if !value_field.is_undefined() {
                        define_class_prototype_method(target_cid, &name, value_field.bits());
                    }
                }
            }
            return obj_value;
        }

        // Closures are object-like but not ObjectHeader-backed, so descriptor
        // writes have to route through the closure property side tables.
        let target_closure_ptr = {
            let value = crate::value::JSValue::from_bits(obj_value.to_bits());
            let raw = if value.is_pointer() {
                value.as_pointer::<u8>() as usize
            } else {
                let bits = obj_value.to_bits();
                if bits != 0 && bits <= 0x0000_FFFF_FFFF_FFFF && bits > 0x10000 {
                    bits as usize
                } else {
                    0
                }
            };
            if raw >= 0x10000 && crate::closure::is_closure_ptr(raw) {
                Some(raw)
            } else {
                None
            }
        };
        if let Some(closure_ptr) = target_closure_ptr {
            let key_str = crate::builtins::js_string_coerce(key_value);
            if key_str.is_null() {
                return obj_value;
            }
            let key_rust: Option<String> = {
                let name_ptr =
                    (key_str as *const u8).add(std::mem::size_of::<crate::StringHeader>());
                let name_len = (*key_str).byte_len as usize;
                let name_bytes = std::slice::from_raw_parts(name_ptr, name_len);
                std::str::from_utf8(name_bytes).ok().map(|s| s.to_string())
            };
            let Some(key_rust) = key_rust else {
                return obj_value;
            };
            let desc_ptr = extract_obj_ptr(descriptor_value);
            if desc_ptr.is_null() {
                return obj_value;
            }

            let get_key = crate::string::js_string_from_bytes(b"get".as_ptr(), 3);
            let set_key = crate::string::js_string_from_bytes(b"set".as_ptr(), 3);
            let get_field = js_object_get_field_by_name(desc_ptr as *const ObjectHeader, get_key);
            let set_field = js_object_get_field_by_name(desc_ptr as *const ObjectHeader, set_key);
            let has_accessor = !get_field.is_undefined() || !set_field.is_undefined();

            if has_accessor {
                let get_bits = if get_field.is_undefined() {
                    0
                } else {
                    crate::closure::clone_closure_rebind_this(get_field.bits(), obj_value)
                };
                let set_bits = if set_field.is_undefined() {
                    0
                } else {
                    crate::closure::clone_closure_rebind_this(set_field.bits(), obj_value)
                };
                set_accessor_descriptor(
                    closure_ptr,
                    key_rust.clone(),
                    AccessorDescriptor {
                        get: get_bits,
                        set: set_bits,
                    },
                );
            } else {
                let value_key = crate::string::js_string_from_bytes(b"value".as_ptr(), 5);
                let value_field =
                    js_object_get_field_by_name(desc_ptr as *const ObjectHeader, value_key);
                ACCESSOR_DESCRIPTORS.with(|m| {
                    m.borrow_mut().remove(&(closure_ptr, key_rust.clone()));
                });
                if !value_field.is_undefined() {
                    crate::closure::closure_set_dynamic_prop(
                        closure_ptr,
                        &key_rust,
                        f64::from_bits(value_field.bits()),
                    );
                }
            }

            let read_bool = |name: &[u8]| -> Option<bool> {
                let k = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
                let v = js_object_get_field_by_name(desc_ptr as *const ObjectHeader, k);
                if v.is_undefined() {
                    None
                } else {
                    Some(crate::value::js_is_truthy(f64::from_bits(v.bits())) != 0)
                }
            };
            let writable = read_bool(b"writable").unwrap_or(has_accessor);
            let enumerable = read_bool(b"enumerable").unwrap_or(false);
            let configurable = read_bool(b"configurable").unwrap_or(false);
            set_property_attrs(
                closure_ptr,
                key_rust,
                PropertyAttrs::new(writable, enumerable, configurable),
            );
            return obj_value;
        }

        let obj = extract_obj_ptr(obj_value);
        if obj.is_null() {
            return obj_value;
        }
        // #1250: when the key is a Symbol, route into the symbol side
        // table (`SYMBOL_PROPERTIES`) the same way `obj[sym] = value`
        // does. Without this, `Object.defineProperty(obj, sym, ...)`
        // would drop the symbol and try to coerce it to a string,
        // which is exactly the failure mode reported for
        // `Object.defineProperty(obj, inspect.custom, …)`.
        let key_bits = key_value.to_bits();
        let key_tag = key_bits & 0xFFFF_0000_0000_0000;
        if key_tag == 0x7FFD_0000_0000_0000 {
            let raw_ptr = (key_bits & 0x0000_FFFF_FFFF_FFFF) as *const crate::symbol::SymbolHeader;
            if !raw_ptr.is_null()
                && (raw_ptr as usize) >= 0x1000
                && (*raw_ptr).magic == crate::symbol::SYMBOL_MAGIC
            {
                let desc_ptr = extract_obj_ptr(descriptor_value);
                if !desc_ptr.is_null() {
                    let get_key = crate::string::js_string_from_bytes(b"get".as_ptr(), 3);
                    let set_key = crate::string::js_string_from_bytes(b"set".as_ptr(), 3);
                    let get_field =
                        js_object_get_field_by_name(desc_ptr as *const ObjectHeader, get_key);
                    let set_field =
                        js_object_get_field_by_name(desc_ptr as *const ObjectHeader, set_key);
                    let has_get = own_key_present(desc_ptr, get_key);
                    let has_set = own_key_present(desc_ptr, set_key);
                    let has_accessor = has_get || has_set;
                    if has_accessor {
                        let get_bits = if !has_get || get_field.is_undefined() {
                            0
                        } else {
                            crate::closure::clone_closure_rebind_this(get_field.bits(), obj_value)
                        };
                        let set_bits = if !has_set || set_field.is_undefined() {
                            0
                        } else {
                            crate::closure::clone_closure_rebind_this(set_field.bits(), obj_value)
                        };
                        crate::symbol::set_symbol_accessor_property(
                            obj_value, key_value, get_bits, set_bits,
                        );
                    } else {
                        let value_key = crate::string::js_string_from_bytes(b"value".as_ptr(), 5);
                        if own_key_present(desc_ptr, value_key) {
                            let value_field = js_object_get_field_by_name(
                                desc_ptr as *const ObjectHeader,
                                value_key,
                            );
                            crate::symbol::js_object_set_symbol_property(
                                obj_value,
                                key_value,
                                f64::from_bits(value_field.bits()),
                            );
                        }
                    }
                    let read_bool = |name: &[u8]| -> Option<bool> {
                        let k =
                            crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
                        let v = js_object_get_field_by_name(desc_ptr as *const ObjectHeader, k);
                        if v.is_undefined() {
                            None
                        } else {
                            Some(crate::value::js_is_truthy(f64::from_bits(v.bits())) != 0)
                        }
                    };
                    let writable = read_bool(b"writable").unwrap_or(has_accessor);
                    let enumerable = read_bool(b"enumerable").unwrap_or(false);
                    let configurable = read_bool(b"configurable").unwrap_or(false);
                    crate::symbol::set_symbol_property_attrs(
                        obj as usize,
                        raw_ptr as usize,
                        PropertyAttrs::new(writable, enumerable, configurable),
                    );
                }
                return obj_value;
            }
        }
        // Extract key string
        let key_str = crate::builtins::js_string_coerce(key_value);
        if key_str.is_null() {
            return obj_value;
        }
        // Extract the key as a Rust string for the descriptor side-table lookup.
        let key_rust: Option<String> = {
            let name_ptr = (key_str as *const u8).add(std::mem::size_of::<crate::StringHeader>());
            let name_len = (*key_str).byte_len as usize;
            let name_bytes = std::slice::from_raw_parts(name_ptr, name_len);
            std::str::from_utf8(name_bytes).ok().map(|s| s.to_string())
        };
        if let Some(result) = super::define_array_property(
            obj,
            obj_value,
            key_str,
            key_rust.as_deref(),
            descriptor_value,
        ) {
            return result;
        }
        // #2843: enforce frozen / sealed / non-extensible invariants BEFORE any
        // mutation, so a rejected definition leaves the object untouched and the
        // thrown TypeError matches Node.
        if let Some(ref k) = key_rust {
            enforce_define_property_invariants(obj, key_str, k, descriptor_value);
        }
        super::mark_object_dynamic_shape_unknown(obj);
        // Extract descriptor object
        let desc_ptr = extract_obj_ptr(descriptor_value);
        if desc_ptr.is_null() {
            return obj_value;
        }

        // Detect accessor descriptor (has `get` and/or `set`) vs. data descriptor (has `value`).
        // JS disallows mixing them, but we only check for `get`/`set` presence.
        let get_key = crate::string::js_string_from_bytes(b"get".as_ptr(), 3);
        let set_key = crate::string::js_string_from_bytes(b"set".as_ptr(), 3);
        let get_field = js_object_get_field_by_name(desc_ptr as *const ObjectHeader, get_key);
        let set_field = js_object_get_field_by_name(desc_ptr as *const ObjectHeader, set_key);
        let has_accessor = !get_field.is_undefined() || !set_field.is_undefined();

        if has_accessor {
            // Store the accessor closures in the side table. Ensure the key is present
            // in the object's keys_array so lookups (hasOwn, getOwnPropertyDescriptor,
            // keys) can see it.
            ensure_key_in_keys_array(obj, key_str);
            if let Some(k) = key_rust.clone() {
                // Issue #450: spec says the getter/setter runs with `this === obj`
                // (the property access target). The user's descriptor literal
                // `{ get() {...}, set() {...} }` was lowered with `captures_this: true`
                // and had its reserved `this` slot patched to point to the *descriptor*
                // object at construction time — that's what every other object-literal
                // method does. Clone the closure once at defineProperty time and
                // rebind `this` to `obj`, so every subsequent get/set call sees the
                // correct receiver. Closures without CAPTURES_THIS_FLAG (e.g. arrow-form
                // `get: () => this._backing` written as a field rather than a method
                // shorthand) pass through unchanged.
                let recv_box = crate::value::js_nanbox_pointer(obj as i64);
                let get_bits = if get_field.is_undefined() {
                    0u64
                } else {
                    crate::closure::clone_closure_rebind_this(get_field.bits(), recv_box)
                };
                let set_bits = if set_field.is_undefined() {
                    0u64
                } else {
                    crate::closure::clone_closure_rebind_this(set_field.bits(), recv_box)
                };
                set_accessor_descriptor(
                    obj as usize,
                    k,
                    AccessorDescriptor {
                        get: get_bits,
                        set: set_bits,
                    },
                );
            }
        } else {
            // Data descriptor: look for "value" field and store it.
            let value_key = crate::string::js_string_from_bytes(b"value".as_ptr(), 5);
            let value_field =
                js_object_get_field_by_name(desc_ptr as *const ObjectHeader, value_key);
            // Clear any existing accessor for this key so the write doesn't fire the setter.
            if let Some(ref k) = key_rust {
                ACCESSOR_DESCRIPTORS.with(|m| {
                    m.borrow_mut().remove(&(obj as usize, k.clone()));
                });
            }
            // Ensure the key exists even if the descriptor's value is undefined —
            // the property still "exists" per JS semantics.
            if value_field.is_undefined() {
                ensure_key_in_keys_array(obj, key_str);
            } else {
                // Store via runtime path. Any existing descriptor attrs are NOT yet set,
                // so writability defaults to true and the write goes through.
                js_object_set_field_by_name(obj, key_str, f64::from_bits(value_field.bits()));
            }
        }

        // Read attribute flags from descriptor. JS defaults when omitted in
        // `Object.defineProperty` are `false` (NOT `true` like for direct assignment).
        let read_bool = |name: &[u8]| -> Option<bool> {
            let k = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
            let v = js_object_get_field_by_name(desc_ptr as *const ObjectHeader, k);
            if v.is_undefined() {
                None
            } else {
                Some(crate::value::js_is_truthy(f64::from_bits(v.bits())) != 0)
            }
        };
        // Accessor descriptors don't have `writable`; we leave it true so data
        // lookups that happen before the accessor override don't accidentally
        // reject a legitimate fallthrough write. Attrs default to false when
        // omitted (JS spec).
        let writable = read_bool(b"writable").unwrap_or(has_accessor);
        let enumerable = read_bool(b"enumerable").unwrap_or(false);
        let configurable = read_bool(b"configurable").unwrap_or(false);

        if let Some(k) = key_rust {
            set_property_attrs(
                obj as usize,
                k,
                PropertyAttrs::new(writable, enumerable, configurable),
            );
        }
        super::arguments_object_after_define(obj, key_str, descriptor_value);
        // Return the object
        obj_value
    }
}

/// Ensure a key appears in the object's keys_array. Used by `Object.defineProperty`
/// so the property is enumerable-filterable and discoverable by `getOwnPropertyNames`
/// even when the value is undefined or the property is an accessor (no underlying slot).
#[allow(unused_assignments)]
pub(crate) unsafe fn ensure_key_in_keys_array(
    obj: *mut ObjectHeader,
    key: *const crate::StringHeader,
) {
    if obj.is_null() || (obj as usize) < 0x10000 || key.is_null() {
        return;
    }
    let scope = crate::gc::RuntimeHandleScope::new();
    let obj_handle = scope.root_raw_mut_ptr(obj);
    let key_handle = scope.root_string_ptr(key);
    let mut obj = obj_handle.get_raw_mut_ptr::<ObjectHeader>();
    let mut key = key_handle.get_raw_const_ptr::<crate::StringHeader>();
    macro_rules! refresh_define_property_roots {
        () => {{
            obj = obj_handle.get_raw_mut_ptr::<ObjectHeader>();
            key = key_handle.get_raw_const_ptr::<crate::StringHeader>();
        }};
    }
    // If no keys array exists, create one with this key.
    let keys = (*obj).keys_array;
    if keys.is_null() {
        let new_keys = crate::array::js_array_alloc(4);
        refresh_define_property_roots!();
        let new_keys = crate::array::js_array_push(new_keys, JSValue::string_ptr(key as *mut _));
        refresh_define_property_roots!();
        set_object_keys_array(obj, new_keys);
        if (*obj).field_count == 0 {
            (*obj).field_count = 1;
        }
        return;
    }
    // Validate keys array pointer. The bare high-bits/low-address checks let
    // through values that are non-null and tag-free yet still not real heap
    // pointers (e.g. a stray `0x20_0000_0203` left in a miscompiled object's
    // keys_array slot), which then fault inside `js_array_length`'s GC-header
    // read. Gate on the arena-bounds predicate (same one `js_object_create`
    // uses for prototype validation) so a garbage slot is treated as "no keys
    // array" instead of crashing the process. (#321: defends against the
    // Effect `makeGenericTag` mis-tagged-receiver corruption.)
    let keys_ptr = keys as usize;
    if (keys_ptr as u64) >> 48 != 0 || keys_ptr < 0x10000 || !is_valid_obj_ptr(keys as *const u8) {
        return;
    }
    // Check if key already exists
    let key_count = crate::array::js_array_length(keys) as usize;
    for i in 0..key_count {
        let stored = crate::array::js_array_get(keys, i as u32);
        // #1781: SSO-aware match — pre-fix an existing inline-SSO key
        // wasn't seen here, so `Object.defineProperty(obj, "id", ...)`
        // on an object that already had `id` as an SSO key
        // double-inserted instead of overwriting.
        if crate::string::js_string_key_matches(stored, key) {
            return; // already present
        }
    }
    // Clone shared keys array if needed, then append.
    let owned_keys = if key_count == (*obj).field_count as usize {
        let cloned = crate::array::js_array_alloc(key_count as u32 + 4);
        refresh_define_property_roots!();
        let keys = (*obj).keys_array;
        let src_data = (keys as *const u8).add(8) as *const f64;
        let dst_data = (cloned as *mut u8).add(8) as *mut f64;
        for i in 0..key_count {
            // GC_STORE_AUDIT(INIT): cloned keys array is unpublished; layout is rebuilt before publication.
            *dst_data.add(i) = *src_data.add(i);
        }
        (*cloned).length = key_count as u32;
        super::rebuild_array_layout_from_slots(cloned);
        set_object_keys_array(obj, cloned);
        cloned
    } else {
        keys
    };
    let owned_keys_handle = scope.root_raw_mut_ptr(owned_keys);
    let new_keys = crate::array::js_array_push(owned_keys, JSValue::string_ptr(key as *mut _));
    let _owned_keys = owned_keys_handle.get_raw_mut_ptr::<ArrayHeader>();
    refresh_define_property_roots!();
    set_object_keys_array(obj, new_keys);
    // `field_count` is the inline/overflow boundary consulted by the read path
    // (`js_object_get_field`: index < field_count ⇒ read inline slot, else the
    // overflow map). It must never exceed the object's physically-allocated
    // inline capacity, which is `max(field_count, 8)` (see `js_object_alloc`).
    // Only bump it when this key genuinely lands in an in-bounds inline slot.
    //
    // A keys-only entry — a built-in accessor like `Map.prototype.size`, or a
    // key whose data spilled to the overflow map — must NOT push field_count
    // past the inline region. Doing so reclassifies already-overflowed (or
    // out-of-bounds) slots as inline, so later reads dereference past the
    // allocation into adjacent-heap garbage. That is what made
    // `Map.prototype.set` / `.values` read back as raw non-pointer values and
    // crash the reflective `.call` dispatch (#4099): installing the `size`
    // getter here bumped field_count from 8 (the proto's physical capacity) to
    // 11, exposing the overflowed `values` slot and corrupting the boundary.
    let new_index = key_count as u32;
    let inline_capacity = std::cmp::max((*obj).field_count, 8);
    if new_index < inline_capacity && new_index >= (*obj).field_count {
        (*obj).field_count = new_index + 1;
    }
}

/// Install a built-in *getter-only* accessor on a prototype object so that
/// `Object.getOwnPropertyDescriptor(proto, key)` reflects it as a real
/// accessor descriptor `{ get, set: undefined, enumerable, configurable }`.
///
/// `getter_bits` is the NaN-boxed `f64` bits of the getter closure (0 = none).
/// The descriptor is non-enumerable and configurable, matching the ECMA-262
/// shape for `%TypedArray%.prototype` accessors like `length` / `byteLength` /
/// `byteOffset` / `buffer`. Reflection-only: this does NOT flip the hot-path
/// descriptor gate (see `set_builtin_accessor_descriptor`). #2060.
pub(crate) unsafe fn install_builtin_getter(proto: *mut ObjectHeader, key: &str, getter_bits: u64) {
    if proto.is_null() || (proto as usize) < 0x10000 {
        return;
    }
    let key_str = crate::string::js_string_from_bytes(key.as_ptr(), key.len() as u32);
    if key_str.is_null() {
        return;
    }
    // Make the key discoverable by `own_key_present` / `getOwnPropertyNames`.
    ensure_key_in_keys_array(proto, key_str);
    set_builtin_accessor_descriptor(
        proto as usize,
        key.to_string(),
        AccessorDescriptor {
            get: getter_bits,
            set: 0,
        },
        // writable is N/A for an accessor; enumerable=false, configurable=true.
        PropertyAttrs::new(true, false, true),
    );
}

/// Helper: does `key` appear in `obj.keys_array`?
pub(crate) unsafe fn own_key_present(
    obj: *mut ObjectHeader,
    key: *const crate::StringHeader,
) -> bool {
    // Every GC allocation is `align.max(8)`-aligned, so a real object pointer
    // has its low 3 bits clear. Rejecting misaligned `obj` keeps a non-object
    // value (e.g. a native-module namespace sentinel reaching `hasOwnProperty`
    // via a caller that didn't route through `extract_obj_ptr`) from being
    // dereferenced as an ObjectHeader. (#3527)
    if obj.is_null() || (obj as usize) < 0x10000 || (obj as usize) & 0x7 != 0 || key.is_null() {
        return false;
    }
    let keys = (*obj).keys_array;
    if keys.is_null() {
        return false;
    }
    let keys_ptr = keys as usize;
    // Same alignment invariant for the keys_array pointer: when `obj` is not a
    // genuine object its `keys_array` field holds garbage that may land in the
    // address range yet be misaligned. Without this guard the `[keys-8]`
    // GcHeader read below SIGBUSes on that garbage. (#3527)
    if (keys_ptr as u64) >> 48 != 0 || keys_ptr < 0x10000 || keys_ptr & 0x7 != 0 {
        return false;
    }
    // Validate keys_array GC header
    let keys_gc = (keys as *const u8).sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader;
    if (*keys_gc).obj_type != crate::gc::GC_TYPE_ARRAY {
        return false;
    }
    let key_count = crate::array::js_array_length(keys) as usize;
    if key_count > 65536 {
        return false;
    }
    for i in 0..key_count {
        let stored = crate::array::js_array_get(keys, i as u32);
        // #1781: SSO-aware match — `hasOwnProperty("id")` previously
        // returned false when "id" lived as an inline SSO key.
        if crate::string::js_string_key_matches(stored, key) {
            return true;
        }
    }
    false
}

/// Issue #620: returns the OWN-property value at `name` if one exists in the
/// receiver's own keys_array (a string-keyed data property), otherwise
/// returns TAG_UNDEFINED. Used by class-method dispatch to detect override
/// patterns like `this.method = X` (hono's SmartRouter.match rebinds itself
/// on first call). Distinct from `js_object_get_field_by_name` because it
/// does NOT walk the class vtable's getter chain — we only want a raw own
/// data-property read, not a side-effecting getter invocation.
#[no_mangle]
pub extern "C" fn js_object_get_own_field_or_undef(
    obj_value: f64,
    name_ptr: *const u8,
    name_len: usize,
) -> f64 {
    const TAG_UNDEF: u64 = 0x7FFC_0000_0000_0001;
    if name_ptr.is_null() {
        return f64::from_bits(TAG_UNDEF);
    }
    unsafe {
        let obj = extract_obj_ptr(obj_value);
        if obj.is_null() || (obj as usize) < 0x10000 {
            return f64::from_bits(TAG_UNDEF);
        }
        if (obj as usize) < crate::gc::GC_HEADER_SIZE + 0x1000 {
            return f64::from_bits(TAG_UNDEF);
        }
        if !is_valid_obj_ptr(obj as *const u8) {
            return f64::from_bits(TAG_UNDEF);
        }
        let gc_header =
            (obj as *const u8).sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader;
        if (*gc_header).obj_type != crate::gc::GC_TYPE_OBJECT {
            return f64::from_bits(TAG_UNDEF);
        }
        // Skip closures sharing the GC_TYPE_OBJECT slot (CLOSURE_MAGIC at +12).
        let type_tag_at_12 = *((obj as *const u8).add(12) as *const u32);
        if type_tag_at_12 == crate::closure::CLOSURE_MAGIC {
            return f64::from_bits(TAG_UNDEF);
        }
        let keys = (*obj).keys_array;
        if keys.is_null() {
            return f64::from_bits(TAG_UNDEF);
        }
        let keys_ptr = keys as usize;
        if (keys_ptr as u64) >> 48 != 0 || keys_ptr < 0x10000 {
            return f64::from_bits(TAG_UNDEF);
        }
        let keys_gc =
            (keys as *const u8).sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader;
        if (*keys_gc).obj_type != crate::gc::GC_TYPE_ARRAY {
            return f64::from_bits(TAG_UNDEF);
        }
        let key_bytes = std::slice::from_raw_parts(name_ptr, name_len);
        let key_count = crate::array::js_array_length(keys) as usize;
        if key_count > 65536 {
            return f64::from_bits(TAG_UNDEF);
        }
        let alloc_limit = std::cmp::max((*obj).field_count, 8) as usize;
        for i in 0..key_count {
            let key_val = crate::array::js_array_get(keys, i as u32);
            // #1781: SSO-aware match by byte slice — the
            // own-property-or-undef path was the route through which
            // hono's `c.req.X` dispatch decided to invoke the vtable
            // getter, and pre-fix a SSO-stored `X` was invisible here.
            if crate::string::js_string_key_matches_bytes(key_val, key_bytes) {
                let val = if i < alloc_limit {
                    js_object_get_field(obj, i as u32)
                } else {
                    match overflow_get(obj as usize, i) {
                        Some(bits) => crate::JSValue::from_bits(bits),
                        None => return f64::from_bits(TAG_UNDEF),
                    }
                };
                return f64::from_bits(val.bits());
            }
        }
        f64::from_bits(TAG_UNDEF)
    }
}

/// Look up the canonical NaN-boxed value of a built-in constructor /
/// namespace stored on `globalThis` (the singleton populated by
/// `populate_global_this_builtins`). Used by `instance.constructor`
/// reads and by bare `Date`/`Array`/`Object` identifier resolution so
/// both forms produce the same closure-pointer value — that's what
/// `instance.constructor === Date` (date-fns's `constructFrom`,
/// drizzle's `is(value, ctor)` duck checks, ...) hinges on.
///
/// Returns NaN-boxed undefined if the name isn't one of the populated
/// built-ins or the singleton hasn't been initialized yet.
#[no_mangle]
pub extern "C" fn js_get_global_this_builtin_value(name_ptr: *const u8, name_len: usize) -> f64 {
    if name_ptr.is_null() || name_len == 0 {
        return f64::from_bits(crate::value::TAG_UNDEFINED);
    }
    let name_bytes = unsafe { std::slice::from_raw_parts(name_ptr, name_len) };
    let name = match std::str::from_utf8(name_bytes) {
        Ok(s) => s,
        Err(_) => return f64::from_bits(crate::value::TAG_UNDEFINED),
    };
    // Force the singleton init the first time so the lookup below has
    // a populated field bag.
    let global_this_f64 = js_get_global_this();
    let global_obj = crate::value::js_nanbox_get_pointer(global_this_f64) as *const ObjectHeader;
    if global_obj.is_null() {
        return f64::from_bits(crate::value::TAG_UNDEFINED);
    }
    let key = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
    let value = js_object_get_field_by_name(global_obj, key);
    let bits = value.bits();
    f64::from_bits(bits)
}

/// Object.create(proto) — create empty object. Perry ignores prototype; Object.create(null) returns {}.
#[no_mangle]
pub extern "C" fn js_object_create(proto_value: f64) -> f64 {
    // #809: actually wire up the prototype. Pre-fix this ignored its
    // argument entirely, so `Object.create(Proto)` returned a bare empty
    // object — `inst.method()` / `inst.prop` saw nothing and threw
    // `TypeError: <m> is not a function`. Reuse the #711 prototype-object
    // machinery: allocate a synthetic class_id, map it to `proto` in
    // CLASS_PROTOTYPE_OBJECTS, and stamp the new object with that id. The
    // chain walk in `js_object_get_field_by_name` (the `class_id != 0`
    // branch) then resolves missing own props/methods off `proto`.
    //
    // `Object.create(null)` (or a non-object proto / a builtin-backed
    // Set/Map/Regex source Perry can't model as a prototype) falls back
    // to the original behavior: a plain prototype-less object.
    const POINTER_TAG: u64 = 0x7FFD_0000_0000_0000;
    let mut class_id: u32 = 0;
    let proto_bits = proto_value.to_bits();
    if (proto_bits & 0xFFFF_0000_0000_0000) == POINTER_TAG {
        let proto_ptr = crate::value::js_nanbox_get_pointer(proto_value) as *mut ObjectHeader;
        if !proto_ptr.is_null() && (proto_ptr as usize) > 0x10000 {
            let proto_addr = proto_ptr as usize;
            let modellable = !(crate::set::is_registered_set(proto_addr)
                || crate::map::is_registered_map(proto_addr)
                || crate::regex::is_regex_pointer(proto_ptr as *const u8));
            let valid = modellable && is_valid_obj_ptr(proto_ptr as *const u8);
            if valid {
                let cid =
                    NEXT_SYNTHETIC_CLASS_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                class_prototype_object_root_store(cid, proto_ptr);
                unsafe { js_register_class_id(cid) };
                // #1805: link the synthetic class_id into the original class's
                // inheritance chain. `Object.getPrototypeOf(instance)` returns
                // the instance pointer itself in Perry's model (see
                // `js_object_get_prototype_of`), so `proto_ptr` here is a real
                // class instance whose `class_id` field IS the user class's
                // id. Registering it as the synthetic cid's parent lets
                // `js_instanceof`'s `get_parent_class_id` walk reach the
                // original class and match — without this, the chain stopped
                // at the unregistered synthetic id and `Object.create(proto)
                // instanceof C` was always false even though property /
                // getter dispatch through the chain worked correctly.
                let parent_class_id = unsafe { (*proto_ptr).class_id };
                if parent_class_id != 0 && parent_class_id != cid {
                    register_class(cid, parent_class_id);
                }
                class_id = cid;
            }
        }
    }
    // #1175: when `proto_value` is null/undefined/non-object, the resulting
    // object has no [[Prototype]]. Stamp OBJ_FLAG_NULL_PROTO so
    // `Object.getPrototypeOf(Object.create(null))` returns null (it
    // previously returned the object itself).
    let null_proto = class_id == 0;
    let obj = if null_proto {
        js_object_alloc_null_proto(class_id, 0)
    } else {
        js_object_alloc(class_id, 0)
    };
    // Return NaN-boxed pointer
    f64::from_bits((obj as u64) | 0x7FFD_0000_0000_0000)
}

/// Object.getPrototypeOf(obj):
/// - For an INT32-tagged class ref (top16 == 0x7FFE) — return the parent
///   class ref via CLASS_REGISTRY's parent_class_id chain, or null at
///   the root. Drizzle's `is(value, type)` chain walks this.
/// - For an object instance with a registered class_id — return the
///   class ref. Conceptually JS returns `Class.prototype`; Perry doesn't
///   maintain prototype objects, but drizzle's chain consumes
///   `Object.getPrototypeOf(value).constructor`, and class_ref's
///   `.constructor` synthesizes back to the same class ref via the
///   constructor intercept (v0.5.746). So returning the class ref here
///   makes that chain produce `value.constructor` as Node would.
/// - Other receivers — null.
/// Refs #420 / #618 followup.
#[no_mangle]
pub extern "C" fn js_object_get_prototype_of(obj_value: f64) -> f64 {
    const TAG_NULL: u64 = 0x7FFC_0000_0000_0002;
    // #2820: `Object.getPrototypeOf(null | undefined)` throws TypeError
    // (`Cannot convert undefined or null to object`). Class refs and heap
    // objects fall through to the existing resolution below.
    {
        let jv = crate::value::JSValue::from_bits(obj_value.to_bits());
        if jv.is_null() || jv.is_undefined() {
            throw_object_type_error(b"Cannot convert undefined or null to object");
        }
    }
    let bits = obj_value.to_bits();
    let top16 = bits >> 48;
    if top16 == 0x7FFD {
        let raw_addr = bits & 0x0000_FFFF_FFFF_FFFF;
        if raw_addr > 0 && raw_addr < 0x100000 {
            if let Some(dispatch) = super::class_registry::handle_prototype_dispatch() {
                let proto = unsafe { dispatch(raw_addr as i64) };
                if proto.to_bits() != crate::value::TAG_UNDEFINED {
                    return proto;
                }
            }
            return f64::from_bits(TAG_NULL);
        }
    }
    let collection_prototype = |addr: usize| -> Option<f64> {
        if crate::map::is_registered_map(addr) {
            let proto = crate::object::builtin_prototype_value("Map");
            if proto.to_bits() != crate::value::TAG_UNDEFINED {
                return Some(proto);
            }
        }
        if crate::set::is_registered_set(addr) {
            let proto = crate::object::builtin_prototype_value("Set");
            if proto.to_bits() != crate::value::TAG_UNDEFINED {
                return Some(proto);
            }
        }
        None
    };
    let function_prototype_or_null = || {
        let proto = crate::object::builtin_prototype_value("Function");
        if proto.to_bits() != crate::value::TAG_UNDEFINED {
            proto
        } else {
            f64::from_bits(TAG_NULL)
        }
    };
    if top16 == 0x7FFE {
        let class_id = (bits & 0xFFFF_FFFF) as u32;
        if let Some(parent_id) = get_parent_class_id(class_id) {
            if parent_id != 0 {
                let parent_bits = 0x7FFE_0000_0000_0000u64 | (parent_id as u64);
                return f64::from_bits(parent_bits);
            }
        }
        return f64::from_bits(TAG_NULL);
    }
    // Heap-pointer receiver — return the input value itself. For
    // class-id-tagged instances, `.constructor` then returns the class
    // ref (via the constructor intercept in js_object_get_field_by_name,
    // v0.5.746), making `getPrototypeOf(v).constructor === v.constructor`.
    // For object literals / arrays / other non-class-tagged heap values,
    // `.constructor` returns undefined, which collapses drizzle's
    // `if (cls)` chain to false safely (instead of throwing on
    // `null.constructor` if we returned null). Drizzle's
    // `is(value, type)` chain calls this on every chunk including
    // arrays of values, so the array case is load-bearing.
    //
    // Two NaN-shapes cover the heap-pointer case:
    //  - top16 == 0x7FFD: NaN-boxed POINTER_TAG (typical function-local).
    //  - top16 == 0x0000 with raw_addr large enough: module-level object
    //    literals get stored as raw I64 pointers (no NaN-boxing) per the
    //    "Module-level variables" note in CLAUDE.md, so we accept that
    //    form here too.
    if top16 == 0x7FFD {
        let raw_addr = bits & 0x0000_FFFF_FFFF_FFFF;
        if raw_addr != 0 && raw_addr >= (crate::gc::GC_HEADER_SIZE as u64) + 0x1000 {
            if let Some(proto) = collection_prototype(raw_addr as usize) {
                return proto;
            }
            // #2820: an explicit `Object.setPrototypeOf(obj, proto)` recorded
            // in the side-table takes precedence — return exactly what was set
            // (including `null`).
            if let Some(proto_bits) =
                super::prototype_chain::object_static_prototype(raw_addr as usize)
            {
                return f64::from_bits(proto_bits);
            }
            unsafe {
                let obj = raw_addr as *const ObjectHeader;
                let gc = gc_header_for(obj);
                // #1175: objects allocated with a null prototype
                // (Object.create(null), querystring.parse) report null here.
                if (*gc)._reserved & crate::gc::OBJ_FLAG_NULL_PROTO != 0 {
                    return f64::from_bits(TAG_NULL);
                }
                // #2145: per-kind typed-array `.prototype` objects share a
                // single `%TypedArray%.prototype` parent. Resolved off the
                // cached intrinsic pointer (also a GC root) so the chain holds
                // through copying GC.
                if (*gc)._reserved & crate::gc::OBJ_FLAG_TYPED_ARRAY_PROTO != 0 {
                    let p = crate::object::typed_array_intrinsic_proto_ptr();
                    if !p.is_null() {
                        return f64::from_bits(crate::value::js_nanbox_pointer(p as i64).to_bits());
                    }
                }
                if (*gc).obj_type == crate::gc::GC_TYPE_ERROR {
                    let err = raw_addr as *const crate::error::ErrorHeader;
                    if let Some(proto) = error_kind_prototype_value((*err).error_kind) {
                        return proto;
                    }
                }
                if (*gc).obj_type == crate::gc::GC_TYPE_ARRAY {
                    if let Some(proto) = super::array_get_prototype_of_addr(raw_addr as usize) {
                        return proto;
                    }
                }
                // #489 / #2145: a function/constructor receiver has no
                // walkable [[Prototype]] in Perry's model UNLESS its
                // closure-static-prototype side-table has been set
                // (`Object.setPrototypeOf(closure, parent)` — effect's
                // TagClass and Perry's `%TypedArray%`-chain typed-array
                // constructors use this). Returning the recorded parent
                // satisfies drizzle's `cls = getPrototypeOf(cls)` walk
                // (which terminates when the parent has no further
                // recorded proto) and the test262 `__proto__` chain. When
                // no static prototype is recorded, return null to break
                // the would-be `getPrototypeOf(cls) === cls` self-cycle.
                if (*gc).obj_type == crate::gc::GC_TYPE_CLOSURE {
                    if let Some(proto_bits) =
                        crate::closure::closure_static_prototype(raw_addr as usize)
                    {
                        return f64::from_bits(proto_bits);
                    }
                    // #3664: a generator/async-generator function's
                    // [[Prototype]] is `%Generator%` / `%AsyncGenerator%`.
                    if let Some(proto) =
                        crate::object::generator_function_proto_of(raw_addr as usize)
                    {
                        return proto;
                    }
                    return function_prototype_or_null();
                }
                if let Some(proto) = constructor_dynamic_prototype(obj) {
                    return proto;
                }
                if (*gc).obj_type == crate::gc::GC_TYPE_OBJECT
                    && ((*obj).class_id == 0 || is_anon_shape_class_id((*obj).class_id))
                {
                    if let Some(proto_bits) =
                        super::prototype_chain::default_object_prototype_for_owner(
                            raw_addr as usize,
                        )
                    {
                        return f64::from_bits(proto_bits);
                    }
                    return f64::from_bits(TAG_NULL);
                }
                // Built-in iterator instances (Array/Map/Set/String iterators)
                // share a `%...IteratorPrototype%` singleton. Their instances
                // normally carry it as a recorded static prototype (returned
                // above), but resolve by class id too so the chain holds even if
                // the static-prototype side-table entry was dropped.
                if (*gc).obj_type == crate::gc::GC_TYPE_OBJECT {
                    if let Some(proto) = super::iterator_prototype_for_class_id((*obj).class_id) {
                        return proto;
                    }
                }
            }
            return obj_value;
        }
    }
    if top16 == 0 {
        if bits >= (crate::gc::GC_HEADER_SIZE as u64) + 0x1000 {
            if let Some(proto) = collection_prototype(bits as usize) {
                return proto;
            }
            // #2820: explicit setPrototypeOf side-table takes precedence.
            if let Some(proto_bits) = super::prototype_chain::object_static_prototype(bits as usize)
            {
                return f64::from_bits(proto_bits);
            }
            unsafe {
                let obj = bits as *const ObjectHeader;
                let gc = gc_header_for(obj);
                if (*gc)._reserved & crate::gc::OBJ_FLAG_NULL_PROTO != 0 {
                    return f64::from_bits(TAG_NULL);
                }
                if (*gc)._reserved & crate::gc::OBJ_FLAG_TYPED_ARRAY_PROTO != 0 {
                    let p = crate::object::typed_array_intrinsic_proto_ptr();
                    if !p.is_null() {
                        return f64::from_bits(crate::value::js_nanbox_pointer(p as i64).to_bits());
                    }
                }
                if (*gc).obj_type == crate::gc::GC_TYPE_ERROR {
                    let err = bits as *const crate::error::ErrorHeader;
                    if let Some(proto) = error_kind_prototype_value((*err).error_kind) {
                        return proto;
                    }
                }
                if (*gc).obj_type == crate::gc::GC_TYPE_ARRAY {
                    if let Some(proto) = super::array_get_prototype_of_addr(bits as usize) {
                        return proto;
                    }
                }
                // #489 / #2145: function/constructor receiver — see the
                // 0x7FFD branch above. Return the recorded static
                // prototype if any, else null to break the chain-walk
                // self-cycle.
                if (*gc).obj_type == crate::gc::GC_TYPE_CLOSURE {
                    if let Some(proto_bits) =
                        crate::closure::closure_static_prototype(bits as usize)
                    {
                        return f64::from_bits(proto_bits);
                    }
                    // #3664: generator/async-generator [[Prototype]] resolution.
                    if let Some(proto) = crate::object::generator_function_proto_of(bits as usize) {
                        return proto;
                    }
                    return function_prototype_or_null();
                }
                if let Some(proto) = constructor_dynamic_prototype(obj) {
                    return proto;
                }
                if (*gc).obj_type == crate::gc::GC_TYPE_OBJECT
                    && ((*obj).class_id == 0 || is_anon_shape_class_id((*obj).class_id))
                {
                    if let Some(proto_bits) =
                        super::prototype_chain::default_object_prototype_for_owner(bits as usize)
                    {
                        return f64::from_bits(proto_bits);
                    }
                    return f64::from_bits(TAG_NULL);
                }
                if (*gc).obj_type == crate::gc::GC_TYPE_OBJECT {
                    if let Some(proto) = super::iterator_prototype_for_class_id((*obj).class_id) {
                        return proto;
                    }
                }
            }
            return obj_value;
        }
    }
    f64::from_bits(TAG_NULL)
}

/// `Object.defineProperties(target, descriptors)` — iterate the descriptor
/// object's own keys and invoke `js_object_define_property` for each one.
/// Used by chalk's `Object.defineProperties(createChalk.prototype, styles)`
/// where `styles` is built via `Object.create(null)` + dynamic assignment,
/// so the static `Object(...)` literal desugar in the HIR lowering can't
/// fire and we fall here.
///
/// Returns the target. Spec also returns target — Perry's lowering relies
/// on that so `const x = Object.defineProperties(...)` still binds `x`.
#[no_mangle]
pub extern "C" fn js_object_define_properties(target: f64, descriptors: f64) -> f64 {
    // #2817: target must be an object (or class-ref). Node throws
    // `Object.defineProperties called on non-object` for primitives.
    let target_is_class_ref = super::class_ref_id(target).is_some();
    if !target_is_class_ref && !unsafe { value_is_object_like(target) } {
        throw_object_type_error(b"Object.defineProperties called on non-object");
    }
    // #2817: the properties bag must be coercible to an object. Node throws
    // `Cannot convert undefined or null to object` for null/undefined, and
    // primitives are boxed (no own enumerable keys → no-op). Match the nullish
    // case explicitly.
    {
        let jv = crate::value::JSValue::from_bits(descriptors.to_bits());
        if jv.is_undefined() || jv.is_null() {
            throw_object_type_error(b"Cannot convert undefined or null to object");
        }
    }
    let desc_obj = unsafe { extract_obj_ptr(descriptors) };
    if desc_obj.is_null() || !is_valid_obj_ptr(desc_obj as *const u8) {
        return target;
    }
    // Snapshot the descriptor object's own keys array. We collect into a
    // Vec<f64> first so adding properties via `js_object_define_property`
    // (which can resize the target's keys_array) can't perturb iteration
    // — descriptors and target are usually different objects, but a
    // defensive copy costs ~ngc and protects against a user who passes
    // `Object.defineProperties(obj, obj)` aliasing.
    let key_array_ptr: *const crate::array::ArrayHeader = unsafe { (*desc_obj).keys_array };
    if key_array_ptr.is_null() {
        return target;
    }
    let len = unsafe { crate::array::js_array_length(key_array_ptr) } as usize;
    let mut keys: Vec<f64> = Vec::with_capacity(len);
    for i in 0..len {
        let k = unsafe { crate::array::js_array_get(key_array_ptr, i as u32) };
        keys.push(f64::from_bits(k.bits()));
    }
    for k in keys {
        let descriptor = unsafe {
            js_object_get_field_by_name_f64(desc_obj as *const ObjectHeader, str_from_value(k))
        };
        if descriptor.to_bits() == TAG_UNDEFINED_LOCAL {
            continue;
        }
        js_object_define_property(target, k, descriptor);
    }
    target
}

const TAG_UNDEFINED_LOCAL: u64 = 0x7FFC_0000_0000_0001;

/// Coerce an arbitrary key value (f64 — usually a STRING_TAG NaN-box) to a
/// `*const StringHeader` for use with `js_object_get_field_by_name_f64`.
/// Returns null if the value isn't string-like.
fn str_from_value(v: f64) -> *const crate::string::StringHeader {
    let bits = v.to_bits();
    let top = bits >> 48;
    if top == 0x7FFF {
        (bits & 0x0000_FFFF_FFFF_FFFF) as *const crate::string::StringHeader
    } else {
        // Try to coerce (handles number keys, etc.).
        crate::builtins::js_string_coerce(v) as *const crate::string::StringHeader
    }
}

/// `Object.setPrototypeOf(obj, proto)` — chalk's callable-with-getter-bag
/// foundation. Perry's runtime bakes class IDs at allocation time (it
/// walks `parent_class_id` for INT32-tagged class refs), so we cannot
/// mutate an existing object's prototype chain in a fully observable
/// way. What we *can* do is satisfy the spec's "return target" contract
/// so callers like
///
/// ```text
/// const chalk = (...s) => s.join(' ');
/// Object.setPrototypeOf(chalk, Foo.prototype);
/// ```
///
/// don't crash with `TypeError: value is not a function` (which is what
/// the generic `(Object).setPrototypeOf(...)` PropertyGet → Call fallback
/// used to produce — the property lookup returned undefined and the call
/// dispatched a non-callable). chalk's module init invokes this exact
/// pattern; ms / express decorate functions with `Object.assign` instead,
/// which is already a fast path.
///
/// Pragmatically: today this returns the target and otherwise no-ops.
/// chalk's getters on `createChalk.prototype` won't actually fire under
/// Perry, but the rest of the program keeps running and chalk's
/// call-without-properties form (the most common usage) keeps working.
/// A future change can register the (obj → proto) mapping in a
/// thread-local side-table so a downstream `Object.getPrototypeOf(obj)`
/// + inherited property dispatch can consult it.
#[no_mangle]
pub extern "C" fn js_object_set_prototype_of(obj_value: f64, proto: f64) -> f64 {
    const TAG_NULL: u64 = 0x7FFC_0000_0000_0002;
    const POINTER_TAG: u64 = 0x7FFD_0000_0000_0000;
    let obj_bits = obj_value.to_bits();
    let proto_bits = proto.to_bits();

    // #2820: `Object.setPrototypeOf(null | undefined, proto)` throws
    // `TypeError: Object.setPrototypeOf called on null or undefined`.
    {
        let jv = crate::value::JSValue::from_bits(obj_bits);
        if jv.is_null() || jv.is_undefined() {
            throw_object_type_error(b"Object.setPrototypeOf called on null or undefined");
        }
    }

    // #2820: `proto` must be an object or `null`. A primitive / undefined proto
    // throws `TypeError: Object prototype may only be an Object or null`.
    let proto_is_null = proto_bits == TAG_NULL;
    let proto_ok = proto_is_null
        || unsafe { value_is_object_like(proto) }
        || super::class_ref_id(proto).is_some();
    if !proto_ok {
        throw_object_type_error(b"Object prototype may only be an Object or null");
    }

    // #2820: setting the prototype of a primitive target is a spec no-op that
    // returns the (boxed) primitive value. `value_is_object_like` is false for
    // numbers/strings/booleans, and class refs are handled by the recording
    // path below — so a non-object, non-closure target just returns unchanged.
    let obj_ptr_for_record = {
        let top = obj_bits >> 48;
        if top == 0x7FFD {
            (obj_bits & 0x0000_FFFF_FFFF_FFFF) as usize
        } else if top == 0 && obj_bits > 0x10000 {
            obj_bits as usize
        } else {
            0
        }
    };

    // #36 / #321: when the target is a closure (a plain function value) and the
    // proto is an object, record the (closure → proto) link in the closure
    // static-prototype side-table. effect's `Context.Tag(id)` returns a
    // function `TagClass` whose `_op`/`[TagTypeId]`/`[EffectTypeId]` live on a
    // `TagProto` object wired in via `Object.setPrototypeOf(TagClass,
    // TagProto)`. Recording the link lets later string/symbol property reads on
    // the closure (and on a subclass that `extends TagClass`) walk to the
    // proto's own properties, so the Tag is recognized as a valid Effect.
    if (obj_bits & 0xFFFF_0000_0000_0000) == POINTER_TAG
        && (proto_bits & 0xFFFF_0000_0000_0000) == POINTER_TAG
    {
        let obj_ptr = crate::value::js_nanbox_get_pointer(obj_value) as usize;
        let proto_ptr = crate::value::js_nanbox_get_pointer(proto) as usize;
        if obj_ptr != 0 && proto_ptr != 0 && crate::closure::is_closure_ptr(obj_ptr) {
            crate::closure::closure_set_static_prototype(obj_ptr, proto_bits);
            return obj_value;
        }
    }

    // #2820: ordinary heap object — record the observable [[Prototype]] in the
    // object-prototype side-table so `Object.getPrototypeOf(obj)` and inherited
    // property reads (`obj.x` where `x` lives on `proto`) reflect it. Records
    // `TAG_NULL` for `setPrototypeOf(obj, null)`.
    if obj_ptr_for_record != 0
        && !crate::closure::is_closure_ptr(obj_ptr_for_record)
        && is_valid_obj_ptr(obj_ptr_for_record as *const u8)
    {
        super::prototype_chain::object_set_static_prototype(obj_ptr_for_record, proto_bits);
    }

    // Spec: `Object.setPrototypeOf(O, proto)` returns O.
    obj_value
}
