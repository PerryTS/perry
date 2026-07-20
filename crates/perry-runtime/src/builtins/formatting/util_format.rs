//! `util.format` / `util.formatWithOptions` / `util.inspect` family —
//! the printf-style formatter entry points and their `%j` cycle-detection
//! helpers. Split out of `formatting.rs` to keep that file under the
//! file-size gate; the shared per-tag value formatter (`format_jsvalue` /
//! `format_jsvalue_for_json`) and the deep-equal machinery stay in the parent.

use super::*;

/// #1002: `util.format(fmt, ...args)` / `util.formatWithOptions(opts,
/// fmt, ...args)` native implementation. Codegen bundles the call args
/// into a heap-allocated array (same shape as `js_console_log_spread`)
/// and calls in here; the first element is the format string and the
/// rest are substitution values. Returns a NaN-boxed string.
///
/// Placeholder support mirrors Node's `util.format` for the substrings
/// most callers care about: `%s` (string-coerce), `%d` (Number-coerce),
/// `%i` (integer), `%f` (float), `%j` (JSON), `%o`/`%O` (object inspect),
/// `%%` (literal percent). Anything else is left as-is. Trailing args without a
/// matching placeholder are appended space-separated, again matching
/// Node.
///
/// When the first array element isn't a string, Node falls back to
/// space-joining every arg through `util.inspect` — same here, going
/// through `format_jsvalue` for parity with `console.log`.
// `%j` must turn circular `JSON.stringify` failures into a whole-placeholder
// `[Circular]`. Perry's exceptions longjmp through generated try frames, so
// preflight the JSON-visible graph instead of attempting to catch here.
unsafe fn util_format_json_arg_has_cycle(value: f64) -> bool {
    let mut stack = Vec::new();
    util_format_json_value_has_cycle(value, &mut stack)
}

unsafe fn util_format_json_value_has_cycle(value: f64, stack: &mut Vec<usize>) -> bool {
    let jv = JSValue::from_bits(value.to_bits());
    if jv.is_pointer() {
        let ptr = jv.as_pointer::<u8>();
        return util_format_json_ptr_has_cycle(ptr, stack);
    }
    if looks_like_raw_heap_pointer(value) {
        return util_format_json_ptr_has_cycle(value.to_bits() as *const u8, stack);
    }
    false
}

unsafe fn util_format_json_ptr_has_cycle(ptr: *const u8, stack: &mut Vec<usize>) -> bool {
    let addr = ptr as usize;
    if crate::value::addr_class::is_handle_band(addr)
        || crate::buffer::is_registered_buffer(addr)
        || crate::symbol::is_registered_symbol(addr)
    {
        return false;
    }
    let gc_header = ptr.sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader;
    match (*gc_header).obj_type {
        crate::gc::GC_TYPE_ARRAY => util_format_json_array_has_cycle(ptr, stack),
        crate::gc::GC_TYPE_OBJECT => util_format_json_object_has_cycle(ptr, stack),
        _ => false,
    }
}

unsafe fn util_format_json_array_has_cycle(ptr: *const u8, stack: &mut Vec<usize>) -> bool {
    let addr = ptr as usize;
    if stack.contains(&addr) {
        return true;
    }
    stack.push(addr);

    let arr = ptr as *const crate::ArrayHeader;
    let len = (*arr).length as usize;
    let elements = ptr.add(std::mem::size_of::<crate::ArrayHeader>()) as *const f64;
    let found = (0..len).any(|i| {
        let value = *elements.add(i);
        let bits = value.to_bits();
        bits != crate::value::TAG_UNDEFINED
            && !crate::json::is_closure_value(bits)
            && util_format_json_value_has_cycle(value, stack)
    });

    stack.pop();
    found
}

unsafe fn util_format_json_object_has_cycle(ptr: *const u8, stack: &mut Vec<usize>) -> bool {
    let addr = ptr as usize;
    if stack.contains(&addr) {
        return true;
    }
    stack.push(addr);

    let obj = ptr as *const crate::ObjectHeader;
    let keys_arr = (*obj).keys_array;
    let found = if keys_arr.is_null() {
        false
    } else {
        let keys_len = (*keys_arr).length;
        let num_fields = (*obj).field_count;
        let fields_ptr = ptr.add(std::mem::size_of::<crate::ObjectHeader>()) as *const f64;
        let alloc_limit = std::cmp::max(num_fields, crate::object::INLINE_SLOT_FLOOR as u32);
        (0..keys_len).any(|f| {
            let bits = if f < alloc_limit {
                (*fields_ptr.add(f as usize)).to_bits()
            } else {
                crate::object::js_object_get_field(obj, f).bits()
            };
            bits != crate::value::TAG_UNDEFINED
                && !crate::json::is_closure_value(bits)
                && util_format_json_value_has_cycle(f64::from_bits(bits), stack)
        })
    };

    stack.pop();
    found
}

/// Render a `%o`/`%O` argument the way `util.inspect` does at the top
/// level: a primitive string is **quoted** (`'hello'`), matching
/// `js_util_inspect`. `format_jsvalue` alone leaves a top-level string
/// unquoted (the `console.log(str)` bare-string behavior), so calling it
/// directly for `%o`/`%O` diverged from Node, which routes the value
/// through `util.inspect` (strings quoted at every depth).
fn inspect_format_arg(val: f64) -> String {
    let jv = crate::value::JSValue::from_bits(val.to_bits());
    if jv.is_any_string() {
        let s = jsvalue_string_content(val).unwrap_or_default();
        format!("'{}'", escape_string(&s))
    } else {
        format_jsvalue(val, 0)
    }
}

#[no_mangle]
pub extern "C" fn js_util_format(arr_ptr: *const crate::array::ArrayHeader) -> f64 {
    use crate::value::JSValue;
    // Helper: produce a NaN-boxed string from a Rust `&str`.
    fn boxed_string(s: &str) -> f64 {
        let ptr = crate::string::js_string_from_bytes(s.as_ptr(), s.len() as u32);
        f64::from_bits(JSValue::string_ptr(ptr).bits())
    }
    // Helper: turn any JS value into its `String(value)` coercion using
    // Perry's existing helper (covers strings, numbers, null/undefined,
    // objects via their .toString protocol).
    unsafe fn jsvalue_as_owned_string(val: f64) -> String {
        let s_ptr = crate::value::js_jsvalue_to_string(val);
        if s_ptr.is_null() {
            return String::new();
        }
        let len = (*s_ptr).byte_len as usize;
        let data = (s_ptr as *const u8).add(std::mem::size_of::<crate::string::StringHeader>());
        let bs = std::slice::from_raw_parts(data, len);
        std::str::from_utf8(bs).unwrap_or("").to_string()
    }
    fn util_number_placeholder_coerce(val: f64, jv: &JSValue) -> f64 {
        if jv.is_int32() {
            jv.as_int32() as f64
        } else if unsafe { crate::symbol::js_is_symbol(val) } != 0 {
            f64::NAN
        } else {
            js_number_coerce(val)
        }
    }
    if arr_ptr.is_null() {
        return boxed_string("");
    }
    unsafe {
        let length = (*arr_ptr).length as usize;
        let data_ptr = (arr_ptr as *const u8).add(std::mem::size_of::<crate::array::ArrayHeader>())
            as *const f64;

        // No format string → empty result. Node returns "" for
        // `util.format()`.
        if length == 0 {
            return boxed_string("");
        }

        // If arg[0] isn't a string, fall back to space-joining every
        // arg with `format_jsvalue` (matches Node's non-string-first
        // util.format codepath).
        let first = *data_ptr;
        let first_jv = JSValue::from_bits(first.to_bits());
        if !first_jv.is_any_string() {
            let mut parts: Vec<String> = Vec::with_capacity(length);
            for i in 0..length {
                parts.push(format_jsvalue(*data_ptr.add(i), 0));
            }
            return boxed_string(&parts.join(" "));
        }

        // Materialize the format string. Short strings live inline in
        // the NaN-box (top bits set), long strings live in a
        // StringHeader. The unified helper handles both.
        let fmt = jsvalue_as_owned_string(first);
        if length == 1 {
            return boxed_string(&fmt);
        }

        let mut out = String::with_capacity(fmt.len());
        let mut arg_idx: usize = 1;
        let bytes = fmt.as_bytes();
        let mut i = 0;
        // Issue #1275: emit literal-text segments as UTF-8 `&str` slices
        // so multi-byte codepoints (e.g. "…", "é", "中") survive the format
        // pass. The previous `out.push(byte as char)` cast each UTF-8 byte
        // to a Latin-1 codepoint and produced mojibake on the terminal.
        let mut seg_start = 0usize;
        while i < bytes.len() {
            let b = bytes[i];
            if b != b'%' || i + 1 >= bytes.len() {
                i += 1;
                continue;
            }
            // Flush the literal text accumulated since the last % handled.
            if seg_start < i {
                out.push_str(&fmt[seg_start..i]);
            }
            // Advance the literal-segment cursor past the %spec; the
            // various branches below all consume exactly 2 bytes via
            // `i += 2`, so this stays in sync regardless of which arm runs.
            seg_start = i + 2;
            let spec = bytes[i + 1];
            // `%%` → literal `%` (no arg consumed).
            if spec == b'%' {
                out.push('%');
                i += 2;
                continue;
            }
            // Out of args: leave the placeholder untouched (Node does
            // the same — `util.format("%s %s", "x")` prints `"x %s"`).
            if arg_idx >= length {
                out.push('%');
                out.push(spec as char);
                i += 2;
                continue;
            }
            let val = *data_ptr.add(arg_idx);
            arg_idx += 1;
            let jv = JSValue::from_bits(val.to_bits());
            match spec {
                b's' => {
                    out.push_str(&jsvalue_as_owned_string(val));
                }
                b'd' => {
                    // Node's `%d` uses Number(value), except BigInt keeps the
                    // literal `n` suffix.
                    if jv.is_bigint() {
                        out.push_str(&format_bigint_literal(val));
                    } else {
                        let f = util_number_placeholder_coerce(val, &jv);
                        out.push_str(&format_util_number(f));
                    }
                }
                b'i' => {
                    // Node preserves the BigInt `n` suffix for `%i`
                    // (e.g. `util.format("%i", 5n)` → `"5n"`).
                    if jv.is_bigint() {
                        out.push_str(&format_bigint_literal(val));
                    } else {
                        let f = if jv.is_int32() {
                            jv.as_int32() as f64
                        } else if jv.is_any_string()
                            && jsvalue_string_content(val)
                                .map(|s| s.is_empty())
                                .unwrap_or(false)
                        {
                            f64::NAN
                        } else {
                            util_number_placeholder_coerce(val, &jv)
                        };
                        if f.is_nan() {
                            out.push_str("NaN");
                        } else {
                            let t = f.trunc();
                            if t == 0.0 && f.is_sign_negative() {
                                out.push_str("-0");
                            } else {
                                // Integer-truncated, matching Node. Format the
                                // whole value shortest-round-trip so large
                                // magnitudes (`%d` of `2**58`) print V8's
                                // `…740`, not the exact `…744` (#6127).
                                out.push_str(&super::format_integral_f64(t));
                            }
                        }
                    }
                }
                b'f' => {
                    // Node coerces BigInt lossily to Number for `%f`
                    // (`util.format("%f", 5n)` → `"5"`), dropping the `n`.
                    if jv.is_bigint() {
                        let ptr = jv.as_bigint_ptr();
                        let f = if ptr.is_null() {
                            f64::NAN
                        } else {
                            crate::bigint::js_bigint_to_f64(ptr)
                        };
                        if f.is_nan() {
                            out.push_str("NaN");
                        } else {
                            out.push_str(&format_finite_number_js(f));
                        }
                    } else {
                        let f = if jv.is_int32() {
                            jv.as_int32() as f64
                        } else if jv.is_any_string()
                            && jsvalue_string_content(val)
                                .map(|s| s.is_empty())
                                .unwrap_or(false)
                        {
                            f64::NAN
                        } else {
                            util_number_placeholder_coerce(val, &jv)
                        };
                        if f.is_nan() {
                            out.push_str("NaN");
                        } else {
                            out.push_str(&format_finite_number_js(f));
                        }
                    }
                }
                b'j' => {
                    unsafe {
                        if util_format_json_arg_has_cycle(val) {
                            out.push_str("[Circular]");
                            i += 2;
                            continue;
                        }
                        // Real JSON.stringify — string-replace post-processing
                        // of inspect output mangles strings that contain
                        // ", ", ": ", "{ ", or " }".
                        let s_ptr = crate::json::js_json_stringify(val, 0);
                        if s_ptr.is_null() {
                            out.push_str("undefined");
                        } else {
                            let len = (*s_ptr).byte_len as usize;
                            let data = (s_ptr as *const u8)
                                .add(std::mem::size_of::<crate::string::StringHeader>());
                            let bytes = std::slice::from_raw_parts(data, len);
                            out.push_str(std::str::from_utf8(bytes).unwrap_or(""));
                        }
                    }
                }
                b'o' => {
                    // Node's `%o` overlays util.inspect options with
                    // showHidden/showProxy and depth: 4.
                    let _depth_guard = InspectDepthLimitGuard::new(4);
                    let _hidden_guard = InspectShowHiddenGuard::new(true);
                    let _proxy_guard = InspectShowProxyGuard::new(true);
                    out.push_str(&inspect_format_arg(val));
                }
                b'O' => {
                    // `%O` keeps the default depth cap (2) — matching
                    // Node's `util.inspect` default options.
                    out.push_str(&inspect_format_arg(val));
                }
                b'c' => {
                    // Browser/Node console style marker. Consume the CSS
                    // argument but do not emit ANSI styling in the
                    // NO_COLOR parity environment.
                }
                _ => {
                    // Unknown specifier: leave verbatim, don't consume
                    // the arg (Node 22+ behavior — older Node consumed
                    // it; modern behavior is what libraries write
                    // against).
                    out.push('%');
                    out.push(spec as char);
                    arg_idx -= 1;
                }
            }
            i += 2;
        }
        // Flush the trailing literal segment (everything after the last %spec
        // or the entire string if no specifier was found).
        if seg_start < bytes.len() {
            out.push_str(&fmt[seg_start..]);
        }

        // Append any remaining args separated by spaces, again matching
        // Node: `util.format("hi", "x", "y")` → `"hi x y"`.
        while arg_idx < length {
            out.push(' ');
            out.push_str(&format_jsvalue(*data_ptr.add(arg_idx), 0));
            arg_idx += 1;
        }

        boxed_string(&out)
    }
}

#[no_mangle]
pub extern "C" fn js_util_format_with_options(
    options: f64,
    arr_ptr: *const crate::array::ArrayHeader,
) -> f64 {
    let max_depth =
        unsafe { crate::builtins::console::decode_dir_depth_option(options) }.unwrap_or(2);
    let show_hidden =
        unsafe { crate::builtins::console::decode_dir_bool_option(options, "showHidden") }
            .unwrap_or(false);
    let show_proxy =
        unsafe { crate::builtins::console::decode_dir_bool_option(options, "showProxy") }
            .unwrap_or(false);
    let custom_inspect =
        unsafe { crate::builtins::console::decode_dir_bool_option(options, "customInspect") }
            .unwrap_or(true);
    let getters = unsafe { crate::builtins::console::decode_dir_bool_option(options, "getters") }
        .unwrap_or(false);
    let sorted = unsafe { crate::builtins::console::decode_dir_bool_option(options, "sorted") }
        .unwrap_or(false);
    let compact = unsafe { crate::builtins::console::decode_dir_bool_option(options, "compact") }
        .unwrap_or(true);
    let _depth_guard = InspectDepthLimitGuard::new(max_depth);
    let _hidden_guard = InspectShowHiddenGuard::new(show_hidden);
    let _proxy_guard = InspectShowProxyGuard::new(show_proxy);
    let _custom_guard = InspectCustomInspectGuard::new(custom_inspect);
    let _getters_guard = InspectGettersGuard::new(getters);
    let _sorted_guard = InspectSortedGuard::new(sorted);
    let _compact_guard = InspectCompactGuard::new(compact);
    js_util_format(arr_ptr)
}

#[no_mangle]
pub extern "C" fn js_util_inspect(value: f64, options: f64) -> f64 {
    let default_options = crate::object::util_inspect_default_options_value();
    let max_depth = unsafe { crate::builtins::console::decode_dir_depth_option(options) }
        .or_else(|| unsafe { crate::builtins::console::decode_dir_depth_option(default_options) })
        .unwrap_or(2);
    let show_hidden = inspect_bool_option(options, default_options, "showHidden").unwrap_or(false);
    let show_proxy = inspect_bool_option(options, default_options, "showProxy").unwrap_or(false);
    // `util.inspect` defaults to `customInspect: true`; an explicit
    // `{ customInspect: false }` opts out and surfaces the hook as a
    // symbol property. Refs #1201.
    let custom_inspect =
        inspect_bool_option(options, default_options, "customInspect").unwrap_or(true);
    let getters = inspect_bool_option(options, default_options, "getters").unwrap_or(false);
    let sorted = inspect_bool_option(options, default_options, "sorted").unwrap_or(false);
    let compact = inspect_bool_option(options, default_options, "compact").unwrap_or(true);
    let _depth_guard = InspectDepthLimitGuard::new(max_depth);
    let _hidden_guard = InspectShowHiddenGuard::new(show_hidden);
    let _proxy_guard = InspectShowProxyGuard::new(show_proxy);
    let _custom_guard = InspectCustomInspectGuard::new(custom_inspect);
    let _getters_guard = InspectGettersGuard::new(getters);
    let _sorted_guard = InspectSortedGuard::new(sorted);
    let _compact_guard = InspectCompactGuard::new(compact);
    let jv = crate::value::JSValue::from_bits(value.to_bits());
    let out = if jv.is_any_string() {
        let s = jsvalue_string_content(value).unwrap_or_default();
        format!("'{}'", escape_string(&s))
    } else {
        format_jsvalue(value, 0)
    };
    let ptr = crate::string::js_string_from_bytes(out.as_ptr(), out.len() as u32);
    f64::from_bits(crate::value::JSValue::string_ptr(ptr).bits())
}

fn inspect_bool_option(options: f64, default_options: f64, name: &str) -> Option<bool> {
    unsafe { crate::builtins::console::decode_dir_bool_option(options, name) }.or_else(|| unsafe {
        crate::builtins::console::decode_dir_bool_option(default_options, name)
    })
}
