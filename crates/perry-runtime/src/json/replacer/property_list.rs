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
    // Circular reference check
    if STRINGIFY_STACK.with(|s| s.borrow().contains(&(ptr as usize))) {
        let msg = "Converting circular structure to JSON";
        let msg_ptr = js_string_from_bytes(msg.as_ptr(), msg.len() as u32);
        // Use js_typeerror_new so error_kind == ERROR_KIND_TYPE_ERROR and
        // `e instanceof TypeError` returns true (matching Node).
        let err_ptr = crate::error::js_typeerror_new(msg_ptr);
        crate::exception::js_throw(f64::from_bits(
            POINTER_TAG | (err_ptr as u64 & POINTER_MASK),
        ));
    }
    STRINGIFY_STACK.with(|s| s.borrow_mut().push(ptr as usize));

    let obj = ptr as *const crate::ObjectHeader;
    let num_fields = crate::object::object_live_slot_count(obj);
    let Some(keys_view) = super::super::stringify::object_keys_array_checked(obj) else {
        // Not an ObjectHeader after all (a Promise / WeakMap / ArrayBuffer that
        // reached here via a static TYPE_OBJECT hint). Node serializes those as
        // `{}`; walking the slot as an ArrayHeader would fault.
        buf.push_str("{}");
        return;
    };
    // A layout that keeps its own storage slots reads each key through them.
    let slotted = crate::object::key_attrs::keys_have_slots(keys_view.arr(), keys_view.count());
    if !slotted {
        crate::object::key_attrs::debug_assert_positional(keys_view.arr(), keys_view.count());
    }
    let keys_len = keys_view.count();
    let keys_elements = crate::object::keys_array_dense_slots(keys_view.arr()).0;
    let fields_ptr =
        (ptr as *const u8).add(std::mem::size_of::<crate::ObjectHeader>()) as *const f64;
    // Iterate keys_len, not min(...): ≥9-field objects keep overflow values in
    // OVERFLOW_FIELDS (see the function-replacer walk above / plain #307 fix).
    let alloc_limit = std::cmp::max(num_fields, crate::object::INLINE_SLOT_FLOOR as u32);
    let actual_fields = keys_len;

    // Build a map of key_name -> field_value for the object. An own accessor
    // (`get key()`) holds no value in its raw slot, so resolve it through the
    // getter — matching the function-replacer walk (test262
    // replacer-array-duplicates, whose whitelisted key is a getter).
    // A per-object attribute (a builtin accessor installs no process gate)
    // must filter too: an accessor key's slot holds its accessor pair
    // (`accessor_pair.rs`), never a value to serialize.
    let filter_non_enum =
        crate::object::key_attrs::object_summary(ptr as *const crate::ObjectHeader)
            & crate::object::key_attrs::SUMMARY_KEY_BITS
            != 0;
    // An own key is hidden only on a class instance or a shape with a private
    // entry (#11791); the shape answers once for the whole walk.
    let hide_private =
        crate::object::field_get_set::own_keys_may_hide(ptr as *const crate::ObjectHeader);
    let mut field_map: Vec<(String, f64)> = Vec::new();
    // OrdinaryOwnPropertyKeys order: array indices ascending, then strings.
    let key_order = crate::object::ecma_own_key_order(keys_view);
    for j in 0..actual_fields {
        let f = key_order.as_ref().map_or(j, |order| order[j as usize]);
        // #9398: tombstoned slot from an O(1) delete — not a key, not
        // serialized. See `tombstoned_key_slot`.
        if tombstoned_key_slot(*keys_elements.add(f as usize))
            || super::super::stringify::is_symbol_value((*keys_elements.add(f as usize)).to_bits())
        {
            continue;
        }
        // #11232: physical class capture/private slots are not JS properties.
        // Filter before reading the value or invoking getters/replacers.
        if hide_private
            && crate::object::instance_private_key_hidden(
                obj,
                JSValue::from_bits((*keys_elements.add(f as usize)).to_bits()),
            )
        {
            continue;
        }
        let mut field_val = if f < alloc_limit && !slotted {
            *fields_ptr.add(f as usize)
        } else {
            f64::from_bits(crate::object::field_get_set::js_object_get_key_field(obj, f).bits())
        };
        if filter_non_enum && f < keys_len {
            if let Some(gv) =
                crate::object::json_object_getter_value(obj, *keys_elements.add(f as usize))
            {
                field_val = gv;
            }
        }
        let key_name = if f < keys_len {
            let key_f64 = *keys_elements.add(f as usize);
            let key_bits = key_f64.to_bits();
            let key_tag = key_bits & 0xFFFF_0000_0000_0000;
            let key_ptr = if key_tag == STRING_TAG || key_tag == POINTER_TAG {
                (key_bits & POINTER_MASK) as *const StringHeader
            } else {
                key_bits as *const StringHeader
            };
            str_from_header(key_ptr)
                .map(|s| s.to_string())
                .unwrap_or_else(|| format!("field{}", f))
        } else {
            format!("field{}", f)
        };
        field_map.push((key_name, field_val));
    }

    buf.push('{');
    let mut first = true;
    for allowed_key in allowed_keys {
        if let Some((_, field_val)) = field_map.iter().find(|(k, _)| k == allowed_key) {
            let field_bits = field_val.to_bits();
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
                    *field_val,
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
                    *field_val,
                    allowed_keys,
                    buf,
                    indent,
                    depth,
                    false,
                );
            }
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
        let err_ptr = crate::error::js_typeerror_new(msg_ptr);
        crate::exception::js_throw(f64::from_bits(
            POINTER_TAG | (err_ptr as u64 & POINTER_MASK),
        ));
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
    let mut result: Vec<String> = Vec::new();
    for i in 0..len {
        let elem = *elements.add(i as usize);
        // Per ECMA-262 SerializeJSONObject: a PropertyList item is kept when it
        // is a String, a Number, or a String/Number wrapper object (Numbers and
        // wrappers coerce to their canonical string form). Duplicate names are
        // dropped — first occurrence wins.
        if let Some(key) = json_property_list_key(elem) {
            if !result.contains(&key) {
                result.push(key);
            }
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
