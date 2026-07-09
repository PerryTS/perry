//! `console.table` rendering machinery.
//!
//! Split out of the original monolithic `builtins.rs` (#topic: split-large-files).
//! Implements the box-drawing tabular view used by `console.table(value, properties?)`
//! across arrays of objects/arrays/primitives, Maps, Sets, typed arrays, and
//! single-object inputs.

#[cfg(feature = "ohos-napi")]
use super::println;
use super::*;

// === console.table ===
//
// Render a tabular view of an array of objects, array of arrays, or single object,
// matching Node.js' `util.inspect.table` output (box-drawing characters, single-quoted
// strings in cells, left-aligned everything).

/// Format a single JSValue for use as a table cell.
/// Strings get single-quote-wrapped (matching Node's util.inspect default).
/// Numbers, booleans, null, undefined are stringified verbatim.
/// Nested arrays/objects collapse to a JS-ish summary.
fn format_table_cell(value: f64) -> String {
    let jsval = JSValue::from_bits(value.to_bits());
    unsafe {
        if jsval.is_undefined() {
            "undefined".to_string()
        } else if jsval.is_null() {
            "null".to_string()
        } else if jsval.is_bool() {
            jsval.as_bool().to_string()
        } else if let Some(s) = read_string_from_jsvalue(jsval) {
            // #1781: covers both heap STRING_TAG and inline SSO short
            // strings — without the SSO branch a <=5-char cell fell through
            // to the numeric arm and rendered as "NaN".
            format!("'{}'", s)
        } else if jsval.is_int32() {
            jsval.as_int32().to_string()
        } else if jsval.is_pointer() {
            // Nested array/object: use the existing pretty-printer (un-quoted strings inside)
            format_jsvalue(value, 0)
        } else if jsval.is_bigint() {
            // Reuse format_jsvalue's bigint formatter
            format_jsvalue(value, 0)
        } else {
            // Plain number
            let n = value;
            if n.is_nan() {
                "NaN".to_string()
            } else if n.is_infinite() {
                if n > 0.0 {
                    "Infinity".to_string()
                } else {
                    "-Infinity".to_string()
                }
            } else if is_negative_zero(n) {
                "-0".to_string()
            } else if n.fract() == 0.0 && n.abs() < crate::builtins::INT_EXACT_FASTPATH_LIMIT {
                (n as i64).to_string()
            } else {
                format_finite_number_js(n)
            }
        }
    }
}

/// Read a string out of a NaN-boxed string JSValue. SSO-aware (#1781):
/// accepts both heap `STRING_TAG` pointers and inline `SHORT_STRING_TAG`
/// values; returns `None` for non-strings.
unsafe fn read_string_from_jsvalue(jsval: JSValue) -> Option<String> {
    if jsval.is_short_string() {
        let mut buf = [0u8; crate::value::SHORT_STRING_MAX_LEN];
        let n = jsval.short_string_to_buf(&mut buf);
        return Some(
            std::str::from_utf8(&buf[..n])
                .unwrap_or("[invalid utf8]")
                .to_string(),
        );
    }
    if !jsval.is_string() {
        return None;
    }
    let ptr = jsval.as_string_ptr();
    if ptr.is_null() {
        return Some(String::new());
    }
    let len = (*ptr).byte_len as usize;
    let data = (ptr as *const u8).add(std::mem::size_of::<StringHeader>());
    let bytes = std::slice::from_raw_parts(data, len);
    Some(
        std::str::from_utf8(bytes)
            .unwrap_or("[invalid utf8]")
            .to_string(),
    )
}

/// Get the GC type tag for a value's pointed-to allocation, if any.
/// Returns 0 if the value is not a GC-tracked pointer.
unsafe fn get_gc_type(value: f64) -> u8 {
    let jsval = JSValue::from_bits(value.to_bits());
    if !jsval.is_pointer() {
        return 0;
    }
    let ptr: *const u8 = jsval.as_pointer();
    if ptr.is_null() || (ptr as usize) < 0x10000 {
        return 0;
    }
    let gc_header = ptr.sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader;
    (*gc_header).obj_type
}

/// Render a console.table given headers and rows.
/// `headers[0]` is always the (index) column. Each row's `cells[0]` is the
/// (index) value (row number for arrays, property name for single objects).
fn table_display_width(s: &str) -> usize {
    s.chars()
        .map(|c| {
            let cp = c as u32;
            if (0x1100..=0x115f).contains(&cp)
                || (0x2e80..=0xa4cf).contains(&cp)
                || (0xac00..=0xd7a3).contains(&cp)
                || (0xf900..=0xfaff).contains(&cp)
                || (0xfe10..=0xfe19).contains(&cp)
                || (0xfe30..=0xfe6f).contains(&cp)
                || (0xff00..=0xff60).contains(&cp)
                || (0xffe0..=0xffe6).contains(&cp)
            {
                2
            } else {
                1
            }
        })
        .sum()
}

fn render_table(headers: &[String], rows: &[Vec<String>]) {
    let num_cols = headers.len();
    if num_cols == 0 {
        return;
    }

    // Compute display widths, not scalar counts. Node's table rendering pads
    // East Asian/full-width code points as width 2, which matters for values
    // such as `￥` and `你好`.
    let mut widths: Vec<usize> = headers.iter().map(|h| table_display_width(h)).collect();
    for row in rows {
        for (i, cell) in row.iter().enumerate() {
            if i < widths.len() {
                let w = table_display_width(cell);
                if w > widths[i] {
                    widths[i] = w;
                }
            }
        }
    }

    // Helpers
    let dashes = |w: usize| -> String { "─".repeat(w + 2) };
    let pad_cell = |s: &str, w: usize| -> String {
        let count = table_display_width(s);
        let pad = w.saturating_sub(count);
        format!(" {}{} ", s, " ".repeat(pad))
    };

    // Top border: ┌────┬────┐
    let mut top = String::from("┌");
    for (i, w) in widths.iter().enumerate() {
        top.push_str(&dashes(*w));
        top.push_str(if i + 1 == num_cols { "┐" } else { "┬" });
    }
    println!("{}", top);

    // Header row: │ (index) │ a │
    let mut header_row = String::from("│");
    for (i, h) in headers.iter().enumerate() {
        header_row.push_str(&pad_cell(h, widths[i]));
        header_row.push('│');
    }
    println!("{}", header_row);

    // Separator: ├────┼────┤
    let mut sep = String::from("├");
    for (i, w) in widths.iter().enumerate() {
        sep.push_str(&dashes(*w));
        sep.push_str(if i + 1 == num_cols { "┤" } else { "┼" });
    }
    println!("{}", sep);

    // Data rows
    for row in rows {
        let mut line = String::from("│");
        for (i, _) in headers.iter().enumerate() {
            let cell = row.get(i).map(|s| s.as_str()).unwrap_or("");
            line.push_str(&pad_cell(cell, widths[i]));
            line.push('│');
        }
        println!("{}", line);
    }

    // Bottom border: └────┴────┘
    let mut bottom = String::from("└");
    for (i, w) in widths.iter().enumerate() {
        bottom.push_str(&dashes(*w));
        bottom.push_str(if i + 1 == num_cols { "┘" } else { "┴" });
    }
    println!("{}", bottom);
}

/// Read all keys from an object's keys_array as Strings.
unsafe fn object_key_names(obj_ptr: *const crate::object::ObjectHeader) -> Vec<String> {
    let keys_array = (*obj_ptr).keys_array;
    if keys_array.is_null() {
        return Vec::new();
    }
    let count = crate::array::js_array_length(keys_array) as usize;
    let mut keys = Vec::with_capacity(count);
    for i in 0..count {
        let key_val = crate::array::js_array_get(keys_array, i as u32);
        if let Some(s) = read_string_from_jsvalue(key_val) {
            keys.push(s);
        }
    }
    keys
}

#[no_mangle]
pub extern "C" fn js_console_table(value: f64) {
    js_console_table_with_properties(value, f64::from_bits(JSValue::undefined().bits()))
}

fn table_properties_from_value(value: f64) -> Option<Vec<String>> {
    unsafe {
        let jsval = JSValue::from_bits(value.to_bits());
        if jsval.is_undefined() || jsval.is_null() {
            return None;
        }
        if get_gc_type(value) != crate::gc::GC_TYPE_ARRAY {
            return None;
        }
        let arr_ptr = jsval.as_pointer::<crate::array::ArrayHeader>();
        if arr_ptr.is_null() {
            return None;
        }
        let length = (*arr_ptr).length as usize;
        let data_ptr = (arr_ptr as *const u8).add(std::mem::size_of::<crate::array::ArrayHeader>())
            as *const f64;
        let mut out = Vec::with_capacity(length);
        for i in 0..length {
            let prop = *data_ptr.add(i);
            let prop_js = JSValue::from_bits(prop.to_bits());
            if let Some(s) = read_string_from_jsvalue(prop_js) {
                if !out.contains(&s) {
                    out.push(s);
                }
            } else {
                let s = format_jsvalue(prop, 0);
                if !out.contains(&s) {
                    out.push(s);
                }
            }
        }
        Some(out)
    }
}

#[no_mangle]
pub extern "C" fn js_console_table_with_properties(value: f64, properties: f64) {
    unsafe {
        let jsval = JSValue::from_bits(value.to_bits());
        if !jsval.is_pointer() {
            // Primitives just print via the dynamic logger.
            js_console_log_dynamic(value);
            return;
        }
        let only_properties = table_properties_from_value(properties);
        let raw_addr = jsval.as_pointer::<u8>() as usize;
        if crate::typedarray::lookup_typed_array_kind(raw_addr).is_some() {
            let ta = raw_addr as *const crate::typedarray::TypedArrayHeader;
            let length = crate::typedarray::js_typed_array_length(ta).max(0) as usize;
            let headers = vec!["(index)".to_string(), "Values".to_string()];
            let mut rows: Vec<Vec<String>> = Vec::with_capacity(length);
            for i in 0..length {
                rows.push(vec![
                    i.to_string(),
                    format_table_cell(crate::typedarray::js_typed_array_get(ta, i as i32)),
                ]);
            }
            render_table(&headers, &rows);
            return;
        }
        if crate::buffer::is_uint8array_buffer(raw_addr) {
            let buf = raw_addr as *const crate::buffer::BufferHeader;
            let length = (*buf).length as usize;
            let headers = vec!["(index)".to_string(), "Values".to_string()];
            let mut rows: Vec<Vec<String>> = Vec::with_capacity(length);
            for i in 0..length {
                rows.push(vec![
                    i.to_string(),
                    crate::buffer::js_buffer_get(buf, i as i32).to_string(),
                ]);
            }
            render_table(&headers, &rows);
            return;
        }
        let gc_type = get_gc_type(value);

        if gc_type == crate::gc::GC_TYPE_ARRAY {
            // Array case — peek at first element to decide shape.
            let arr_ptr = jsval.as_pointer::<crate::array::ArrayHeader>();
            if arr_ptr.is_null() {
                println!("undefined");
                return;
            }
            let length = (*arr_ptr).length as usize;
            let data_ptr = (arr_ptr as *const u8)
                .add(std::mem::size_of::<crate::array::ArrayHeader>())
                as *const f64;

            if length == 0 {
                render_table(&["(index)".to_string()], &[]);
                return;
            }

            // Decide: array of objects vs array of arrays vs array of primitives.
            let mut has_array = false;
            let mut has_object = false;
            for i in 0..length {
                match get_gc_type(*data_ptr.add(i)) {
                    t if t == crate::gc::GC_TYPE_ARRAY => has_array = true,
                    t if t == crate::gc::GC_TYPE_OBJECT => has_object = true,
                    _ => {}
                }
            }

            if has_object && !has_array {
                // Array of objects: union all keys, or honor the optional
                // `properties` argument (`console.table(rows, ["a"])`).
                let mut all_keys: Vec<String> = only_properties.clone().unwrap_or_default();
                let mut row_keys: Vec<Vec<String>> = Vec::with_capacity(length);
                for i in 0..length {
                    let elem = *data_ptr.add(i);
                    let elem_jsval = JSValue::from_bits(elem.to_bits());
                    if get_gc_type(elem) == crate::gc::GC_TYPE_OBJECT {
                        let obj_ptr = elem_jsval.as_pointer::<crate::object::ObjectHeader>();
                        let keys = object_key_names(obj_ptr);
                        if only_properties.is_none() {
                            for k in &keys {
                                if !all_keys.contains(k) {
                                    all_keys.push(k.clone());
                                }
                            }
                        }
                        row_keys.push(keys);
                    } else {
                        row_keys.push(Vec::new());
                    }
                }

                let mut headers: Vec<String> = Vec::with_capacity(1 + all_keys.len());
                headers.push("(index)".to_string());
                for k in &all_keys {
                    headers.push(k.clone());
                }

                let mut rows: Vec<Vec<String>> = Vec::with_capacity(length);
                for i in 0..length {
                    let elem = *data_ptr.add(i);
                    let elem_jsval = JSValue::from_bits(elem.to_bits());
                    let mut row: Vec<String> = Vec::with_capacity(headers.len());
                    row.push(i.to_string());
                    if get_gc_type(elem) == crate::gc::GC_TYPE_OBJECT {
                        let obj_ptr = elem_jsval.as_pointer::<crate::object::ObjectHeader>();
                        for key in &all_keys {
                            let present = row_keys
                                .get(i)
                                .map(|keys| keys.contains(key))
                                .unwrap_or(false);
                            // Build a temporary StringHeader for the lookup
                            let key_ptr = build_temp_string_header(key);
                            let v =
                                crate::object::js_object_get_field_by_name_f64(obj_ptr, key_ptr);
                            free_temp_string_header(key_ptr);
                            let v_jsval = JSValue::from_bits(v.to_bits());
                            if v_jsval.is_undefined() && !present {
                                row.push("".to_string());
                            } else {
                                row.push(format_table_cell(v));
                            }
                        }
                    } else {
                        for _ in &all_keys {
                            row.push("".to_string());
                        }
                    }
                    rows.push(row);
                }

                render_table(&headers, &rows);
            } else if has_array {
                // Array of arrays (or mixed array / primitive). Issue #1276:
                // Node skips the "Values" column entirely when *every* row is
                // an array — only mixed cases (e.g. `[Symbol(), 5, [10]]`)
                // get the trailing Values column for the non-array rows.
                let mut max_len = 0usize;
                let mut all_arrays = true;
                for i in 0..length {
                    let elem = *data_ptr.add(i);
                    let elem_jsval = JSValue::from_bits(elem.to_bits());
                    if get_gc_type(elem) == crate::gc::GC_TYPE_ARRAY {
                        let sub = elem_jsval.as_pointer::<crate::array::ArrayHeader>();
                        let l = (*sub).length as usize;
                        if l > max_len {
                            max_len = l;
                        }
                    } else {
                        all_arrays = false;
                    }
                }

                let include_values_col = !all_arrays;
                let mut headers: Vec<String> = Vec::with_capacity(2 + max_len);
                headers.push("(index)".to_string());
                for j in 0..max_len {
                    headers.push(j.to_string());
                }
                if include_values_col {
                    headers.push("Values".to_string());
                }

                let mut rows: Vec<Vec<String>> = Vec::with_capacity(length);
                for i in 0..length {
                    let elem = *data_ptr.add(i);
                    let elem_jsval = JSValue::from_bits(elem.to_bits());
                    let mut row: Vec<String> = Vec::with_capacity(headers.len());
                    row.push(i.to_string());
                    if get_gc_type(elem) == crate::gc::GC_TYPE_ARRAY {
                        let sub = elem_jsval.as_pointer::<crate::array::ArrayHeader>();
                        let sub_len = (*sub).length as usize;
                        let sub_data = (sub as *const u8)
                            .add(std::mem::size_of::<crate::array::ArrayHeader>())
                            as *const f64;
                        for j in 0..max_len {
                            if j < sub_len {
                                let v = *sub_data.add(j);
                                row.push(format_table_cell(v));
                            } else {
                                row.push("".to_string());
                            }
                        }
                        if include_values_col {
                            row.push("".to_string());
                        }
                    } else {
                        for _ in 0..max_len {
                            row.push("".to_string());
                        }
                        row.push(format_table_cell(elem));
                    }
                    rows.push(row);
                }

                render_table(&headers, &rows);
            } else {
                // Array of primitives: single "Values" column.
                let headers = vec!["(index)".to_string(), "Values".to_string()];
                let mut rows: Vec<Vec<String>> = Vec::with_capacity(length);
                for i in 0..length {
                    let elem = *data_ptr.add(i);
                    if elem.to_bits() == crate::value::TAG_HOLE
                        || JSValue::from_bits(elem.to_bits()).is_undefined()
                    {
                        continue;
                    }
                    rows.push(vec![i.to_string(), format_table_cell(elem)]);
                }
                render_table(&headers, &rows);
            }
        } else if gc_type == crate::gc::GC_TYPE_OBJECT {
            // Single object: rows are property name → "Values" column.
            let obj_ptr = jsval.as_pointer::<crate::object::ObjectHeader>();
            let keys = object_key_names(obj_ptr);
            if keys.is_empty() {
                return;
            }
            // Object whose values are row objects: union nested keys into
            // columns, e.g. console.table({ a: { a: 1, b: 2 } }).
            let mut nested_keys: Vec<String> = Vec::new();
            let mut all_nested = true;
            for i in 0..keys.len() {
                let v = crate::object::js_object_get_field_f64(obj_ptr, i as u32);
                if get_gc_type(v) == crate::gc::GC_TYPE_OBJECT {
                    let vp =
                        JSValue::from_bits(v.to_bits()).as_pointer::<crate::object::ObjectHeader>();
                    for k in object_key_names(vp) {
                        if !nested_keys.contains(&k) {
                            nested_keys.push(k);
                        }
                    }
                } else {
                    all_nested = false;
                }
            }
            if !nested_keys.is_empty() {
                let include_values_col = !all_nested;
                let mut headers = vec!["(index)".to_string()];
                headers.extend(nested_keys.iter().cloned());
                if include_values_col {
                    headers.push("Values".to_string());
                }
                let mut rows: Vec<Vec<String>> = Vec::with_capacity(keys.len());
                for (i, key) in keys.iter().enumerate() {
                    let v = crate::object::js_object_get_field_f64(obj_ptr, i as u32);
                    let mut row = vec![key.clone()];
                    if get_gc_type(v) == crate::gc::GC_TYPE_OBJECT {
                        let vp = JSValue::from_bits(v.to_bits())
                            .as_pointer::<crate::object::ObjectHeader>();
                        for nested_key in &nested_keys {
                            let key_ptr = build_temp_string_header(nested_key);
                            let cell = crate::object::js_object_get_field_by_name_f64(vp, key_ptr);
                            free_temp_string_header(key_ptr);
                            if JSValue::from_bits(cell.to_bits()).is_undefined() {
                                row.push("".to_string());
                            } else {
                                row.push(format_table_cell(cell));
                            }
                        }
                        if include_values_col {
                            row.push("".to_string());
                        }
                    } else {
                        for _ in &nested_keys {
                            row.push("".to_string());
                        }
                        if include_values_col {
                            row.push(format_table_cell(v));
                        }
                    }
                    rows.push(row);
                }
                render_table(&headers, &rows);
                return;
            }
            let headers = vec!["(index)".to_string(), "Values".to_string()];
            let mut rows: Vec<Vec<String>> = Vec::with_capacity(keys.len());
            for (i, key) in keys.iter().enumerate() {
                // Read the value by field index (matches keys_array order).
                let v = crate::object::js_object_get_field_f64(obj_ptr, i as u32);
                rows.push(vec![key.clone(), format_table_cell(v)]);
            }
            render_table(&headers, &rows);
        } else if gc_type == crate::gc::GC_TYPE_MAP
            || crate::map::is_registered_map(jsval.as_pointer::<u8>() as usize)
        {
            let map = jsval.as_pointer::<crate::map::MapHeader>();
            let size = crate::map::js_map_size(map) as usize;
            let headers = vec![
                "(iteration index)".to_string(),
                "Key".to_string(),
                "Values".to_string(),
            ];
            let mut rows = Vec::with_capacity(size);
            for i in 0..size {
                rows.push(vec![
                    i.to_string(),
                    format_table_cell(crate::map::js_map_entry_key_at(map, i as u32)),
                    format_table_cell(crate::map::js_map_entry_value_at(map, i as u32)),
                ]);
            }
            render_table(&headers, &rows);
        } else if crate::set::is_registered_set(jsval.as_pointer::<u8>() as usize) {
            let set = jsval.as_pointer::<crate::set::SetHeader>();
            let size = crate::set::js_set_size(set) as usize;
            let headers = vec!["(iteration index)".to_string(), "Values".to_string()];
            let mut rows = Vec::with_capacity(size);
            for i in 0..size {
                rows.push(vec![
                    i.to_string(),
                    format_table_cell(crate::set::js_set_value_at(set, i as u32)),
                ]);
            }
            render_table(&headers, &rows);
        } else {
            // Unknown pointer kind — fall back to console.log
            js_console_log_dynamic(value);
        }
    }
}

/// Build a temporary GC-allocated StringHeader for use in
/// `js_object_get_field_by_name`. The GC will reclaim it.
unsafe fn build_temp_string_header(s: &str) -> *const StringHeader {
    let bytes = s.as_bytes();
    js_string_from_bytes(bytes.as_ptr(), bytes.len() as u32) as *const StringHeader
}

unsafe fn free_temp_string_header(_ptr: *const StringHeader) {
    // No-op: GC-allocated, will be collected.
}

#[cfg(test)]
mod sso_tests_1781 {
    use super::*;

    /// #1781: `console.table` cells/headers that are strings <= 5 bytes are
    /// inline SSO values. `is_string()` (STRING_TAG-only) missed them, so a
    /// short cell fell through to the numeric arm and rendered as "NaN", and
    /// a short header decoded to None.
    #[test]
    fn read_string_from_jsvalue_handles_sso() {
        for s in ["", "a", "id", "abc", "hello"] {
            let v = JSValue::try_short_string(s.as_bytes()).expect("len <= 5 -> SSO");
            let got = unsafe { read_string_from_jsvalue(v) };
            assert_eq!(got.as_deref(), Some(s), "header decode mismatch for {s:?}");
        }
    }

    #[test]
    fn format_table_cell_quotes_sso_string() {
        let v = JSValue::try_short_string(b"abc").expect("SSO");
        assert_eq!(format_table_cell(f64::from_bits(v.bits())), "'abc'");
        // empty SSO string renders as ''
        let empty = JSValue::try_short_string(b"").expect("SSO");
        assert_eq!(format_table_cell(f64::from_bits(empty.bits())), "''");
    }
}
