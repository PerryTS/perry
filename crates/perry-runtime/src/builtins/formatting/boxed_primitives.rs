use super::*;

thread_local! {
    static BOXED_PRIMITIVE_PAYLOADS: std::cell::RefCell<crate::fast_hash::PtrHashMap<usize, u64>> =
        std::cell::RefCell::new(crate::fast_hash::new_ptr_hash_map());
}

const CLASS_ID_BOXED_NUMBER: u32 = 0xFFFF_00D0;
const CLASS_ID_BOXED_STRING: u32 = 0xFFFF_00D1;
const CLASS_ID_BOXED_BOOLEAN: u32 = 0xFFFF_00D2;
const CLASS_ID_BOXED_BIGINT: u32 = 0xFFFF_00D3;
const CLASS_ID_BOXED_SYMBOL: u32 = 0xFFFF_00D4;

pub(super) unsafe fn boxed_primitive_base_for_object(
    obj_ptr: *const crate::object::ObjectHeader,
) -> Option<String> {
    let (class_id, payload) = boxed_primitive_payload_for_object(obj_ptr)?;
    match class_id {
        CLASS_ID_BOXED_STRING => {
            let s = jsvalue_string_content(payload).unwrap_or_default();
            Some(format!("[String: '{}']", escape_string(&s)))
        }
        CLASS_ID_BOXED_NUMBER => Some(format!("[Number: {}]", format_util_number(payload))),
        CLASS_ID_BOXED_BOOLEAN => {
            let value = crate::value::JSValue::from_bits(payload.to_bits());
            Some(format!(
                "[Boolean: {}]",
                if value.is_bool() && value.as_bool() {
                    "true"
                } else {
                    "false"
                }
            ))
        }
        _ => None,
    }
}

/// For a boxed `String` wrapper (`new String("abc")`), the integer-index own
/// properties `"0".."len-1"` mirror the underlying characters. Node's
/// `util.inspect` treats those as the wrapped primitive (shown via the
/// `[String: '…']` base) and never lists them in the `{ … }` body — only extra
/// own keys appear there. Returns the wrapped string's length for a boxed
/// String, otherwise `None`.
///
/// The count is in UTF-16 code units, NOT Unicode scalar values: the index
/// properties are installed over `0..js_string_length` (`utf16_len`) by
/// `install_string_wrapper_indices`, so a non-BMP char (e.g. an emoji, two
/// UTF-16 units) occupies two indices. Counting `.chars()` would under-count
/// and leak a trailing index (e.g. `new String("a😀b")` → `{ 3: 'b' }`).
pub(super) unsafe fn boxed_string_char_index_count(
    obj_ptr: *const crate::object::ObjectHeader,
) -> Option<usize> {
    let (class_id, payload) = boxed_primitive_payload_for_object(obj_ptr)?;
    if class_id != CLASS_ID_BOXED_STRING {
        return None;
    }
    Some(
        jsvalue_string_content(payload)
            .unwrap_or_default()
            .encode_utf16()
            .count(),
    )
}

unsafe fn boxed_primitive_payload_for_object(
    obj_ptr: *const crate::object::ObjectHeader,
) -> Option<(u32, f64)> {
    if !crate::value::addr_class::is_plausible_heap_addr(obj_ptr as usize) {
        return None;
    }
    let class_id = (*obj_ptr).class_id;
    if !matches!(
        class_id,
        CLASS_ID_BOXED_NUMBER
            | CLASS_ID_BOXED_STRING
            | CLASS_ID_BOXED_BOOLEAN
            | CLASS_ID_BOXED_BIGINT
            | CLASS_ID_BOXED_SYMBOL
    ) {
        return None;
    }
    let ptr_key = obj_ptr as usize;
    let payload = BOXED_PRIMITIVE_PAYLOADS.with(|m| {
        m.borrow()
            .get(&ptr_key)
            .copied()
            .map(f64::from_bits)
            .unwrap_or_else(|| crate::object::js_object_get_field_f64(obj_ptr, 0))
    });
    Some((class_id, payload))
}

fn register_boxed_primitive_payload(obj: *mut crate::object::ObjectHeader, payload: f64) {
    if obj.is_null() {
        return;
    }
    BOXED_PRIMITIVE_PAYLOADS.with(|m| {
        m.borrow_mut().insert(obj as usize, payload.to_bits());
    });
}

fn boxed_constructor_name(class_id: u32) -> Option<&'static [u8]> {
    match class_id {
        CLASS_ID_BOXED_NUMBER => Some(b"Number"),
        CLASS_ID_BOXED_STRING => Some(b"String"),
        CLASS_ID_BOXED_BOOLEAN => Some(b"Boolean"),
        CLASS_ID_BOXED_BIGINT => Some(b"BigInt"),
        CLASS_ID_BOXED_SYMBOL => Some(b"Symbol"),
        _ => None,
    }
}

fn attach_boxed_primitive_prototype(obj: *mut crate::object::ObjectHeader, class_id: u32) {
    if obj.is_null() {
        return;
    }
    let Some(name) = boxed_constructor_name(class_id) else {
        return;
    };
    let ctor = crate::object::js_get_global_this_builtin_value(name.as_ptr(), name.len());
    let ctor_value = crate::value::JSValue::from_bits(ctor.to_bits());
    if !ctor_value.is_pointer() {
        return;
    }
    let ctor_ptr = ctor_value.as_pointer::<crate::closure::ClosureHeader>() as usize;
    let proto = crate::closure::closure_get_dynamic_prop(ctor_ptr, "prototype");
    let proto_value = crate::value::JSValue::from_bits(proto.to_bits());
    if proto_value.is_pointer() {
        crate::object::prototype_chain::object_set_static_prototype(obj as usize, proto.to_bits());
    }
}

fn install_string_wrapper_length(
    obj: *mut crate::object::ObjectHeader,
    string_ptr: *const crate::string::StringHeader,
) {
    if obj.is_null() || string_ptr.is_null() {
        return;
    }
    let key = crate::string::js_string_from_bytes(b"length".as_ptr(), 6);
    let len = crate::string::js_string_length(string_ptr) as f64;
    crate::object::js_object_set_field_by_name(obj, key, len);
    crate::object::set_builtin_property_attrs(
        obj as usize,
        "length".to_string(),
        crate::object::PropertyAttrs::new(false, false, false),
    );
}

/// String exotic objects (ECMA-262 §10.4.3) expose each UTF-16 code unit as an
/// integer-indexed own property `"0".."len-1"` with the descriptor
/// `{ value: <char>, writable: false, enumerable: true, configurable: false }`.
/// `new String("abc")` therefore reports `getOwnPropertyDescriptor(s, "0")`,
/// `s.hasOwnProperty("0")`, and `Object.keys(s)`/enumeration over the indices.
/// Installed eagerly at construction (typical `new String` receivers are
/// short); the wrapper's `length` is installed separately and stays last.
fn install_string_wrapper_indices(
    obj: *mut crate::object::ObjectHeader,
    string_ptr: *const crate::string::StringHeader,
) {
    if obj.is_null() || string_ptr.is_null() {
        return;
    }
    let len = crate::string::js_string_length(string_ptr);
    for i in 0..len {
        let ch = crate::string::js_string_char_at(string_ptr, i as i32);
        if ch.is_null() {
            continue;
        }
        let name = i.to_string();
        let key = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
        let ch_value = f64::from_bits(crate::value::JSValue::string_ptr(ch).bits());
        crate::object::js_object_set_field_by_name(obj, key, ch_value);
        crate::object::set_builtin_property_attrs(
            obj as usize,
            name,
            crate::object::PropertyAttrs::new(false, true, false),
        );
    }
}

pub fn scan_boxed_primitive_payload_roots_mut(visitor: &mut crate::gc::RuntimeRootVisitor<'_>) {
    let mut moved = Vec::new();
    BOXED_PRIMITIVE_PAYLOADS.with(|m| {
        let mut m = m.borrow_mut();
        for (&owner, payload_bits) in m.iter_mut() {
            let mut new_owner = owner;
            if visitor.visit_metadata_usize_slot(&mut new_owner) {
                moved.push((owner, new_owner));
            }
            visitor.visit_nanbox_u64_slot(payload_bits);
        }
        for (old_owner, new_owner) in moved.drain(..) {
            if let Some(payload) = m.remove(&old_owner) {
                m.insert(new_owner, payload);
            }
        }
    });
}

/// #3857: `JSON.stringify` of a boxed primitive wrapper (`new String("hi")`,
/// `new Number(5)`, `new Boolean(true)`, `Object(1n)`) serializes the
/// *underlying primitive*, not the wrapper object's (empty) own-property set.
///
/// Per ECMA-262 §25.5.2.2 step 4:
/// - Boolean: use [[BooleanData]] directly (stored payload).
/// - Number: call ToNumber(wrapper) — honours a custom `valueOf` on the object.
/// - String: call ToString(wrapper) — honours a custom `toString` on the object.
/// - BigInt: throws `TypeError` ("Do not know how to serialize a BigInt") —
///   some serializer paths (e.g. `write_replaced_scalar`) write raw BigInts as
///   plain strings rather than throwing, so we must reject here proactively.
/// - Symbol/non-wrapper: return `None`; callers continue with normal
///   scalar/object dispatch, which omits primitive Symbols but still handles
///   boxed Symbol objects through the object path.
#[inline]
pub(crate) fn boxed_primitive_json_value(value: f64) -> Option<f64> {
    let (class_id, payload) = boxed_primitive_payload(value)?;
    match class_id {
        CLASS_ID_BOXED_BOOLEAN => Some(payload),
        CLASS_ID_BOXED_BIGINT => {
            crate::collection_iter::throw_type_error("Do not know how to serialize a BigInt");
        }
        CLASS_ID_BOXED_NUMBER => {
            // ToNumber(wrapper): honours a custom `valueOf` on the instance.
            Some(js_number_coerce(value))
        }
        CLASS_ID_BOXED_STRING => {
            // ToString(wrapper): honours a custom `toString` on the instance.
            let s_ptr = crate::value::js_jsvalue_to_string(value);
            Some(f64::from_bits(
                crate::value::JSValue::string_ptr(s_ptr).bits(),
            ))
        }
        _ => None,
    }
}

#[inline]
pub(crate) fn boxed_primitive_payload(value: f64) -> Option<(u32, f64)> {
    let jv = crate::value::JSValue::from_bits(value.to_bits());
    let bits = value.to_bits();
    let ptr = if jv.is_pointer() {
        jv.as_pointer::<crate::object::ObjectHeader>() as *mut crate::object::ObjectHeader
    } else if (bits >> 48) == 0 && crate::value::addr_class::is_above_handle_band(bits as usize) {
        bits as *mut crate::object::ObjectHeader
    } else {
        return None;
    };
    // This is a defensive type-probe over arbitrary `f64` bits, so a candidate
    // that isn't a real heap object must be rejected *before* the `class_id`
    // read — otherwise a small subnormal double (e.g. raw bits `0x2800000207`)
    // that slips through the raw-pointer heuristic above is dereferenced as an
    // `ObjectHeader` and faults. `is_plausible_heap_addr` keeps the
    // small-handle floor (the fetch/Headers id-space lives below it and
    // `is_valid_obj_ptr`'s Linux `HEAP_MIN` of `0x1000` would otherwise let
    // those handles through) and additionally gates on the real heap range
    // (#4099).
    if !crate::value::addr_class::is_plausible_heap_addr(ptr as usize) {
        return None;
    }
    unsafe { boxed_primitive_payload_for_object(ptr) }
}

pub(crate) fn boxed_primitive_to_string_tag(value: f64) -> Option<&'static str> {
    let (class_id, _) = boxed_primitive_payload(value)?;
    match class_id {
        CLASS_ID_BOXED_NUMBER => Some("Number"),
        CLASS_ID_BOXED_STRING => Some("String"),
        CLASS_ID_BOXED_BOOLEAN => Some("Boolean"),
        CLASS_ID_BOXED_BIGINT => Some("BigInt"),
        CLASS_ID_BOXED_SYMBOL => Some("Symbol"),
        _ => None,
    }
}

#[no_mangle]
pub extern "C" fn js_boxed_number_new(value: f64) -> f64 {
    let obj = crate::object::js_object_alloc(CLASS_ID_BOXED_NUMBER, 0);
    // `new Number()` (no args) is spec'd to box +0, not NaN. js_number_coerce
    // would map undefined to NaN, so detect the missing-arg sentinel first.
    let payload = if crate::value::JSValue::from_bits(value.to_bits()).is_undefined() {
        0.0
    } else {
        js_number_coerce(value)
    };
    register_boxed_primitive_payload(obj, payload);
    attach_boxed_primitive_prototype(obj, CLASS_ID_BOXED_NUMBER);
    crate::value::js_nanbox_pointer(obj as i64)
}

/// `has_arg` is an explicit compile-time flag (codegen passes 0/1 based on
/// whether `new String(...)` was given an argument), NOT an inference from
/// `value.is_undefined()` — a *present* argument that evaluates to
/// `undefined` at runtime (`new String(x)` where `x` holds `undefined`) must
/// still box to `"undefined"`, distinct from the no-arg `""` default. Both
/// would otherwise collapse to the same NaN-boxed `undefined` bits and be
/// indistinguishable here.
#[no_mangle]
pub extern "C" fn js_boxed_string_new(value: f64, has_arg: i32) -> f64 {
    let obj = crate::object::js_object_alloc(CLASS_ID_BOXED_STRING, 0);
    // `new String()` (no args) is spec'd to box "", not "undefined".
    let ptr = if has_arg == 0 {
        crate::string::js_string_from_bytes(std::ptr::null(), 0)
    } else {
        // ECMA-262 §22.1.1 step 2b: ToString(value) — throws TypeError for Symbol.
        if unsafe { crate::symbol::js_is_symbol(value) } != 0 {
            crate::collection_iter::throw_type_error("Cannot convert a Symbol value to a string");
        }
        js_string_coerce(value)
    };
    let boxed = f64::from_bits(crate::value::JSValue::string_ptr(ptr).bits());
    register_boxed_primitive_payload(obj, boxed);
    install_string_wrapper_indices(obj, ptr);
    install_string_wrapper_length(obj, ptr);
    attach_boxed_primitive_prototype(obj, CLASS_ID_BOXED_STRING);
    crate::value::js_nanbox_pointer(obj as i64)
}

#[no_mangle]
pub extern "C" fn js_boxed_boolean_new(value: f64) -> f64 {
    let obj = crate::object::js_object_alloc(CLASS_ID_BOXED_BOOLEAN, 0);
    let boxed =
        f64::from_bits(crate::value::JSValue::bool(crate::value::js_is_truthy(value) != 0).bits());
    register_boxed_primitive_payload(obj, boxed);
    attach_boxed_primitive_prototype(obj, CLASS_ID_BOXED_BOOLEAN);
    crate::value::js_nanbox_pointer(obj as i64)
}

#[no_mangle]
pub extern "C" fn js_boxed_bigint_new(value: f64) -> f64 {
    let obj = crate::object::js_object_alloc(CLASS_ID_BOXED_BIGINT, 0);
    register_boxed_primitive_payload(obj, value);
    attach_boxed_primitive_prototype(obj, CLASS_ID_BOXED_BIGINT);
    crate::value::js_nanbox_pointer(obj as i64)
}

#[no_mangle]
pub extern "C" fn js_boxed_symbol_new(value: f64) -> f64 {
    let obj = crate::object::js_object_alloc(CLASS_ID_BOXED_SYMBOL, 0);
    register_boxed_primitive_payload(obj, value);
    attach_boxed_primitive_prototype(obj, CLASS_ID_BOXED_SYMBOL);
    crate::value::js_nanbox_pointer(obj as i64)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn boxed_primitive_probe_rejects_pointer_tagged_native_handles() {
        let fetch_family_handle = crate::value::js_nanbox_pointer(0x40001);
        assert!(boxed_primitive_payload(fetch_family_handle).is_none());
    }
}
