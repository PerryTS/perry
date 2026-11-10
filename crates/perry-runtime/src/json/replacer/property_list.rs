use super::*;

// ─── Array replacer (key whitelist) stringify ────────────────────────────────

pub(crate) unsafe fn stringify_object_with_array_replacer(
    ptr: *const u8,
    allowed_keys: &[String],
    buf: &mut String,
    indent: &str,
    depth: usize,
    use_pretty: bool,
) {
    check_stringify_nesting_depth(depth);
    let ptr = super::super::stringify_object::refresh_environment_receiver(ptr);
    // Circular reference check
    if STRINGIFY_STACK.with(|s| s.borrow().contains(&(ptr as usize))) {
        let msg = "Converting circular structure to JSON";
        let msg_ptr = js_string_from_bytes(msg.as_ptr(), msg.len() as u32);
        // Use js_typeerror_new so error_kind == ERROR_KIND_TYPE_ERROR and
        // `e instanceof TypeError` returns true (matching Node).
        crate::exception::throw_core(crate::error::CoreError::type_error(msg_ptr));
    }
    STRINGIFY_STACK.with(|s| s.borrow_mut().push(ptr as usize));

    let scope = crate::gc::RuntimeHandleScope::new();
    let object = scope.root_raw_const_ptr(ptr as *const crate::ObjectHeader);
    if object
        .with_const_ptr(|obj| super::super::stringify::object_keys_array_checked(obj).is_none())
    {
        // Native cells without ObjectHeader storage have no ordinary members.
        buf.push_str("{}");
        STRINGIFY_STACK.with(|s| s.borrow_mut().pop());
        return;
    }

    buf.push('{');
    let mut first = true;
    for allowed_key in allowed_keys {
        let member_scope = crate::gc::RuntimeHandleScope::new();
        let key =
            member_scope.root_raw_const_ptr(crate::string::canonical_key(allowed_key.as_bytes()));
        // PropertyList determines which Get operations run, and their order.
        // Read each value immediately before serializing it: an earlier getter
        // or nested serialization can change a later property or move storage.
        let field_val = key.with_const_ptr(|key: *const StringHeader| {
            object.with_const_ptr(|obj: *const crate::ObjectHeader| {
                if crate::object::instance_private_key_hidden(
                    obj,
                    JSValue::string_ptr(key.cast_mut()),
                ) {
                    JSValue::undefined()
                } else {
                    crate::object::js_object_get_field_by_name(obj, key)
                }
            })
        });
        let field_val = member_scope.root_nanbox_f64(f64::from_bits(field_val.bits()));
        let field_bits = field_val.get_nanbox_f64().to_bits();
        if field_bits == TAG_UNDEFINED || is_closure_value(field_bits) {
            continue;
        }
        if !first {
            buf.push(',');
        }
        first = false;
        if use_pretty {
            buf.push('\n');
            let inner_indent_count = depth + 1;
            for _ in 0..inner_indent_count {
                buf.push_str(indent);
            }
            write_escaped_string(buf, allowed_key);
            buf.push_str(": ");
            stringify_value_with_array_replacer(
                field_val.get_nanbox_f64(),
                allowed_keys,
                buf,
                indent,
                inner_indent_count,
                true,
            );
        } else {
            write_escaped_string(buf, allowed_key);
            buf.push(':');
            stringify_value_with_array_replacer(
                field_val.get_nanbox_f64(),
                allowed_keys,
                buf,
                indent,
                depth,
                false,
            );
        }
    }
    if use_pretty && !first {
        buf.push('\n');
        for _ in 0..depth {
            buf.push_str(indent);
        }
    }
    buf.push('}');
    STRINGIFY_STACK.with(|s| s.borrow_mut().pop());
}

/// Serialize one value while an array replacer (PropertyList) is active. Per
/// spec the key whitelist applies to EVERY nested object in the tree, not just
/// the top-level one — so a plain object recurses through
/// `stringify_object_with_array_replacer` and an array through
/// `stringify_array_with_array_replacer`. Scalars, boxed primitive wrappers,
/// buffers, Map/Set, etc. defer to the ordinary (whitelist-agnostic) serializer.
pub(crate) unsafe fn stringify_value_with_array_replacer(
    val: f64,
    allowed_keys: &[String],
    buf: &mut String,
    indent: &str,
    depth: usize,
    use_pretty: bool,
) {
    let bits = val.to_bits();
    if let Some(ptr) = extract_pointer(bits) {
        if super::super::stringify_proxy::try_stringify(
            val,
            buf,
            indent,
            depth,
            super::super::stringify_proxy::Replacer::Keys(allowed_keys),
            false,
        ) {
            return;
        }
        if !crate::buffer::is_registered_buffer(ptr as usize)
            && crate::builtins::boxed_primitive_json_value(val).is_none()
        {
            match gc_obj_type(ptr) {
                crate::gc::GC_TYPE_ARRAY => {
                    stringify_array_with_array_replacer(
                        ptr,
                        allowed_keys,
                        buf,
                        indent,
                        depth,
                        use_pretty,
                    );
                    return;
                }
                crate::gc_object_type_pat!() if is_object_pointer(ptr) => {
                    stringify_object_with_array_replacer(
                        ptr,
                        allowed_keys,
                        buf,
                        indent,
                        depth,
                        use_pretty,
                    );
                    return;
                }
                _ => {}
            }
        }
    }
    if use_pretty {
        stringify_value_pretty(val, TYPE_UNKNOWN, buf, indent, depth);
    } else {
        stringify_value(val, TYPE_UNKNOWN, buf);
    }
}

/// Array walk under an active array replacer. Every element is serialized (the
/// PropertyList only filters object keys, never array indices); undefined /
/// function elements become `null`. Nested objects/arrays recurse carrying the
/// same key whitelist.
pub(crate) unsafe fn stringify_array_with_array_replacer(
    ptr: *const u8,
    allowed_keys: &[String],
    buf: &mut String,
    indent: &str,
    depth: usize,
    use_pretty: bool,
) {
    check_stringify_nesting_depth(depth);
    // #5989: follow GC_FLAG_FORWARDED array-growth stubs (`js_array_grow`, issue
    // #233) so a stale pre-grow pointer serializes the current grown array —
    // mirrors the resolution in `dispatch_pointer_with_replacer`'s array arm.
    let ptr = crate::array::clean_arr_ptr(ptr as *const crate::ArrayHeader) as *const u8;
    if ptr.is_null() {
        buf.push_str("null");
        return;
    }
    if STRINGIFY_STACK.with(|s| s.borrow().contains(&(ptr as usize))) {
        let msg = "Converting circular structure to JSON";
        let msg_ptr = js_string_from_bytes(msg.as_ptr(), msg.len() as u32);
        crate::exception::throw_core(crate::error::CoreError::type_error(msg_ptr));
    }
    STRINGIFY_STACK.with(|s| s.borrow_mut().push(ptr as usize));

    let arr = ptr as *const crate::ArrayHeader;
    let len = (*arr).length;
    let elements = crate::array::array_elements_ptr(arr as *const crate::ArrayHeader) as *const f64;
    if len == 0 {
        buf.push_str("[]");
        STRINGIFY_STACK.with(|s| s.borrow_mut().pop());
        return;
    }
    let inner_depth = depth + 1;
    buf.push('[');
    for i in 0..len {
        if i > 0 {
            buf.push(',');
        }
        if use_pretty {
            buf.push('\n');
            for _ in 0..inner_depth {
                buf.push_str(indent);
            }
        }
        let elem = *elements.add(i as usize);
        let elem_bits = elem.to_bits();
        // TAG_HOLE: sparse-array holes serialize as null, same as undefined.
        if elem_bits == TAG_UNDEFINED
            || elem_bits == crate::value::TAG_HOLE
            || is_closure_value(elem_bits)
        {
            buf.push_str("null");
        } else {
            stringify_value_with_array_replacer(
                elem,
                allowed_keys,
                buf,
                indent,
                inner_depth,
                use_pretty,
            );
        }
    }
    if use_pretty {
        buf.push('\n');
        for _ in 0..depth {
            buf.push_str(indent);
        }
    }
    buf.push(']');
    STRINGIFY_STACK.with(|s| s.borrow_mut().pop());
}

// ─── Extract array of strings from a JSValue array ──────────────────────────

pub(crate) unsafe fn extract_string_array(ptr: *const u8) -> Vec<String> {
    // #7269: the PropertyList replacer array (`JSON.stringify(v, ['a','b'])`)
    // is exactly as susceptible to the GC_FLAG_FORWARDED array-growth stub as
    // any other array pointer in this file — resolve before the header read.
    let ptr = crate::array::clean_arr_ptr(ptr as *const crate::ArrayHeader) as *const u8;
    if ptr.is_null() {
        return Vec::new();
    }
    let arr = ptr as *const crate::ArrayHeader;
    let len = (*arr).length;
    let elements = crate::array::array_elements_ptr(arr as *const crate::ArrayHeader) as *const f64;
    // Coerce in array order before deduplicating: wrapper conversion can run
    // user code even when its resulting name already appears in the list.
    collect_property_list_keys((0..len).filter_map(|i| {
        json_property_list_key(*elements.add(i as usize))
    }))
}

/// Keep the first occurrence of each coerced key without changing read order.
/// Membership is indexed separately so distinct keys do not scan the prefix.
fn collect_property_list_keys<K: Clone + Eq + std::hash::Hash>(
    keys: impl Iterator<Item = K>,
) -> Vec<K> {
    let mut result = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for key in keys {
        if seen.insert(key.clone()) {
            result.push(key);
        }
    }
    result
}

/// Resolve one PropertyList element to its key name (or `None` to skip it).
/// String/Number values and `Number`/`String` wrapper objects qualify; all
/// other types (booleans, null, plain objects, symbols, …) are skipped.
pub(super) unsafe fn json_property_list_key(elem: f64) -> Option<String> {
    let bits = elem.to_bits();
    let tag = bits & 0xFFFF_0000_0000_0000;
    if tag == STRING_TAG {
        let str_ptr = (bits & POINTER_MASK) as *const StringHeader;
        return str_from_header(str_ptr).map(|s| s.to_string());
    }
    if tag == crate::value::SHORT_STRING_TAG {
        let jsval = JSValue::from_bits(bits);
        let mut scratch = [0u8; crate::value::SHORT_STRING_MAX_LEN];
        let n = jsval.short_string_to_buf(&mut scratch);
        return std::str::from_utf8(&scratch[..n])
            .ok()
            .map(|s| s.to_string());
    }
    // Heap-string literal pointer (denormal-range bits, never a real number).
    if is_raw_pointer(bits) {
        let str_ptr = bits as *const StringHeader;
        return str_from_header(str_ptr).map(|s| s.to_string());
    }
    let jsval = JSValue::from_bits(bits);
    if jsval.is_int32() {
        return Some(jsval.as_int32().to_string());
    }
    if jsval.is_number() {
        let p = crate::string::js_number_to_string(elem);
        return str_from_header(p).map(|s| s.to_string());
    }
    // String / Number wrapper objects → ToString(v). This runs the object's
    // own `toString`/`valueOf` (ToPrimitive), not a raw read of [[*Data]] — so
    // `new Number(10)` with an overridden `toString` contributes that string,
    // not "10" (test262 replacer-array-{number,string}-object).
    if let Some((cid, _)) = crate::builtins::boxed_primitive_payload(elem) {
        const CLASS_ID_BOXED_NUMBER: u32 = 0xFFFF_00D0;
        const CLASS_ID_BOXED_STRING: u32 = 0xFFFF_00D1;
        if cid == CLASS_ID_BOXED_NUMBER || cid == CLASS_ID_BOXED_STRING {
            let s = crate::value::js_jsvalue_to_string(elem);
            return str_from_header(s).map(|s| s.to_string());
        }
    }
    None
}

#[cfg(test)]
#[path = "property_list_tests.rs"]
mod tests;
