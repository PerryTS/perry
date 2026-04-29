//! Built-in functions and objects
//!
//! Provides runtime implementations of JavaScript built-ins like console.log

use crate::JSValue;
use crate::string::{StringHeader, js_string_from_bytes};

/// Returns true if the f64 value is negative zero (-0.0).
/// Uses bit pattern comparison so +0.0 and -0.0 are distinguished
/// (they compare equal with normal `==`).
#[inline]
fn is_negative_zero(n: f64) -> bool {
    n.to_bits() == 0x8000_0000_0000_0000u64
}

/// Format a finite, non-zero, non-integer-like f64 per ECMAScript
/// NumberToString. Caller has already filtered NaN / ±Infinity / ±0 /
/// integer-shaped values; this only decides decimal vs scientific
/// notation per the |n| < 10^-6 / |n| >= 10^21 thresholds.
///
/// Without the threshold split, Rust's Display impl produces 300-digit
/// decimals for `Number.MAX_VALUE` (`1.7976931348623157e+308` → 309
/// zeros) and 16-digit `0.000…0002…` decimals for `Number.EPSILON`,
/// neither of which matches Node.
#[inline]
fn format_finite_number_js(value: f64) -> String {
    let abs = value.abs();
    if abs >= 1e21 || abs < 1e-6 {
        crate::string::fix_exponent_format(&format!("{:e}", value))
    } else {
        format!("{}", value)
    }
}

/// Decode the textual content of any string-shaped JSValue (heap
/// `STRING_TAG` or inline `SHORT_STRING_TAG`) into a fresh `String`.
/// Returns `None` for non-string values. SSO values are decoded
/// inline via the value's NaN-box payload — no heap touch.
///
/// Centralizes the SSO-aware dispatch every print/format/coerce
/// path needs: pre-SSO (≤ v0.5.215), the `is_string()` check used
/// throughout this file rejected SSO so any short string returned
/// by `JSON.parse` (e.g. `"perry"` from `{"foo":"perry"}`) fell
/// through to the "regular number" branch and printed as `NaN`
/// (because SHORT_STRING_TAG bits are NaN bits).
fn jsvalue_string_content(value: f64) -> Option<String> {
    let mut scratch = [0u8; crate::value::SHORT_STRING_MAX_LEN];
    let (ptr, len) = crate::string::str_bytes_from_jsvalue(value, &mut scratch)?;
    if ptr.is_null() {
        return Some(String::new());
    }
    unsafe {
        let bytes = std::slice::from_raw_parts(ptr, len as usize);
        Some(
            std::str::from_utf8(bytes)
                .unwrap_or("[invalid utf8]")
                .to_string(),
        )
    }
}

/// Print a value to stdout (console.log implementation)
#[no_mangle]
pub extern "C" fn js_console_log(value: JSValue) {
    if value.is_undefined() {
        println!("undefined");
    } else if value.is_null() {
        println!("null");
    } else if value.is_bool() {
        println!("{}", value.as_bool());
    } else if value.is_number() {
        let n = value.as_number();
        // Match Node/V8 console.log semantics: distinguish -0 from 0
        if is_negative_zero(n) {
            println!("-0");
        } else if n.fract() == 0.0 && n.abs() < (i64::MAX as f64) {
            // Print integers without decimal point
            println!("{}", n as i64);
        } else {
            println!("{}", format_finite_number_js(n));
        }
    } else if value.is_int32() {
        println!("{}", value.as_int32());
    } else {
        println!("{:?}", value);
    }
}

/// Print a dynamic value to stdout (for union types, etc.)
/// Takes an f64 that uses proper NaN-boxing to distinguish types.
/// - Numbers are stored as regular f64 values
/// - Strings are stored as NaN-boxed pointers (tag 0x7FFF)
/// - Objects are stored as NaN-boxed pointers (tag 0x7FFD)
#[no_mangle]
pub extern "C" fn js_console_log_dynamic(value: f64) {
    let jsval = JSValue::from_bits(value.to_bits());
    let p = console_group_prefix();

    if jsval.is_undefined() {
        println!("{}undefined", p);
    } else if jsval.is_null() {
        println!("{}null", p);
    } else if jsval.is_bool() {
        println!("{}{}", p, jsval.as_bool());
    } else if jsval.is_any_string() {
        // Heap STRING_TAG or inline SHORT_STRING_TAG (SSO).
        match jsvalue_string_content(value) {
            Some(s) => println!("{}{}", p, s),
            None => println!("{}null", p),
        }
    } else if jsval.is_pointer() {
        // Object/array pointer - format as JSON
        println!("{}{}", p, format_jsvalue(value, 0));
    } else if jsval.is_bigint() {
        // Bigint — defer to format_jsvalue which already prints the
        // "<digits>n" form. Without this, the fall-through below
        // treats the NaN-tagged bits as a raw double and prints
        // `NaN` for every single-arg `console.log(x)` where x is a
        // bigint (refs GH #33).
        println!("{}{}", p, format_jsvalue(value, 0));
    } else if jsval.is_int32() {
        println!("{}{}", p, jsval.as_int32());
    } else {
        // Must be a regular number — but first check for a raw (non-NaN-boxed)
        // heap pointer. The codegen returns Buffer pointers as
        // raw `i64` bitcast to `f64` (no POINTER_TAG), so `is_pointer()` is
        // false yet the bit pattern is a valid buffer address. Detect by
        // looking up the raw bits in the thread-local BUFFER_REGISTRY.
        let raw_bits = value.to_bits();
        if raw_bits > 0x1000 && (raw_bits >> 48) == 0
            && (crate::typedarray::lookup_typed_array_kind(raw_bits as usize).is_some()
                || crate::buffer::is_registered_buffer(raw_bits as usize))
        {
            println!("{}{}", p, format_jsvalue(value, 0));
            return;
        }
        let n = value;
        if n.is_nan() {
            println!("{}NaN", p);
        } else if n.is_infinite() {
            if n > 0.0 { println!("{}Infinity", p); } else { println!("{}-Infinity", p); }
        } else if is_negative_zero(n) {
            println!("{}-0", p);
        } else if n.fract() == 0.0 && n.abs() < (i64::MAX as f64) {
            println!("{}{}", p, n as i64);
        } else {
            println!("{}{}", p, n);
        }
    }
}

/// Thunk for `console.log` exposed as a real callable closure value
/// (#236). Lets `Promise.resolve(x).then(console.log)` actually call into
/// `js_console_log_dynamic` instead of being a no-op sentinel; the call
/// signature `extern "C" fn(*const ClosureHeader, f64) -> f64` matches
/// what `js_closure_call1` invokes through.
extern "C" fn console_log_callable_thunk(
    _closure: *const crate::closure::ClosureHeader,
    value: f64,
) -> f64 {
    js_console_log_dynamic(value);
    f64::from_bits(crate::value::TAG_UNDEFINED)
}

use std::sync::atomic::{AtomicI64, Ordering};
/// Singleton closure pointer for `console.log` exposed as a value.
/// Allocated lazily by `js_console_log_as_closure`. Kept alive across GC
/// cycles by the `scan_console_log_singleton_roots` scanner registered in
/// `gc::gc_init`.
static CONSOLE_LOG_SINGLETON: AtomicI64 = AtomicI64::new(0);

/// Returns a singleton ClosureHeader pointer that, when invoked through
/// `js_closure_call1`, calls `console.log` on the argument. Used by codegen
/// for the `let f = console.log` / `.then(console.log)` shapes — pre-fix
/// (#236) those lowered to the sentinel `0.0` ClosurePtr and the chained
/// promise either hung (when `.then` was the consumer) or silently dropped
/// the value. Lazily allocated on first use; the closure carries no
/// captures so it's a single 16-byte allocation per process.
#[no_mangle]
pub extern "C" fn js_console_log_as_closure() -> f64 {
    let cached = CONSOLE_LOG_SINGLETON.load(Ordering::Acquire);
    let closure_ptr = if cached != 0 {
        cached as *mut crate::closure::ClosureHeader
    } else {
        let fresh = crate::closure::js_closure_alloc(
            console_log_callable_thunk as *const u8,
            0,
        );
        // CAS so concurrent first-use callers don't leak a closure.
        // The loser's allocation is unreachable by any user code path
        // and will be reclaimed by the next GC sweep — only the winner
        // is added to the root set via `scan_console_log_singleton_roots`.
        match CONSOLE_LOG_SINGLETON.compare_exchange(
            0,
            fresh as i64,
            Ordering::AcqRel,
            Ordering::Acquire,
        ) {
            Ok(_) => fresh,
            Err(winner) => winner as *mut crate::closure::ClosureHeader,
        }
    };
    f64::from_bits(JSValue::pointer(closure_ptr as *const u8).bits())
}

/// GC root scanner: pin the lazily-allocated `console.log`-as-closure
/// singleton against the next sweep.
pub fn scan_console_log_singleton_roots(mark: &mut dyn FnMut(f64)) {
    let cached = CONSOLE_LOG_SINGLETON.load(Ordering::Acquire);
    if cached != 0 {
        let v = JSValue::pointer(cached as *const u8);
        mark(f64::from_bits(v.bits()));
    }
}

/// Print a number to stdout (optimized path for known numbers)
#[no_mangle]
pub extern "C" fn js_console_log_number(value: f64) {
    if is_negative_zero(value) {
        println!("-0");
    } else if value.is_nan() {
        println!("NaN");
    } else if value.is_infinite() {
        if value > 0.0 { println!("Infinity"); } else { println!("-Infinity"); }
    } else if value.fract() == 0.0 && value.abs() < (i64::MAX as f64) {
        println!("{}", value as i64);
    } else {
        println!("{}", format_finite_number_js(value));
    }
}

/// Print an i32 to stderr (console.error)
#[no_mangle]
pub extern "C" fn js_console_error_i32(value: i32) {
    eprintln!("{}", value);
}

/// Print a dynamic value to stderr (console.error for union types)
#[no_mangle]
pub extern "C" fn js_console_error_dynamic(value: f64) {
    let jsval = JSValue::from_bits(value.to_bits());

    if jsval.is_undefined() {
        eprintln!("undefined");
    } else if jsval.is_null() {
        eprintln!("null");
    } else if jsval.is_bool() {
        eprintln!("{}", jsval.as_bool());
    } else if jsval.is_any_string() {
        match jsvalue_string_content(value) {
            Some(s) => eprintln!("{}", s),
            None => eprintln!("null"),
        }
    } else if jsval.is_pointer() {
        // Object/array pointer - format as JSON
        eprintln!("{}", format_jsvalue(value, 0));
    } else if jsval.is_int32() {
        eprintln!("{}", jsval.as_int32());
    } else {
        let n = value;
        if n.is_nan() {
            eprintln!("NaN");
        } else if n.is_infinite() {
            if n > 0.0 { eprintln!("Infinity"); } else { eprintln!("-Infinity"); }
        } else if is_negative_zero(n) {
            eprintln!("-0");
        } else if n.fract() == 0.0 && n.abs() < (i64::MAX as f64) {
            eprintln!("{}", n as i64);
        } else {
            eprintln!("{}", format_finite_number_js(n));
        }
    }
}

/// Print a number to stderr (console.error for numbers)
#[no_mangle]
pub extern "C" fn js_console_error_number(value: f64) {
    if is_negative_zero(value) {
        eprintln!("-0");
    } else if value.fract() == 0.0 && value.abs() < (i64::MAX as f64) {
        eprintln!("{}", value as i64);
    } else {
        eprintln!("{}", format_finite_number_js(value));
    }
}

/// Print an i32 to stderr (console.warn)
#[no_mangle]
pub extern "C" fn js_console_warn_i32(value: i32) {
    eprintln!("{}", value);
}

/// Print a dynamic value to stderr (console.warn for union types)
#[no_mangle]
pub extern "C" fn js_console_warn_dynamic(value: f64) {
    let jsval = JSValue::from_bits(value.to_bits());

    if jsval.is_undefined() {
        eprintln!("undefined");
    } else if jsval.is_null() {
        eprintln!("null");
    } else if jsval.is_bool() {
        eprintln!("{}", jsval.as_bool());
    } else if jsval.is_any_string() {
        match jsvalue_string_content(value) {
            Some(s) => eprintln!("{}", s),
            None => eprintln!("null"),
        }
    } else if jsval.is_pointer() {
        // Object/array pointer - format as JSON
        eprintln!("{}", format_jsvalue(value, 0));
    } else if jsval.is_int32() {
        eprintln!("{}", jsval.as_int32());
    } else {
        let n = value;
        if n.is_nan() {
            eprintln!("NaN");
        } else if n.is_infinite() {
            if n > 0.0 { eprintln!("Infinity"); } else { eprintln!("-Infinity"); }
        } else if is_negative_zero(n) {
            eprintln!("-0");
        } else if n.fract() == 0.0 && n.abs() < (i64::MAX as f64) {
            eprintln!("{}", n as i64);
        } else {
            eprintln!("{}", format_finite_number_js(n));
        }
    }
}

/// Print a number to stderr (console.warn for numbers)
#[no_mangle]
pub extern "C" fn js_console_warn_number(value: f64) {
    if is_negative_zero(value) {
        eprintln!("-0");
    } else if value.fract() == 0.0 && value.abs() < (i64::MAX as f64) {
        eprintln!("{}", value as i64);
    } else {
        eprintln!("{}", format_finite_number_js(value));
    }
}

/// Print an i32 to stdout
#[no_mangle]
pub extern "C" fn js_console_log_i32(value: i32) {
    println!("{}", value);
}

/// Print an i64 to stdout
#[no_mangle]
pub extern "C" fn js_console_log_i64(value: i64) {
    println!("{}", value);
}

/// Print multiple values from an array (console.log with spread support)
/// Takes a pointer to an ArrayHeader containing f64 values
/// Helper function to format a JSValue as a string (for spread arrays)
fn format_jsvalue(value: f64, depth: usize) -> String {
    // Prevent stack overflow with deeply nested structures
    if depth > 10 {
        return "[...]".to_string();
    }

    let jsval = JSValue::from_bits(value.to_bits());

    unsafe {
        if jsval.is_undefined() {
            "undefined".to_string()
        } else if jsval.is_null() {
            "null".to_string()
        } else if jsval.is_bool() {
            jsval.as_bool().to_string()
        } else if jsval.is_any_string() {
            jsvalue_string_content(value).unwrap_or_else(|| "null".to_string())
        } else if jsval.is_bigint() {
            // Format BigInt by converting to string
            let ptr = jsval.as_bigint_ptr();
            if ptr.is_null() {
                "null".to_string()
            } else {
                let str_ptr = crate::bigint::js_bigint_to_string(ptr);
                if str_ptr.is_null() {
                    "0n".to_string()
                } else {
                    let len = (*str_ptr).byte_len as usize;
                    let data = (str_ptr as *const u8).add(std::mem::size_of::<StringHeader>());
                    let bytes = std::slice::from_raw_parts(data, len);
                    let num_str = std::str::from_utf8(bytes).unwrap_or("0");
                    format!("{}n", num_str)
                }
            }
        } else if jsval.is_pointer() {
            let ptr: *const crate::array::ArrayHeader = jsval.as_pointer();
            if ptr.is_null() {
                "null".to_string()
            } else if crate::symbol::is_registered_symbol(ptr as usize) {
                // Symbols print as "Symbol(description)" inside util.inspect.
                let s = crate::symbol::js_symbol_to_string(value);
                let s_ptr = s as *const StringHeader;
                if s_ptr.is_null() {
                    "Symbol()".to_string()
                } else {
                    let len = (*s_ptr).byte_len as usize;
                    let data = (s_ptr as *const u8).add(std::mem::size_of::<StringHeader>());
                    let bytes = std::slice::from_raw_parts(data, len);
                    std::str::from_utf8(bytes).unwrap_or("Symbol()").to_string()
                }
            } else if crate::typedarray::lookup_typed_array_kind(ptr as usize).is_some() {
                // Typed array — Int32Array(N) [ a, b, c ] etc.
                let ta = ptr as *const crate::typedarray::TypedArrayHeader;
                crate::typedarray::format_typed_array(ta)
            } else if crate::buffer::is_registered_buffer(ptr as usize) {
                // Buffer/Uint8Array — Node prints as `<Buffer xx xx xx ...>`
                // (lowercase hex bytes separated by single spaces). Buffer
                // headers don't carry a GC header, so this check must happen
                // BEFORE the GC_HEADER_SIZE pointer arithmetic below (which
                // would read garbage one word before the BufferHeader).
                let buf_ptr = ptr as *const crate::buffer::BufferHeader;
                format_buffer_value(buf_ptr)
            } else {
                // Use GC header to determine the actual type of the object.
                // The GC header is located GC_HEADER_SIZE bytes before the user pointer.
                let gc_header = (ptr as *const u8).sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader;
                let gc_type = (*gc_header).obj_type;

                if gc_type == crate::gc::GC_TYPE_ERROR {
                    // Error object
                    let error_ptr = ptr as *const crate::error::ErrorHeader;
                    let name_ptr = (*error_ptr).name;
                    let message_ptr = (*error_ptr).message;

                    let name_str = if name_ptr.is_null() {
                        "Error".to_string()
                    } else {
                        let len = (*name_ptr).byte_len as usize;
                        let data = (name_ptr as *const u8).add(std::mem::size_of::<StringHeader>());
                        let bytes = std::slice::from_raw_parts(data, len);
                        std::str::from_utf8(bytes).unwrap_or("Error").to_string()
                    };

                    let message_str = if message_ptr.is_null() {
                        "".to_string()
                    } else {
                        let len = (*message_ptr).byte_len as usize;
                        let data = (message_ptr as *const u8).add(std::mem::size_of::<StringHeader>());
                        let bytes = std::slice::from_raw_parts(data, len);
                        std::str::from_utf8(bytes).unwrap_or("").to_string()
                    };

                    if message_str.is_empty() {
                        name_str
                    } else {
                        format!("{}: {}", name_str, message_str)
                    }
                } else if gc_type == crate::gc::GC_TYPE_ARRAY {
                    // Array — format as [ elem1, elem2, ... ] matching Node.js util.inspect.
                    // Node's default depth cap is 2: anything more than 2
                    // levels of nesting collapses to `[Array]`.
                    if depth > 2 {
                        return "[Array]".to_string();
                    }
                    let maybe_arr = ptr;
                    let length = (*maybe_arr).length as usize;
                    if length == 0 {
                        return "[]".to_string();
                    }
                    let data_ptr = (maybe_arr as *const u8).add(std::mem::size_of::<crate::array::ArrayHeader>()) as *const f64;
                    let mut parts: Vec<String> = Vec::with_capacity(length);
                    let mut all_numeric = true;
                    for i in 0..length {
                        let elem_value = *data_ptr.add(i);
                        let elem_jsval = JSValue::from_bits(elem_value.to_bits());
                        // Quote string elements like Node's util.inspect: 'hello'
                        if elem_jsval.is_any_string() {
                            all_numeric = false;
                            let s = format_jsvalue(elem_value, depth + 1);
                            parts.push(format!("'{}'", s));
                        } else {
                            if !elem_jsval.is_number() && !elem_jsval.is_int32() {
                                all_numeric = false;
                            }
                            parts.push(format_jsvalue(elem_value, depth + 1));
                        }
                    }
                    let inner = parts.join(", ");
                    // Node uses multi-line when length > 6 or single-line exceeds breakLength (76)
                    let use_multiline = length > 6 || inner.len() + 4 > 76;
                    if !use_multiline {
                        format!("[ {} ]", inner)
                    } else if all_numeric {
                        // Node.js groupArrayElements for numeric arrays:
                        // right-align each number to max width, compute per-line
                        // column count via Node's sqrt heuristic.
                        let max_len = parts.iter().map(|s| s.len()).max().unwrap_or(1);
                        // biasedMax = max(maxLength - 2, 1)
                        let biased_max = max_len.saturating_sub(2).max(1);
                        // cols_by_sqrt = round(sqrt(2.5 * biasedMax * N) / biasedMax)
                        let cols_by_sqrt = ((2.5_f64 * biased_max as f64 * length as f64).sqrt()
                            / biased_max as f64)
                            .round() as usize;
                        // cols_by_width = ceil(breakLength / (maxLen + 2)); breakLength=76
                        let actual_max = max_len + 2;
                        let cols_by_width = (76 + actual_max - 1) / actual_max;
                        let columns = cols_by_sqrt
                            .min(cols_by_width.max(1))
                            .min(12) // compact(3) * 4
                            .min(15) // absolute max per Node
                            .max(1);
                        let indent = "  ";
                        let mut lines: Vec<String> = parts
                            .chunks(columns)
                            .map(|chunk| {
                                let elems: Vec<String> = chunk
                                    .iter()
                                    .map(|s| format!("{:>width$}", s, width = max_len))
                                    .collect();
                                format!("{}{}", indent, elems.join(", "))
                            })
                            .collect();
                        // Trailing comma on every line but the last (Node format)
                        let n_lines = lines.len();
                        for line in lines.iter_mut().take(n_lines - 1) {
                            line.push(',');
                        }
                        format!("[\n{}\n]", lines.join("\n"))
                    } else {
                        // Non-numeric multi-line: 4 per line, no padding
                        let indent = "  ";
                        let mut row_strs: Vec<String> = parts
                            .chunks(4)
                            .map(|chunk| format!("{}{}", indent, chunk.join(", ")))
                            .collect();
                        let n = row_strs.len();
                        for line in row_strs.iter_mut().take(n - 1) {
                            line.push(',');
                        }
                        format!("[\n{}\n]", row_strs.join("\n"))
                    }
                } else if gc_type == crate::gc::GC_TYPE_OBJECT {
                    // Object — check for keys_array. Node's default depth
                    // cap is 2: anything past that collapses to `[Object]`.
                    if depth > 2 {
                        return "[Object]".to_string();
                    }
                    let obj_ptr = ptr as *const crate::object::ObjectHeader;
                    let keys_array = (*obj_ptr).keys_array;

                    if !keys_array.is_null() && (keys_array as usize) > 0x10000 && ((keys_array as u64) >> 48) == 0 {
                        format_object_as_json(obj_ptr, depth)
                    } else {
                        "[object Object]".to_string()
                    }
                } else if gc_type == crate::gc::GC_TYPE_MAP {
                    "Map {}".to_string()
                } else if gc_type == crate::gc::GC_TYPE_CLOSURE {
                    "[Function (anonymous)]".to_string()
                } else if gc_type == crate::gc::GC_TYPE_PROMISE {
                    "Promise { <pending> }".to_string()
                } else {
                    // Safe fallback for unknown GC types — avoid heuristic
                    // pointer interpretation which can crash on closures,
                    // sets, maps, etc.
                    "[object Object]".to_string()
                }
            }
        } else if jsval.is_int32() {
            jsval.as_int32().to_string()
        } else {
            // Regular number — but first check for raw (non-NaN-boxed) heap
            // pointers. The codegen sometimes returns a raw
            // i64 buffer pointer bitcast directly to f64 (no POINTER_TAG), so
            // `jsval.is_pointer()` is false yet the bit pattern is a valid
            // buffer address. Detect this case by looking up the raw bits
            // in the thread-local BUFFER_REGISTRY.
            let raw_bits = value.to_bits();
            if raw_bits > 0x1000 && (raw_bits >> 48) == 0 {
                if crate::typedarray::lookup_typed_array_kind(raw_bits as usize).is_some() {
                    let ta = raw_bits as *const crate::typedarray::TypedArrayHeader;
                    return crate::typedarray::format_typed_array(ta);
                }
                if crate::buffer::is_registered_buffer(raw_bits as usize) {
                    let buf_ptr = raw_bits as *const crate::buffer::BufferHeader;
                    return format_buffer_value(buf_ptr);
                }
            }
            let n = value;
            if n.is_nan() {
                "NaN".to_string()
            } else if n.is_infinite() {
                if n > 0.0 { "Infinity".to_string() } else { "-Infinity".to_string() }
            } else if is_negative_zero(n) {
                "-0".to_string()
            } else if n.fract() == 0.0 && n.abs() < (i64::MAX as f64) {
                (n as i64).to_string()
            } else {
                format_finite_number_js(n)
            }
        }
    }
}

/// Format a Node.js Buffer as `<Buffer xx yy zz ...>` (lowercase hex bytes
/// separated by single spaces). Mirrors Node's `util.inspect` output for
/// Buffer / Uint8Array. Node truncates after 50 bytes with `... N more bytes`
/// but we emit the whole buffer for now (tests use small buffers).
unsafe fn format_buffer_value(buf_ptr: *const crate::buffer::BufferHeader) -> String {
    if buf_ptr.is_null() {
        return "<Buffer >".to_string();
    }
    let len = (*buf_ptr).length as usize;
    let data = (buf_ptr as *const u8).add(std::mem::size_of::<crate::buffer::BufferHeader>());
    let bytes = std::slice::from_raw_parts(data, len);

    // If this buffer was created via `new Uint8Array(...)`, format it Node-style
    // as `Uint8Array(N) [ a, b, c ]` rather than `<Buffer aa bb cc>`.
    if crate::buffer::is_uint8array_buffer(buf_ptr as usize) {
        if len == 0 {
            return "Uint8Array(0) []".to_string();
        }
        let mut out = format!("Uint8Array({}) [", len);
        for (i, b) in bytes.iter().enumerate() {
            if i == 0 { out.push(' '); } else { out.push_str(", "); }
            out.push_str(&format!("{}", *b));
        }
        out.push_str(" ]");
        return out;
    }

    // Node caps at 50 bytes then shows "... N more bytes"
    let display_len = len.min(50);
    let mut out = String::with_capacity(9 + display_len * 3);
    out.push_str("<Buffer");
    for b in &bytes[..display_len] {
        out.push(' ');
        out.push_str(&format!("{:02x}", b));
    }
    if len > display_len {
        out.push_str(&format!(" ... {} more bytes", len - display_len));
    }
    out.push('>');
    out
}

/// Format an object as JSON-like string
/// Reads keys from the keys_array and values from the fields.
///
/// `depth` is the current nesting level: `format_jsvalue`/`format_jsvalue_for_json`
/// invoke this with `depth = 0` for the outermost object, and each nested
/// object recurses with `depth + 1`. The hard cap at depth > 10 remains as a
/// crash safety net for cyclic structures; the Node-style `[Object]` truncation
/// at depth > 2 is enforced by `format_jsvalue_for_json` on the way in.
unsafe fn format_object_as_json(obj_ptr: *const crate::object::ObjectHeader, depth: usize) -> String {
    if depth > 10 {
        return "{...}".to_string();
    }

    let keys_array = (*obj_ptr).keys_array;
    if keys_array.is_null() {
        return "{}".to_string();
    }

    let key_count = crate::array::js_array_length(keys_array) as usize;
    if key_count == 0 {
        return "{}".to_string();
    }

    let mut parts: Vec<String> = Vec::with_capacity(key_count);

    for i in 0..key_count {
        // Get the key (NaN-boxed string pointer)
        let key_val = crate::array::js_array_get(keys_array, i as u32);
        let key_str = if key_val.is_string() {
            let key_ptr = key_val.as_string_ptr();
            if key_ptr.is_null() {
                continue;
            }
            let len = (*key_ptr).byte_len as usize;
            let data = (key_ptr as *const u8).add(std::mem::size_of::<StringHeader>());
            let bytes = std::slice::from_raw_parts(data, len);
            std::str::from_utf8(bytes).unwrap_or("").to_string()
        } else {
            continue;
        };

        // Get the value
        let value = crate::object::js_object_get_field_f64(obj_ptr, i as u32);
        let value_str = format_jsvalue_for_json(value, depth + 1);

        parts.push(format!("{}: {}", key_str, value_str));
    }

    format!("{{ {} }}", parts.join(", "))
}

/// Format a JSValue for JSON output (strings get quotes)
///
/// Node's `util.inspect` default options truncate nested objects at depth 2 —
/// anything past that prints as `[Object]` / `[Array]`. We mirror that so
/// `console.log({ a: { b: { c: { d: 1 } } } })` matches Node byte-for-byte.
/// The hard guard at depth > 10 remains as a crash safety net for pathological
/// cyclic structures.
fn format_jsvalue_for_json(value: f64, depth: usize) -> String {
    if depth > 10 {
        return "\"...\"".to_string();
    }

    let jsval = JSValue::from_bits(value.to_bits());

    unsafe {
        if jsval.is_undefined() {
            "undefined".to_string()
        } else if jsval.is_null() {
            "null".to_string()
        } else if jsval.is_bool() {
            jsval.as_bool().to_string()
        } else if jsval.is_any_string() {
            // Escape and quote strings for JSON-like output. SSO + heap
            // strings handled identically via the central decoder.
            let s = jsvalue_string_content(value).unwrap_or_default();
            format!("'{}'", escape_string(&s))
        } else if jsval.is_bigint() {
            let ptr = jsval.as_bigint_ptr();
            if ptr.is_null() {
                "null".to_string()
            } else {
                let str_ptr = crate::bigint::js_bigint_to_string(ptr);
                if str_ptr.is_null() {
                    "0n".to_string()
                } else {
                    let len = (*str_ptr).byte_len as usize;
                    let data = (str_ptr as *const u8).add(std::mem::size_of::<StringHeader>());
                    let bytes = std::slice::from_raw_parts(data, len);
                    let num_str = std::str::from_utf8(bytes).unwrap_or("0");
                    format!("{}n", num_str)
                }
            }
        } else if jsval.is_pointer() {
            let ptr: *const crate::array::ArrayHeader = jsval.as_pointer();
            if ptr.is_null() {
                "null".to_string()
            } else {
                // First check if this is an Error object
                let object_type = *(ptr as *const u32);
                if object_type == crate::error::OBJECT_TYPE_ERROR {
                    // Format Error as "Error: <message>"
                    let error_ptr = ptr as *const crate::error::ErrorHeader;
                    let name_ptr = (*error_ptr).name;
                    let message_ptr = (*error_ptr).message;

                    let name_str = if name_ptr.is_null() {
                        "Error".to_string()
                    } else {
                        let len = (*name_ptr).byte_len as usize;
                        let data = (name_ptr as *const u8).add(std::mem::size_of::<StringHeader>());
                        let bytes = std::slice::from_raw_parts(data, len);
                        std::str::from_utf8(bytes).unwrap_or("Error").to_string()
                    };

                    let message_str = if message_ptr.is_null() {
                        "".to_string()
                    } else {
                        let len = (*message_ptr).byte_len as usize;
                        let data = (message_ptr as *const u8).add(std::mem::size_of::<StringHeader>());
                        let bytes = std::slice::from_raw_parts(data, len);
                        std::str::from_utf8(bytes).unwrap_or("").to_string()
                    };

                    if message_str.is_empty() {
                        name_str
                    } else {
                        format!("{}: {}", name_str, message_str)
                    }
                } else {
                    // Use GC type header to determine the actual type
                    // instead of heuristic pointer checks which can
                    // misinterpret arrays as objects or vice versa.
                    let gc_header = (ptr as *const u8).sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader;
                    let gc_type = (*gc_header).obj_type;

                    if gc_type == crate::gc::GC_TYPE_ARRAY {
                        // Node's default depth cap: beyond 2 levels of
                        // nesting, arrays collapse to `[Array]`.
                        if depth > 2 {
                            return "[Array]".to_string();
                        }
                        let maybe_arr = ptr;
                        let length = (*maybe_arr).length as usize;
                        if length > 1_000_000 {
                            return "[Array]".to_string();
                        }
                        let data_ptr = (maybe_arr as *const u8).add(std::mem::size_of::<crate::array::ArrayHeader>()) as *const f64;
                        let mut parts: Vec<String> = Vec::with_capacity(length);
                        for i in 0..length {
                            let elem_value = *data_ptr.add(i);
                            parts.push(format_jsvalue_for_json(elem_value, depth + 1));
                        }
                        // Node formats empty arrays as `[]` and non-empty
                        // arrays with a space inside the brackets:
                        // `[ 1, 2, 3 ]`. Match byte-for-byte.
                        if length == 0 {
                            "[]".to_string()
                        } else {
                            format!("[ {} ]", parts.join(", "))
                        }
                    } else if gc_type == crate::gc::GC_TYPE_OBJECT {
                        // Past Node's default depth cap, nested objects
                        // collapse to the literal token `[Object]`.
                        if depth > 2 {
                            return "[Object]".to_string();
                        }
                        let obj_ptr = ptr as *const crate::object::ObjectHeader;
                        let keys_array = (*obj_ptr).keys_array;
                        if !keys_array.is_null() && (keys_array as usize) > 0x10000 && ((keys_array as u64) >> 48) == 0 {
                            format_object_as_json(obj_ptr, depth)
                        } else {
                            "[object Object]".to_string()
                        }
                    } else {
                        "[object Object]".to_string()
                    }
                }
            }
        } else if jsval.is_int32() {
            jsval.as_int32().to_string()
        } else {
            let n = value;
            if n.is_nan() {
                "NaN".to_string()
            } else if n.is_infinite() {
                if n > 0.0 { "Infinity".to_string() } else { "-Infinity".to_string() }
            } else if is_negative_zero(n) {
                "-0".to_string()
            } else if n.fract() == 0.0 && n.abs() < (i64::MAX as f64) {
                (n as i64).to_string()
            } else {
                format_finite_number_js(n)
            }
        }
    }
}

/// Escape special characters in a string for display
fn escape_string(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '\\' => result.push_str("\\\\"),
            '\'' => result.push_str("\\'"),
            '\n' => result.push_str("\\n"),
            '\r' => result.push_str("\\r"),
            '\t' => result.push_str("\\t"),
            _ => result.push(c),
        }
    }
    result
}

#[no_mangle]
pub extern "C" fn js_console_log_spread(arr_ptr: *const crate::array::ArrayHeader) {
    if arr_ptr.is_null() {
        println!();
        return;
    }

    unsafe {
        let length = (*arr_ptr).length as usize;
        let data_ptr = (arr_ptr as *const u8).add(std::mem::size_of::<crate::array::ArrayHeader>()) as *const f64;

        let mut parts: Vec<String> = Vec::with_capacity(length);
        for i in 0..length {
            let value = *data_ptr.add(i);
            parts.push(format_jsvalue(value, 0));
        }
        println!("{}", parts.join(" "));
    }
}

/// Print multiple values to stderr (console.error with spread support)
#[no_mangle]
pub extern "C" fn js_console_error_spread(arr_ptr: *const crate::array::ArrayHeader) {
    if arr_ptr.is_null() {
        eprintln!();
        return;
    }

    unsafe {
        let length = (*arr_ptr).length as usize;
        let data_ptr = (arr_ptr as *const u8).add(std::mem::size_of::<crate::array::ArrayHeader>()) as *const f64;

        let mut parts: Vec<String> = Vec::with_capacity(length);
        for i in 0..length {
            let value = *data_ptr.add(i);
            parts.push(format_jsvalue(value, 0));
        }
        eprintln!("{}", parts.join(" "));
    }
}

/// Print multiple values to stderr (console.warn with spread support)
#[no_mangle]
pub extern "C" fn js_console_warn_spread(arr_ptr: *const crate::array::ArrayHeader) {
    // console.warn is essentially the same as console.error in Node.js
    js_console_error_spread(arr_ptr);
}

/// Print an array in the format [element1, element2, ...]
#[no_mangle]
pub extern "C" fn js_array_print(arr_ptr: *const crate::array::ArrayHeader) {
    if arr_ptr.is_null() {
        println!("null");
        return;
    }

    unsafe {
        let length = (*arr_ptr).length as usize;
        let data_ptr = (arr_ptr as *const u8).add(std::mem::size_of::<crate::array::ArrayHeader>()) as *const f64;

        let mut parts: Vec<String> = Vec::with_capacity(length);
        for i in 0..length {
            let value = *data_ptr.add(i);
            parts.push(format_jsvalue_for_json(value, 0));
        }
        println!("[{}]", parts.join(", "));
    }
}

// Arithmetic operations on JSValue (with type coercion)

#[no_mangle]
pub extern "C" fn js_add(a: JSValue, b: JSValue) -> JSValue {
    // For MVP, just handle number + number
    JSValue::number(a.to_number() + b.to_number())
}

#[no_mangle]
pub extern "C" fn js_sub(a: JSValue, b: JSValue) -> JSValue {
    JSValue::number(a.to_number() - b.to_number())
}

#[no_mangle]
pub extern "C" fn js_mul(a: JSValue, b: JSValue) -> JSValue {
    JSValue::number(a.to_number() * b.to_number())
}

#[no_mangle]
pub extern "C" fn js_div(a: JSValue, b: JSValue) -> JSValue {
    JSValue::number(a.to_number() / b.to_number())
}

#[no_mangle]
pub extern "C" fn js_mod(a: JSValue, b: JSValue) -> JSValue {
    JSValue::number(a.to_number() % b.to_number())
}

// Comparison operations

#[no_mangle]
pub extern "C" fn js_eq(a: JSValue, b: JSValue) -> JSValue {
    // Delegate to the SSO-aware strict-equality entry in value.rs,
    // which already handles cross-representation string compares
    // (heap STRING_TAG + inline SHORT_STRING_TAG, in any order) plus
    // BigInt-by-value, INT32-vs-f64, and the negative-zero / NaN
    // edge cases. The previous implementation was bit-equality with
    // a number-only special case — `JSON.parse(...).foo === "perry"`
    // returned `false` because the JSON parser emits SSO for ≤ 5-byte
    // strings while `"perry"` literals are interned to heap strings,
    // and the bits diverge across representations even when the text
    // is identical.
    let result = crate::value::js_jsvalue_equals(
        f64::from_bits(a.bits()),
        f64::from_bits(b.bits()),
    );
    JSValue::bool(result != 0)
}

/// JS abstract equality (==). Implements the coercion rules:
/// - Same type: use strict equality
/// - null == undefined: true
/// - number == string: coerce string to number
/// - boolean == anything: coerce boolean to number, recurse
/// - string == number: coerce string to number
#[no_mangle]
pub extern "C" fn js_loose_eq(a: JSValue, b: JSValue) -> JSValue {
    // Both numbers FIRST: IEEE 754 equality correctly handles NaN!=NaN
    // (NaN has well-defined bits, so the later same-bits fast path
    // would otherwise incorrectly return true for NaN==NaN). Also
    // handles +0 == -0 correctly (different bits, IEEE 754 says equal).
    if a.is_number() && b.is_number() {
        return JSValue::bool(a.as_number() == b.as_number());
    }
    // Same bits → always equal (handles null==null, undefined==undefined,
    // identical pointers, identical SSO encodings, etc.)
    if a.bits() == b.bits() {
        return JSValue::bool(true);
    }
    // null == undefined (and vice versa)
    if (a.is_null() && b.is_undefined()) || (a.is_undefined() && b.is_null()) {
        return JSValue::bool(true);
    }
    // null/undefined != anything else
    if a.is_null() || a.is_undefined() || b.is_null() || b.is_undefined() {
        return JSValue::bool(false);
    }
    // Both strings (heap STRING_TAG and/or inline SHORT_STRING_TAG):
    // content compare. The previous `is_string() && is_string()` test
    // missed any SSO operand — `JSON.parse(...).foo == "perry"` returned
    // false because the JSON parser emits SSO for ≤5-byte strings while
    // string literals are interned to heap strings, and the bit patterns
    // diverged across representations even with identical text.
    if a.is_any_string() && b.is_any_string() {
        let result = crate::value::js_jsvalue_equals(
            f64::from_bits(a.bits()),
            f64::from_bits(b.bits()),
        );
        return JSValue::bool(result != 0);
    }
    // Boolean on either side: coerce to number and recurse
    if a.is_bool() {
        let a_num = if a.as_bool() { 1.0 } else { 0.0 };
        return js_loose_eq(JSValue::number(a_num), b);
    }
    if b.is_bool() {
        let b_num = if b.as_bool() { 1.0 } else { 0.0 };
        return js_loose_eq(a, JSValue::number(b_num));
    }
    // String vs number: coerce string to number. `is_any_string` so
    // SSO operands get the same coercion as heap strings.
    if a.is_number() && b.is_any_string() {
        let b_num = js_number_coerce(f64::from_bits(b.bits()));
        return JSValue::bool(a.as_number() == b_num);
    }
    if a.is_any_string() && b.is_number() {
        let a_num = js_number_coerce(f64::from_bits(a.bits()));
        return JSValue::bool(a_num == b.as_number());
    }
    // Fallback: not equal
    JSValue::bool(false)
}

#[no_mangle]
pub extern "C" fn js_lt(a: JSValue, b: JSValue) -> JSValue {
    JSValue::bool(a.to_number() < b.to_number())
}

#[no_mangle]
pub extern "C" fn js_le(a: JSValue, b: JSValue) -> JSValue {
    JSValue::bool(a.to_number() <= b.to_number())
}

#[no_mangle]
pub extern "C" fn js_gt(a: JSValue, b: JSValue) -> JSValue {
    JSValue::bool(a.to_number() > b.to_number())
}

#[no_mangle]
pub extern "C" fn js_ge(a: JSValue, b: JSValue) -> JSValue {
    JSValue::bool(a.to_number() >= b.to_number())
}

/// Return the typeof a value as a string
/// Takes an f64 that uses NaN-boxing to distinguish types.
/// Returns a pointer to a string: "undefined", "boolean", "number", "string", "object", "function"
///
/// Optimization: typeof only returns 7 possible strings, so we cache them as
/// pre-allocated StringHeader pointers to avoid heap allocation on every call.
#[no_mangle]
pub extern "C" fn js_value_typeof(value: f64) -> *mut StringHeader {
    use std::cell::Cell;

    thread_local! {
        static TYPEOF_UNDEFINED: Cell<*mut StringHeader> = const { Cell::new(std::ptr::null_mut()) };
        static TYPEOF_OBJECT:    Cell<*mut StringHeader> = const { Cell::new(std::ptr::null_mut()) };
        static TYPEOF_BOOLEAN:   Cell<*mut StringHeader> = const { Cell::new(std::ptr::null_mut()) };
        static TYPEOF_NUMBER:    Cell<*mut StringHeader> = const { Cell::new(std::ptr::null_mut()) };
        static TYPEOF_STRING:    Cell<*mut StringHeader> = const { Cell::new(std::ptr::null_mut()) };
        static TYPEOF_FUNCTION:  Cell<*mut StringHeader> = const { Cell::new(std::ptr::null_mut()) };
        static TYPEOF_BIGINT:    Cell<*mut StringHeader> = const { Cell::new(std::ptr::null_mut()) };
        static TYPEOF_SYMBOL:    Cell<*mut StringHeader> = const { Cell::new(std::ptr::null_mut()) };
    }

    /// Get or initialize a cached typeof string.
    fn get_cached(
        cache: &'static std::thread::LocalKey<Cell<*mut StringHeader>>,
        s: &str,
    ) -> *mut StringHeader {
        cache.with(|cell| {
            let ptr = cell.get();
            if !ptr.is_null() {
                return ptr;
            }
            let new_ptr = crate::string::js_string_from_bytes(s.as_ptr(), s.len() as u32);
            cell.set(new_ptr);
            new_ptr
        })
    }

    let jsval = JSValue::from_bits(value.to_bits());

    if jsval.is_undefined() {
        get_cached(&TYPEOF_UNDEFINED, "undefined")
    } else if jsval.is_null() {
        // typeof null === "object" in JavaScript
        get_cached(&TYPEOF_OBJECT, "object")
    } else if jsval.is_bool() {
        get_cached(&TYPEOF_BOOLEAN, "boolean")
    } else if jsval.is_any_string() {
        // String pointer (STRING_TAG) OR inline SSO (SHORT_STRING_TAG).
        // `typeof` doesn't distinguish between representations — both
        // are observed as "string" from user code.
        get_cached(&TYPEOF_STRING, "string")
    } else if crate::value::is_js_handle(value) {
        // JS handle from V8 runtime — ask V8 whether it's a callable, otherwise default
        // to "object". Issue #258: pre-fix this always returned "object" even for
        // V8 functions; the registered callback now flips it to "function" when the
        // handle wraps a v8::Function.
        if crate::value::js_handle_is_function(value) {
            get_cached(&TYPEOF_FUNCTION, "function")
        } else {
            get_cached(&TYPEOF_OBJECT, "object")
        }
    } else if jsval.is_pointer() {
        // Object/array/closure/symbol pointer - check via the side-table first.
        let ptr = jsval.as_pointer::<u8>();
        if !ptr.is_null() && (ptr as usize) > 0x10000 {
            // Symbols: registered in SYMBOL_POINTERS (handles both gc_malloc'd
            // and Box-leaked symbols, which have no GcHeader).
            if crate::symbol::is_registered_symbol(ptr as usize) {
                get_cached(&TYPEOF_SYMBOL, "symbol")
            } else {
                // ClosureHeader has type_tag at offset 12 (after func_ptr:8 + capture_count:4)
                let type_tag = unsafe { *(ptr.add(12) as *const u32) };
                if type_tag == crate::closure::CLOSURE_MAGIC {
                    get_cached(&TYPEOF_FUNCTION, "function")
                } else {
                    get_cached(&TYPEOF_OBJECT, "object")
                }
            }
        } else {
            get_cached(&TYPEOF_OBJECT, "object")
        }
    } else if jsval.is_bigint() {
        get_cached(&TYPEOF_BIGINT, "bigint")
    } else if jsval.is_int32() {
        get_cached(&TYPEOF_NUMBER, "number")
    } else {
        // Regular f64 number
        get_cached(&TYPEOF_NUMBER, "number")
    }
}

/// parseInt(string, radix?) -> number
/// Parses a string and returns an integer.
/// If the string cannot be parsed, returns NaN.
#[no_mangle]
pub extern "C" fn js_parse_int(str_ptr: *const StringHeader, radix: f64) -> f64 {
    if str_ptr.is_null() || (str_ptr as usize) < 0x1000 {
        return f64::NAN;
    }

    unsafe {
        let len = (*str_ptr).byte_len as usize;
        let data = (str_ptr as *const u8).add(std::mem::size_of::<StringHeader>());
        let bytes = std::slice::from_raw_parts(data, len);

        if let Ok(s) = std::str::from_utf8(bytes) {
            let trimmed = s.trim();
            if trimmed.is_empty() {
                return f64::NAN;
            }

            // Determine radix
            let radix = if radix.is_nan() || radix == 0.0 {
                10
            } else {
                radix as u32
            };

            // Handle sign
            let (is_negative, trimmed) = if trimmed.starts_with('-') {
                (true, &trimmed[1..])
            } else if trimmed.starts_with('+') {
                (false, &trimmed[1..])
            } else {
                (false, trimmed)
            };

            // Handle hex prefix (only if radix is 16 or auto)
            let (actual_radix, trimmed) = if (radix == 16 || radix == 10) &&
                (trimmed.starts_with("0x") || trimmed.starts_with("0X")) {
                (16, &trimmed[2..])
            } else {
                (radix, trimmed)
            };

            // Parse characters until we hit a non-digit
            let valid_chars: String = trimmed.chars()
                .take_while(|c| c.is_digit(actual_radix))
                .collect();

            if valid_chars.is_empty() {
                return f64::NAN;
            }

            match i64::from_str_radix(&valid_chars, actual_radix) {
                Ok(n) => {
                    let result = if is_negative { -n } else { n };
                    result as f64
                }
                Err(_) => f64::NAN,
            }
        } else {
            f64::NAN
        }
    }
}

/// parseFloat(string) -> number
/// Parses a string and returns a floating-point number.
#[no_mangle]
pub extern "C" fn js_parse_float(str_ptr: *const StringHeader) -> f64 {
    if str_ptr.is_null() || (str_ptr as usize) < 0x1000 {
        return f64::NAN;
    }

    unsafe {
        let len = (*str_ptr).byte_len as usize;
        let data = (str_ptr as *const u8).add(std::mem::size_of::<StringHeader>());
        let bytes = std::slice::from_raw_parts(data, len);
        parse_float_bytes(bytes)
    }
}

/// Core parseFloat logic operating on raw bytes — no heap allocation.
/// Exposed as `pub(crate)` so unit tests can call it directly.
pub(crate) fn parse_float_bytes(bytes: &[u8]) -> f64 {
    // JS spec: strip leading StrWhiteSpace (ASCII subset covers all common cases)
    let bytes = bytes.trim_ascii_start();
    if bytes.is_empty() {
        return f64::NAN;
    }

    // Detect optional sign, then check for "Infinity"
    let (neg, rest) = match bytes.first() {
        Some(b'-') => (true, &bytes[1..]),
        Some(b'+') => (false, &bytes[1..]),
        _ => (false, bytes),
    };
    if rest.starts_with(b"Infinity") {
        return if neg { f64::NEG_INFINITY } else { f64::INFINITY };
    }

    // Scan for the longest valid StrDecimalLiteral prefix — zero allocations.
    let end = float_prefix_end(bytes);
    if end == 0 {
        return f64::NAN;
    }

    // bytes[..end] contains only ASCII chars (digits, sign, '.', 'e'/'E'), so
    // from_utf8_unchecked is safe.
    let s = unsafe { std::str::from_utf8_unchecked(&bytes[..end]) };
    s.parse::<f64>().unwrap_or(f64::NAN)
}

/// Returns the byte length of the leading StrDecimalLiteral prefix in `bytes`.
/// Returns 0 when no valid prefix exists (e.g. `"abc"`, `"."`, `"+"`).
fn float_prefix_end(bytes: &[u8]) -> usize {
    let mut i = 0;
    let n = bytes.len();

    // Optional sign
    if i < n && (bytes[i] == b'-' || bytes[i] == b'+') {
        i += 1;
    }

    // Integer digits
    let int_start = i;
    while i < n && bytes[i].is_ascii_digit() {
        i += 1;
    }
    let has_int = i > int_start;

    // Optional fractional part
    let mut has_frac = false;
    if i < n && bytes[i] == b'.' {
        i += 1;
        let frac_start = i;
        while i < n && bytes[i].is_ascii_digit() {
            i += 1;
        }
        has_frac = i > frac_start;
    }

    // Need at least one digit on either side of the (optional) decimal point
    if !has_int && !has_frac {
        return 0;
    }

    // Optional exponent — only consumed when at least one exponent digit follows
    if i < n && (bytes[i] == b'e' || bytes[i] == b'E') {
        let exp_start = i;
        i += 1;
        if i < n && (bytes[i] == b'-' || bytes[i] == b'+') {
            i += 1;
        }
        let exp_digit_start = i;
        while i < n && bytes[i].is_ascii_digit() {
            i += 1;
        }
        if i == exp_digit_start {
            i = exp_start; // backtrack: bare 'e' or 'e±' with no digits
        }
    }

    i
}

#[cfg(test)]
mod parse_float_tests {
    use super::parse_float_bytes;

    fn pf(s: &str) -> f64 {
        parse_float_bytes(s.as_bytes())
    }

    #[test]
    fn well_formed_inputs() {
        assert_eq!(pf("3.14"), 3.14_f64);
        assert_eq!(pf("1e10"), 1e10_f64);
        assert_eq!(pf("-0.5"), -0.5_f64);
        assert_eq!(pf("1234567890.12345"), 1234567890.12345_f64);
        assert_eq!(pf("0"), 0.0_f64);
        assert_eq!(pf("42"), 42.0_f64);
        assert_eq!(pf(".5"), 0.5_f64);
        assert_eq!(pf("5."), 5.0_f64);
        assert_eq!(pf("+3.14"), 3.14_f64);
    }

    #[test]
    fn leading_whitespace() {
        assert_eq!(pf("  3.14"), 3.14_f64);
        assert_eq!(pf("\t3.14"), 3.14_f64);
        assert_eq!(pf("\n3.14"), 3.14_f64);
    }

    #[test]
    fn trailing_junk() {
        assert_eq!(pf("3.14abc"), 3.14_f64);
        assert_eq!(pf("1e10xyz"), 1e10_f64);
        assert_eq!(pf("42 extra"), 42.0_f64);
        // bare 'e' with no exponent digits — stop before 'e'
        assert_eq!(pf("1e"), 1.0_f64);
        assert_eq!(pf("1e+"), 1.0_f64);
    }

    #[test]
    fn invalid_inputs_return_nan() {
        assert!(pf("abc").is_nan());
        assert!(pf("").is_nan());
        assert!(pf("   ").is_nan());
        assert!(pf(".").is_nan());
        assert!(pf("+").is_nan());
        assert!(pf("-").is_nan());
    }

    #[test]
    fn infinity_variants() {
        assert_eq!(pf("Infinity"), f64::INFINITY);
        assert_eq!(pf("-Infinity"), f64::NEG_INFINITY);
        assert_eq!(pf("+Infinity"), f64::INFINITY);
        assert_eq!(pf("  Infinity"), f64::INFINITY);
    }
}

/// Number(value) -> number
/// Converts a value to a number.
///
/// Marked `#[inline]` so the bitcode-link path can inline + DCE the
/// branches when the input type is statically known.
#[no_mangle]
#[inline]
pub extern "C" fn js_number_coerce(value: f64) -> f64 {
    let jsval = JSValue::from_bits(value.to_bits());

    if jsval.is_undefined() {
        f64::NAN
    } else if jsval.is_null() {
        0.0
    } else if jsval.is_bool() {
        if jsval.as_bool() { 1.0 } else { 0.0 }
    } else if jsval.is_any_string() {
        // Parse string as number. Accepts both STRING_TAG heap
        // pointers and SHORT_STRING_TAG inline SSO values
        // (v0.5.216). Decode via `str_bytes_from_jsvalue` into a
        // stack scratch buffer for SSO; heap strings get a direct
        // view over the StringHeader payload.
        let mut scratch = [0u8; crate::value::SHORT_STRING_MAX_LEN];
        let view = crate::string::str_bytes_from_jsvalue(value, &mut scratch);
        if let Some((data, len)) = view {
            if data.is_null() && len == 0 {
                return 0.0;
            }
            unsafe {
                let bytes = std::slice::from_raw_parts(data, len as usize);
                if let Ok(s) = std::str::from_utf8(bytes) {
                    let trimmed = s.trim();
                    if trimmed.is_empty() {
                        return 0.0;
                    }
                    if trimmed.starts_with("0x") || trimmed.starts_with("0X") {
                        return match u64::from_str_radix(&trimmed[2..], 16) {
                            Ok(n) => n as f64,
                            Err(_) => f64::NAN,
                        };
                    }
                    if trimmed.starts_with("-0x") || trimmed.starts_with("-0X") {
                        return match u64::from_str_radix(&trimmed[3..], 16) {
                            Ok(n) => -(n as f64),
                            Err(_) => f64::NAN,
                        };
                    }
                    match trimmed.parse::<f64>() {
                        Ok(n) => n,
                        Err(_) => f64::NAN,
                    }
                } else {
                    f64::NAN
                }
            }
        } else {
            f64::NAN
        }
    } else if jsval.is_int32() {
        // INT32 NaN-boxed value → convert to f64
        jsval.as_int32() as f64
    } else if jsval.is_bigint() {
        // BigInt → number conversion
        let ptr = jsval.as_bigint_ptr();
        crate::bigint::js_bigint_to_f64(ptr)
    } else if jsval.is_pointer() {
        // Object → consult [Symbol.toPrimitive]("number") first; if the
        // object has a custom toPrimitive method, recurse with the result.
        // Otherwise returns NaN.
        let primitive = unsafe { crate::symbol::js_to_primitive(value, 1) };
        if primitive.to_bits() != value.to_bits() {
            // toPrimitive returned something different — re-coerce.
            return js_number_coerce(primitive);
        }
        f64::NAN
    } else {
        // Already a number
        value
    }
}

/// String(value) -> string
/// Converts a value to a string.
#[no_mangle]
pub extern "C" fn js_string_coerce(value: f64) -> *mut StringHeader {
    let jsval = JSValue::from_bits(value.to_bits());

    let result = if jsval.is_undefined() {
        "undefined".to_string()
    } else if jsval.is_null() {
        "null".to_string()
    } else if jsval.is_bool() {
        if jsval.as_bool() { "true".to_string() } else { "false".to_string() }
    } else if jsval.is_string() {
        // Already a heap string, return as-is
        return jsval.as_string_ptr() as *mut StringHeader;
    } else if jsval.is_short_string() {
        // SSO inline value — caller wants a `*mut StringHeader`, so
        // materialize the inline bytes onto the heap. Defeats the SSO
        // win for this value but preserves correctness on coercion
        // paths (`String(x)`, `'' + x` via the runtime fallback, etc.)
        // that pass the result downstream as a heap pointer.
        return crate::string::js_string_materialize_to_heap(value);
    } else if jsval.is_bigint() {
        let ptr = jsval.as_bigint_ptr();
        if ptr.is_null() {
            "0".to_string()
        } else {
            let str_ptr = crate::bigint::js_bigint_to_string(ptr);
            return str_ptr as *mut StringHeader;
        }
    } else if jsval.is_pointer() {
        // Pointer type — could be array or object.
        // Delegate to js_jsvalue_to_string which handles arrays via join(",") and objects.
        return crate::value::js_jsvalue_to_string(value);
    } else if jsval.is_int32() {
        jsval.as_int32().to_string()
    } else {
        // Regular number
        let n = value;
        if n.is_nan() {
            "NaN".to_string()
        } else if n.is_infinite() {
            if n > 0.0 { "Infinity".to_string() } else { "-Infinity".to_string() }
        } else if n == 0.0 {
            "0".to_string()
        } else if n.fract() == 0.0 && n.abs() < (i64::MAX as f64) {
            (n as i64).to_string()
        } else {
            n.to_string()
        }
    };

    js_string_from_bytes(result.as_ptr(), result.len() as u32)
}

/// isNaN(value) -> boolean
/// Returns true if value is NaN.
#[no_mangle]
pub extern "C" fn js_is_nan(value: f64) -> f64 {
    let jsval = JSValue::from_bits(value.to_bits());

    // isNaN first coerces to number, then checks for NaN
    let num = if jsval.is_undefined() {
        f64::NAN
    } else if jsval.is_null() {
        0.0
    } else if jsval.is_bool() {
        if jsval.as_bool() { 1.0 } else { 0.0 }
    } else if jsval.is_string() {
        // Parse string as number
        let ptr = jsval.as_string_ptr();
        if ptr.is_null() {
            f64::NAN
        } else {
            unsafe {
                let len = (*ptr).byte_len as usize;
                let data = (ptr as *const u8).add(std::mem::size_of::<StringHeader>());
                let bytes = std::slice::from_raw_parts(data, len);
                if let Ok(s) = std::str::from_utf8(bytes) {
                    let trimmed = s.trim();
                    if trimmed.is_empty() {
                        0.0
                    } else {
                        match trimmed.parse::<f64>() {
                            Ok(n) => n,
                            Err(_) => f64::NAN,
                        }
                    }
                } else {
                    f64::NAN
                }
            }
        }
    } else {
        value
    };

    // Return NaN-boxed boolean (TAG_TRUE / TAG_FALSE)
    const TAG_TRUE: u64 = 0x7FFC_0000_0000_0004;
    const TAG_FALSE: u64 = 0x7FFC_0000_0000_0003;
    if num.is_nan() {
        f64::from_bits(TAG_TRUE)
    } else {
        f64::from_bits(TAG_FALSE)
    }
}

/// isFinite(value) -> boolean
/// Returns true if value is a finite number.
#[no_mangle]
pub extern "C" fn js_is_finite(value: f64) -> f64 {
    let jsval = JSValue::from_bits(value.to_bits());

    // isFinite first coerces to number, then checks for finite
    let num = if jsval.is_undefined() {
        f64::NAN
    } else if jsval.is_null() {
        0.0
    } else if jsval.is_bool() {
        if jsval.as_bool() { 1.0 } else { 0.0 }
    } else if jsval.is_string() {
        // Parse string as number
        let ptr = jsval.as_string_ptr();
        if ptr.is_null() {
            f64::NAN
        } else {
            unsafe {
                let len = (*ptr).byte_len as usize;
                let data = (ptr as *const u8).add(std::mem::size_of::<StringHeader>());
                let bytes = std::slice::from_raw_parts(data, len);
                if let Ok(s) = std::str::from_utf8(bytes) {
                    let trimmed = s.trim();
                    if trimmed.is_empty() {
                        0.0
                    } else {
                        match trimmed.parse::<f64>() {
                            Ok(n) => n,
                            Err(_) => f64::NAN,
                        }
                    }
                } else {
                    f64::NAN
                }
            }
        }
    } else {
        value
    };

    // Return NaN-boxed boolean (TAG_TRUE / TAG_FALSE)
    const TAG_TRUE: u64 = 0x7FFC_0000_0000_0004;
    const TAG_FALSE: u64 = 0x7FFC_0000_0000_0003;
    if num.is_finite() {
        f64::from_bits(TAG_TRUE)
    } else {
        f64::from_bits(TAG_FALSE)
    }
}

const NB_TAG_TRUE: u64 = 0x7FFC_0000_0000_0004;
const NB_TAG_FALSE: u64 = 0x7FFC_0000_0000_0003;

/// Number.isNaN(value) -> boolean (strict, no coercion)
/// Returns true only if value is a plain number AND that number is NaN.
#[no_mangle]
pub extern "C" fn js_number_is_nan(value: f64) -> f64 {
    let jsval = JSValue::from_bits(value.to_bits());
    // Strict: only plain numbers can be NaN. Any NaN-boxed tag type => false.
    if !jsval.is_number() {
        return f64::from_bits(NB_TAG_FALSE);
    }
    let n = jsval.as_number();
    if n.is_nan() {
        f64::from_bits(NB_TAG_TRUE)
    } else {
        f64::from_bits(NB_TAG_FALSE)
    }
}

/// Number.isFinite(value) -> boolean (strict, no coercion)
/// Returns true only if value is a plain finite number.
#[no_mangle]
pub extern "C" fn js_number_is_finite(value: f64) -> f64 {
    let jsval = JSValue::from_bits(value.to_bits());
    if !jsval.is_number() {
        return f64::from_bits(NB_TAG_FALSE);
    }
    let n = jsval.as_number();
    if n.is_finite() {
        f64::from_bits(NB_TAG_TRUE)
    } else {
        f64::from_bits(NB_TAG_FALSE)
    }
}

/// Number.isInteger(value) -> boolean
/// Returns true if value is a finite number with no fractional part.
#[no_mangle]
pub extern "C" fn js_number_is_integer(value: f64) -> f64 {
    let jsval = JSValue::from_bits(value.to_bits());
    if !jsval.is_number() {
        return f64::from_bits(NB_TAG_FALSE);
    }
    let n = jsval.as_number();
    if n.is_finite() && n.floor() == n {
        f64::from_bits(NB_TAG_TRUE)
    } else {
        f64::from_bits(NB_TAG_FALSE)
    }
}

/// Number.isSafeInteger(value) -> boolean
/// Returns true if value is an integer within ±(2^53 - 1).
#[no_mangle]
pub extern "C" fn js_number_is_safe_integer(value: f64) -> f64 {
    let jsval = JSValue::from_bits(value.to_bits());
    if !jsval.is_number() {
        return f64::from_bits(NB_TAG_FALSE);
    }
    let n = jsval.as_number();
    const MAX_SAFE: f64 = 9007199254740991.0;
    if n.is_finite() && n.floor() == n && n.abs() <= MAX_SAFE {
        f64::from_bits(NB_TAG_TRUE)
    } else {
        f64::from_bits(NB_TAG_FALSE)
    }
}

/// Debug trace for module initialization order.
/// Called before each _perry_init_* call to identify which module crashes.
/// No-op in release builds; re-enable eprintln for debugging.
#[no_mangle]
pub extern "C" fn perry_debug_trace_init(_index: i64, _name_ptr: *const u8, _name_len: i64) {
}

#[no_mangle]
pub extern "C" fn perry_debug_trace_init_done(_index: i64) {
}

// === console.time / timeEnd / timeLog ===
//
// Per-thread map from label string to start Instant. Matches Node's
// behavior of warning on duplicate labels and on missing labels.

use std::cell::RefCell;
use std::collections::HashMap;
use std::time::Instant;

thread_local! {
    static CONSOLE_TIMERS: RefCell<HashMap<String, Instant>> = RefCell::new(HashMap::new());
    static CONSOLE_COUNTERS: RefCell<HashMap<String, u64>> = RefCell::new(HashMap::new());
}

unsafe fn label_from_str_ptr(ptr: *const StringHeader) -> String {
    if ptr.is_null() || (ptr as usize) < 0x1000 {
        return "default".to_string();
    }
    let len = (*ptr).byte_len as usize;
    let data = (ptr as *const u8).add(std::mem::size_of::<StringHeader>());
    let bytes = std::slice::from_raw_parts(data, len);
    std::str::from_utf8(bytes).unwrap_or("default").to_string()
}

fn format_elapsed(dur: std::time::Duration) -> String {
    let ms = dur.as_secs_f64() * 1000.0;
    if ms < 1.0 {
        format!("{:.3}ms", ms)
    } else if ms < 1000.0 {
        format!("{:.3}ms", ms)
    } else {
        format!("{:.3}s", dur.as_secs_f64())
    }
}

#[no_mangle]
pub extern "C" fn js_console_time(label_ptr: *const StringHeader) {
    // Capture wall-clock start before any string decoding or TLS overhead
    // so the stored Instant reflects the call site, not the bookkeeping cost.
    let start = Instant::now();
    let label = unsafe { label_from_str_ptr(label_ptr) };
    CONSOLE_TIMERS.with(|t| {
        let mut map = t.borrow_mut();
        if map.contains_key(&label) {
            eprintln!("Warning: Label '{}' already exists for console.time()", label);
        }
        map.insert(label, start);
    });
}

#[no_mangle]
pub extern "C" fn js_console_time_end(label_ptr: *const StringHeader) {
    let label = unsafe { label_from_str_ptr(label_ptr) };
    CONSOLE_TIMERS.with(|t| {
        let mut map = t.borrow_mut();
        match map.remove(&label) {
            Some(start) => println!("{}: {}", label, format_elapsed(start.elapsed())),
            None => eprintln!("Warning: No such label '{}' for console.timeEnd()", label),
        }
    });
}

#[no_mangle]
pub extern "C" fn js_console_time_log(label_ptr: *const StringHeader) {
    let label = unsafe { label_from_str_ptr(label_ptr) };
    CONSOLE_TIMERS.with(|t| {
        let map = t.borrow();
        match map.get(&label) {
            Some(start) => println!("{}: {}", label, format_elapsed(start.elapsed())),
            None => eprintln!("Warning: No such label '{}' for console.timeLog()", label),
        }
    });
}

// === console.count / countReset ===

#[no_mangle]
pub extern "C" fn js_console_count(label_ptr: *const StringHeader) {
    let label = unsafe { label_from_str_ptr(label_ptr) };
    CONSOLE_COUNTERS.with(|c| {
        let mut map = c.borrow_mut();
        let entry = map.entry(label.clone()).or_insert(0);
        *entry += 1;
        println!("{}: {}", label, *entry);
    });
}

#[no_mangle]
pub extern "C" fn js_console_count_reset(label_ptr: *const StringHeader) {
    let label = unsafe { label_from_str_ptr(label_ptr) };
    CONSOLE_COUNTERS.with(|c| {
        let mut map = c.borrow_mut();
        if map.remove(&label).is_none() {
            eprintln!("Warning: Count for '{}' does not exist", label);
        }
    });
}

// === console.group / groupEnd / groupCollapsed ===
//
// Just print the label like console.log; we don't track indent yet.

// Thread-local indent level for console.group. Each call to
// console.group() increments, each groupEnd() decrements. The
// common console.log path prefixes output with `"  ".repeat(level)`
// when level > 0 to match Node's visual indentation.
thread_local! {
    pub(crate) static CONSOLE_GROUP_INDENT: std::cell::Cell<usize> = std::cell::Cell::new(0);
}

/// Return the current indent prefix (two spaces per level).
pub(crate) fn console_group_prefix() -> String {
    CONSOLE_GROUP_INDENT.with(|l| "  ".repeat(l.get()))
}

#[no_mangle]
pub extern "C" fn js_console_group(label_ptr: *const StringHeader) {
    let label = unsafe { label_from_str_ptr(label_ptr) };
    println!("{}{}", console_group_prefix(), label);
    CONSOLE_GROUP_INDENT.with(|l| l.set(l.get() + 1));
}

/// Called after the label is printed via the common console.log
/// path; just bumps the indent level.
#[no_mangle]
pub extern "C" fn js_console_group_begin() {
    CONSOLE_GROUP_INDENT.with(|l| l.set(l.get() + 1));
}

#[no_mangle]
pub extern "C" fn js_console_group_end() {
    CONSOLE_GROUP_INDENT.with(|l| {
        let cur = l.get();
        if cur > 0 {
            l.set(cur - 1);
        }
    });
}

// === console.assert ===
//
// Prints "Assertion failed" + the message args when the condition is false.

#[no_mangle]
pub extern "C" fn js_console_assert(cond: f64, msg_ptr: *const StringHeader) {
    use crate::value::js_is_truthy;
    if js_is_truthy(cond) != 0 { return; }
    let msg = unsafe {
        if msg_ptr.is_null() || (msg_ptr as usize) < 0x1000 {
            String::new()
        } else {
            let len = (*msg_ptr).byte_len as usize;
            let data = (msg_ptr as *const u8).add(std::mem::size_of::<StringHeader>());
            let bytes = std::slice::from_raw_parts(data, len);
            std::str::from_utf8(bytes).unwrap_or("").to_string()
        }
    };
    if msg.is_empty() {
        eprintln!("Assertion failed");
    } else {
        eprintln!("Assertion failed: {}", msg);
    }
}

/// `console.assert(cond, ...messages)` — multi-arg form. The codegen
/// bundles all the message args (everything after the cond) into a
/// heap array and passes the raw array pointer here. We format the
/// messages by calling `format_jsvalue` on each element and joining
/// with spaces, mirroring Node's `util.format` behavior for simple
/// inputs (numbers, strings, objects).
#[no_mangle]
pub extern "C" fn js_console_assert_spread(cond: f64, args_arr_handle: i64) {
    use crate::value::js_is_truthy;
    if js_is_truthy(cond) != 0 { return; }

    let arr_ptr = (args_arr_handle & 0x0000_FFFF_FFFF_FFFF) as *const crate::array::ArrayHeader;
    if arr_ptr.is_null() {
        eprintln!("Assertion failed");
        return;
    }
    unsafe {
        let len = (*arr_ptr).length as usize;
        if len == 0 {
            eprintln!("Assertion failed");
            return;
        }
        let elements = (arr_ptr as *const u8)
            .add(std::mem::size_of::<crate::array::ArrayHeader>()) as *const f64;
        let mut parts: Vec<String> = Vec::with_capacity(len);
        for i in 0..len {
            let v = *elements.add(i);
            parts.push(crate::builtins::format_jsvalue(v, 0));
        }
        eprintln!("Assertion failed: {}", parts.join(" "));
    }
}

// === console.trace ===
//
// Node writes `Trace: <msg>` + a JS stack trace to **stderr**. Perry can't
// reproduce Node's TS source positions without a source-map / DWARF pass,
// but `std::backtrace::Backtrace::force_capture()` gives us the native
// call stack for free — good enough to see *where* the trace was called
// from, which is what issue #20 is actually asking for.
#[no_mangle]
pub extern "C" fn js_console_trace(value: f64) {
    let jsval = JSValue::from_bits(value.to_bits());
    if jsval.is_undefined() {
        eprintln!("Trace");
    } else if jsval.is_string() {
        let ptr = jsval.as_string_ptr();
        if ptr.is_null() {
            eprintln!("Trace");
        } else {
            unsafe {
                let len = (*ptr).byte_len as usize;
                let data = (ptr as *const u8).add(std::mem::size_of::<StringHeader>());
                let bytes = std::slice::from_raw_parts(data, len);
                match std::str::from_utf8(bytes) {
                    Ok(s) => eprintln!("Trace: {}", s),
                    Err(_) => eprintln!("Trace: [invalid utf8]"),
                }
            }
        }
    } else {
        eprintln!("Trace: {}", format_jsvalue(value, 0));
    }
    let bt = std::backtrace::Backtrace::force_capture();
    let rendered = format!("{}", bt);
    // Parse the Display output into (header, continuation*) frames. The
    // header looks like "   N: symbol" and each continuation starts with
    // "at …". Drop frames whose header matches internal noise (the
    // std::backtrace plumbing itself, plus `js_console_trace` — the user
    // already sees `Trace:` above). Collapse consecutive identical headers
    // (what you get on stripped builds, where every frame symbolicates to
    // `__mh_execute_header`).
    let noise = ["backtrace", "Backtrace::", "js_console_trace"];
    let is_header = |t: &str| {
        t.chars().next().is_some_and(|c| c.is_ascii_digit()) && t.contains(':')
    };
    let mut frames: Vec<(String, Vec<String>)> = Vec::new();
    for line in rendered.lines() {
        let t = line.trim_start();
        if t.is_empty() || t.starts_with("note:") {
            continue;
        }
        if is_header(t) {
            let sym = t.split_once(':').map(|(_, r)| r.trim()).unwrap_or(t);
            frames.push((sym.to_string(), Vec::new()));
        } else if let Some(last) = frames.last_mut() {
            last.1.push(t.to_string());
        }
    }
    let mut emitted = 0usize;
    let mut prev_sym: Option<String> = None;
    let mut dup_run = 0usize;
    for (sym, cont) in frames {
        if noise.iter().any(|p| sym.contains(p)) {
            continue;
        }
        if prev_sym.as_deref() == Some(sym.as_str()) {
            dup_run += 1;
            continue;
        }
        if dup_run > 0 {
            eprintln!("        (… {} more identical frames)", dup_run);
            dup_run = 0;
        }
        eprintln!("    {}: {}", emitted, sym);
        for c in cont {
            eprintln!("             {}", c);
        }
        emitted += 1;
        prev_sym = Some(sym);
    }
    if dup_run > 0 {
        eprintln!("        (… {} more identical frames)", dup_run);
    }
}

// === console.clear ===
//
// Best-effort: emit ANSI clear sequence on stdout — but ONLY when stdout
// is an actual TTY. When stdout is piped or redirected to a file, Node
// makes `console.clear()` a no-op (no escape sequence written), so emitting
// it unconditionally would diff against Node by injecting `\x1b[2J\x1b[H`
// into captured output.

#[no_mangle]
pub extern "C" fn js_console_clear() {
    use std::io::IsTerminal as _;
    if std::io::stdout().is_terminal() {
        print!("\x1b[2J\x1b[H");
    }
}

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
        } else if jsval.is_string() {
            let ptr = jsval.as_string_ptr();
            if ptr.is_null() {
                "''".to_string()
            } else {
                let len = (*ptr).byte_len as usize;
                let data = (ptr as *const u8).add(std::mem::size_of::<StringHeader>());
                let bytes = std::slice::from_raw_parts(data, len);
                let s = std::str::from_utf8(bytes).unwrap_or("[invalid utf8]");
                format!("'{}'", s)
            }
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
                if n > 0.0 { "Infinity".to_string() } else { "-Infinity".to_string() }
            } else if is_negative_zero(n) {
                "-0".to_string()
            } else if n.fract() == 0.0 && n.abs() < (i64::MAX as f64) {
                (n as i64).to_string()
            } else {
                format_finite_number_js(n)
            }
        }
    }
}

/// Read a string out of a NaN-boxed string JSValue.
unsafe fn read_string_from_jsvalue(jsval: JSValue) -> Option<String> {
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
    Some(std::str::from_utf8(bytes).unwrap_or("[invalid utf8]").to_string())
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
fn render_table(headers: &[String], rows: &[Vec<String>]) {
    let num_cols = headers.len();
    if num_cols == 0 {
        return;
    }

    // Compute column widths: max(header.chars().count(), max(row[i].chars().count()))
    let mut widths: Vec<usize> = headers.iter().map(|h| h.chars().count()).collect();
    for row in rows {
        for (i, cell) in row.iter().enumerate() {
            if i < widths.len() {
                let w = cell.chars().count();
                if w > widths[i] {
                    widths[i] = w;
                }
            }
        }
    }

    // Helpers
    let dashes = |w: usize| -> String { "─".repeat(w + 2) };
    let pad_cell = |s: &str, w: usize| -> String {
        let count = s.chars().count();
        let pad = if w > count { w - count } else { 0 };
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
    unsafe {
        let jsval = JSValue::from_bits(value.to_bits());
        if !jsval.is_pointer() {
            // Primitives just print via the dynamic logger.
            js_console_log_dynamic(value);
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
            let data_ptr = (arr_ptr as *const u8).add(std::mem::size_of::<crate::array::ArrayHeader>()) as *const f64;

            if length == 0 {
                // Node prints "(no values)" but for our purposes a minimal table is fine.
                // Match Node by printing nothing (it actually prints an empty box, but that's fine).
                return;
            }

            // Decide: array of objects vs array of arrays vs array of primitives.
            // Look at the first element.
            let first = *data_ptr;
            let first_gc = get_gc_type(first);

            if first_gc == crate::gc::GC_TYPE_OBJECT {
                // Array of objects: union all keys.
                let mut all_keys: Vec<String> = Vec::new();
                let mut row_keys: Vec<Vec<String>> = Vec::with_capacity(length);
                for i in 0..length {
                    let elem = *data_ptr.add(i);
                    let elem_jsval = JSValue::from_bits(elem.to_bits());
                    if get_gc_type(elem) == crate::gc::GC_TYPE_OBJECT {
                        let obj_ptr = elem_jsval.as_pointer::<crate::object::ObjectHeader>();
                        let keys = object_key_names(obj_ptr);
                        for k in &keys {
                            if !all_keys.contains(k) {
                                all_keys.push(k.clone());
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
                            // Build a temporary StringHeader for the lookup
                            let key_ptr = build_temp_string_header(key);
                            let v = crate::object::js_object_get_field_by_name_f64(obj_ptr, key_ptr);
                            free_temp_string_header(key_ptr);
                            // If undefined, leave cell empty
                            let v_jsval = JSValue::from_bits(v.to_bits());
                            if v_jsval.is_undefined() {
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
            } else if first_gc == crate::gc::GC_TYPE_ARRAY {
                // Array of arrays: columns are 0..max_len.
                let mut max_len = 0usize;
                let mut sub_lens: Vec<usize> = Vec::with_capacity(length);
                for i in 0..length {
                    let elem = *data_ptr.add(i);
                    let elem_jsval = JSValue::from_bits(elem.to_bits());
                    if get_gc_type(elem) == crate::gc::GC_TYPE_ARRAY {
                        let sub = elem_jsval.as_pointer::<crate::array::ArrayHeader>();
                        let l = (*sub).length as usize;
                        sub_lens.push(l);
                        if l > max_len { max_len = l; }
                    } else {
                        sub_lens.push(0);
                    }
                }

                let mut headers: Vec<String> = Vec::with_capacity(1 + max_len);
                headers.push("(index)".to_string());
                for j in 0..max_len {
                    headers.push(j.to_string());
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
                        let sub_data = (sub as *const u8).add(std::mem::size_of::<crate::array::ArrayHeader>()) as *const f64;
                        for j in 0..max_len {
                            if j < sub_len {
                                let v = *sub_data.add(j);
                                row.push(format_table_cell(v));
                            } else {
                                row.push("".to_string());
                            }
                        }
                    } else {
                        for _ in 0..max_len {
                            row.push("".to_string());
                        }
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
            let headers = vec!["(index)".to_string(), "Values".to_string()];
            let mut rows: Vec<Vec<String>> = Vec::with_capacity(keys.len());
            for (i, key) in keys.iter().enumerate() {
                // Read the value by field index (matches keys_array order).
                let v = crate::object::js_object_get_field_f64(obj_ptr, i as u32);
                rows.push(vec![key.clone(), format_table_cell(v)]);
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

// ============================================================
// TextEncoder / TextDecoder
// ============================================================

/// TextEncoder.encode(string) -> Buffer (Uint8Array of UTF-8 bytes)
/// Takes a NaN-boxed string value and returns a raw buffer pointer.
#[no_mangle]
pub extern "C" fn js_text_encoder_encode(value: f64) -> i64 {
    use crate::buffer::js_buffer_from_string;
    let str_ptr = crate::value::js_get_string_pointer_unified(value);
    let buf = js_buffer_from_string(str_ptr as *const StringHeader, 0); // 0 = UTF-8
    buf as i64
}

/// TextDecoder.decode(buffer_ptr) -> string pointer (i64)
/// Takes a raw buffer/Uint8Array pointer (i64) and returns a StringHeader pointer.
#[no_mangle]
pub extern "C" fn js_text_decoder_decode(buf_ptr: i64) -> i64 {
    use crate::buffer::{BufferHeader, js_buffer_to_string};
    if buf_ptr == 0 || (buf_ptr as usize) < 0x1000 {
        return js_string_from_bytes(std::ptr::null(), 0) as i64;
    }
    let ptr = buf_ptr as *const BufferHeader;
    let str_ptr = js_buffer_to_string(ptr, 0); // 0 = UTF-8
    str_ptr as i64
}

// ============================================================
// encodeURI / decodeURI / encodeURIComponent / decodeURIComponent
// ============================================================

/// Characters that encodeURI does NOT encode (RFC 2396 unreserved + reserved)
const URI_UNESCAPED: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_.!~*'()";
const URI_RESERVED: &[u8] = b";/?:@&=+$,#";

/// Characters that encodeURIComponent does NOT encode (RFC 2396 unreserved only)
const URI_COMPONENT_UNESCAPED: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_.!~*'()";

fn percent_encode(input: &str, safe_chars: &[u8]) -> String {
    let mut result = String::with_capacity(input.len() * 3);
    for byte in input.as_bytes() {
        if safe_chars.contains(byte) {
            result.push(*byte as char);
        } else {
            result.push('%');
            result.push_str(&format!("{:02X}", byte));
        }
    }
    result
}

fn percent_decode(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut result = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            let hi = hex_digit(bytes[i + 1]);
            let lo = hex_digit(bytes[i + 2]);
            if let (Some(h), Some(l)) = (hi, lo) {
                result.push(h * 16 + l);
                i += 3;
                continue;
            }
        }
        result.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&result).into_owned()
}

fn hex_digit(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

fn extract_str_from_nanbox(value: f64) -> String {
    let str_ptr = crate::value::js_get_string_pointer_unified(value);
    if (str_ptr as usize) < 0x1000 {
        return String::new();
    }
    unsafe {
        let header = str_ptr as *const StringHeader;
        let len = (*header).byte_len as usize;
        let data = (header as *const u8).add(std::mem::size_of::<StringHeader>());
        let bytes = std::slice::from_raw_parts(data, len);
        std::str::from_utf8(bytes).unwrap_or("").to_string()
    }
}

/// encodeURI(string) -> string
#[no_mangle]
pub extern "C" fn js_encode_uri(value: f64) -> i64 {
    let input = extract_str_from_nanbox(value);
    let mut safe = Vec::with_capacity(URI_UNESCAPED.len() + URI_RESERVED.len());
    safe.extend_from_slice(URI_UNESCAPED);
    safe.extend_from_slice(URI_RESERVED);
    let encoded = percent_encode(&input, &safe);
    let ptr = js_string_from_bytes(encoded.as_ptr(), encoded.len() as u32);
    ptr as i64
}

/// decodeURI(string) -> string
#[no_mangle]
pub extern "C" fn js_decode_uri(value: f64) -> i64 {
    let input = extract_str_from_nanbox(value);
    let decoded = percent_decode(&input);
    let ptr = js_string_from_bytes(decoded.as_ptr(), decoded.len() as u32);
    ptr as i64
}

/// encodeURIComponent(string) -> string
#[no_mangle]
pub extern "C" fn js_encode_uri_component(value: f64) -> i64 {
    let input = extract_str_from_nanbox(value);
    let encoded = percent_encode(&input, URI_COMPONENT_UNESCAPED);
    let ptr = js_string_from_bytes(encoded.as_ptr(), encoded.len() as u32);
    ptr as i64
}

/// decodeURIComponent(string) -> string
#[no_mangle]
pub extern "C" fn js_decode_uri_component(value: f64) -> i64 {
    let input = extract_str_from_nanbox(value);
    let decoded = percent_decode(&input);
    let ptr = js_string_from_bytes(decoded.as_ptr(), decoded.len() as u32);
    ptr as i64
}

// ============================================================
// structuredClone
// ============================================================

/// structuredClone(value) -> deep-cloned value
/// Handles numbers (pass-through), strings (copy), arrays/objects (shallow for now)
#[no_mangle]
pub extern "C" fn js_structured_clone(value: f64) -> f64 {
    let bits = value.to_bits();
    // Pass through primitives (undefined, null, true, false)
    if bits == 0x7FFC_0000_0000_0001 || bits == 0x7FFC_0000_0000_0002
        || bits == 0x7FFC_0000_0000_0003 || bits == 0x7FFC_0000_0000_0004 {
        return value;
    }
    // Regular f64 numbers pass through
    let tag = (bits >> 48) as u16;
    if tag < 0x7FF8 {
        return value;
    }

    match tag {
        0x7FFF => {
            // STRING_TAG — copy the string
            let str_ptr = (bits & 0x0000_FFFF_FFFF_FFFF) as *const StringHeader;
            if (str_ptr as usize) < 0x1000 {
                return value;
            }
            unsafe {
                let len = (*str_ptr).byte_len as usize;
                let data = (str_ptr as *const u8).add(std::mem::size_of::<StringHeader>());
                let new_str = js_string_from_bytes(data, len as u32);
                let new_bits = 0x7FFF_0000_0000_0000u64 | (new_str as u64 & 0x0000_FFFF_FFFF_FFFF);
                f64::from_bits(new_bits)
            }
        }
        0x7FFE => {
            // INT32_TAG — pass through
            value
        }
        0x7FFD => {
            // POINTER_TAG — could be array/object/Map/Set/RegExp. Deep clone recursively.
            let ptr = (bits & 0x0000_FFFF_FFFF_FFFF) as *const u8;
            if (ptr as usize) < 0x10000 { return value; }
            // Set is tracked in SET_REGISTRY (not GC_TYPE_SET since it has
            // no GC header). Check the registry BEFORE touching the GC
            // header bytes — they'd be garbage for raw-allocated sets.
            if crate::set::is_registered_set(ptr as usize) {
                unsafe {
                    let src = ptr as *const crate::set::SetHeader;
                    let size = crate::set::js_set_size(src);
                    let new_set = crate::set::js_set_alloc(size.max(8));
                    let arr = crate::set::js_set_to_array(src);
                    let len = (*arr).length as usize;
                    let data = (arr as *const u8)
                        .add(std::mem::size_of::<crate::array::ArrayHeader>())
                        as *const f64;
                    for i in 0..len {
                        let v = js_structured_clone(*data.add(i));
                        crate::set::js_set_add(new_set, v);
                    }
                    let new_bits = 0x7FFD_0000_0000_0000u64
                        | (new_set as u64 & 0x0000_FFFF_FFFF_FFFF);
                    return f64::from_bits(new_bits);
                }
            }
            unsafe {
                // GcHeader is stored BEFORE the user pointer (at ptr - GC_HEADER_SIZE)
                let gc_header_ptr = (ptr as *const u8).sub(crate::gc::GC_HEADER_SIZE);
                let gc_type = *gc_header_ptr;
                if gc_type == crate::gc::GC_TYPE_ARRAY {
                    // Clone array using existing clone, then recursively clone elements
                    let arr = ptr as *const crate::array::ArrayHeader;
                    let new_arr = crate::array::js_array_clone(arr);
                    let len = (*new_arr).length;
                    let elements = (new_arr as *mut u8).add(std::mem::size_of::<crate::array::ArrayHeader>()) as *mut f64;
                    for i in 0..len as usize {
                        let elem = *elements.add(i);
                        *elements.add(i) = js_structured_clone(elem);
                    }
                    let new_bits = 0x7FFD_0000_0000_0000u64 | (new_arr as u64 & 0x0000_FFFF_FFFF_FFFF);
                    f64::from_bits(new_bits)
                } else if gc_type == crate::gc::GC_TYPE_OBJECT {
                    // Check if this is a RegExp (the RegExpHeader lives in an
                    // arena slot with GC_TYPE_OBJECT but tracked in
                    // REGEX_POINTERS). Clone by reading source/flags and
                    // building a fresh one via js_regexp_new.
                    if crate::regex::is_regex_pointer(ptr as *const u8) {
                        let re_ptr = ptr as *const crate::regex::RegExpHeader;
                        let src = crate::regex::js_regexp_get_source(re_ptr);
                        let flg = crate::regex::js_regexp_get_flags(re_ptr);
                        let new_re = crate::regex::js_regexp_new(src, flg);
                        let new_bits = 0x7FFD_0000_0000_0000u64
                            | (new_re as u64 & 0x0000_FFFF_FFFF_FFFF);
                        return f64::from_bits(new_bits);
                    }
                    // Clone object using clone_with_extra (0 extra fields, no static keys)
                    let cloned_obj = crate::object::js_object_clone_with_extra(value, 0, std::ptr::null(), 0);
                    if !cloned_obj.is_null() && (cloned_obj as usize) > 0x10000 {
                        let field_count = (*cloned_obj).field_count;
                        let fields = (cloned_obj as *mut u8).add(std::mem::size_of::<crate::object::ObjectHeader>()) as *mut f64;
                        for i in 0..field_count as usize {
                            let field = *fields.add(i);
                            *fields.add(i) = js_structured_clone(field);
                        }
                    }
                    // NaN-box with POINTER_TAG
                    let new_bits = 0x7FFD_0000_0000_0000u64 | (cloned_obj as u64 & 0x0000_FFFF_FFFF_FFFF);
                    f64::from_bits(new_bits)
                } else if gc_type == crate::gc::GC_TYPE_MAP {
                    // Deep-clone a Map by building a fresh one and copying
                    // entries through js_map_set (which handles the hash
                    // bucket + entries array layout).
                    let map = ptr as *const crate::map::MapHeader;
                    let size = crate::map::js_map_size(map);
                    let new_map = crate::map::js_map_alloc(size.max(8));
                    // Walk entries via js_map_entries which returns an
                    // Array<[key, value]> pair array.
                    let entries_arr = crate::map::js_map_entries(map);
                    let entries_len = (*entries_arr).length as usize;
                    let entries_data = (entries_arr as *const u8)
                        .add(std::mem::size_of::<crate::array::ArrayHeader>())
                        as *const f64;
                    for i in 0..entries_len {
                        let pair_box = *entries_data.add(i);
                        let pair_bits = pair_box.to_bits();
                        let pair_ptr = (pair_bits & 0x0000_FFFF_FFFF_FFFF)
                            as *const crate::array::ArrayHeader;
                        if pair_ptr.is_null() {
                            continue;
                        }
                        let pair_data = (pair_ptr as *const u8)
                            .add(std::mem::size_of::<crate::array::ArrayHeader>())
                            as *const f64;
                        let k = js_structured_clone(*pair_data);
                        let v = js_structured_clone(*pair_data.add(1));
                        crate::map::js_map_set(new_map, k, v);
                    }
                    let new_bits = 0x7FFD_0000_0000_0000u64
                        | (new_map as u64 & 0x0000_FFFF_FFFF_FFFF);
                    f64::from_bits(new_bits)
                } else {
                    // Unknown pointer type — pass through
                    value
                }
            }
        }
        _ => value,
    }
}

// ============================================================
// queueMicrotask
// ============================================================

/// queueMicrotask(callback) — schedule a closure on the microtask queue.
/// The closure runs during the next `js_promise_run_microtasks()` drain,
/// AFTER the current synchronous code completes. Previously this called
/// the closure immediately, which broke the JS spec ordering:
///   queueMicrotask(() => log("micro"));
///   log("sync");
/// should print "sync" then "micro", not "micro" then "sync".
#[no_mangle]
pub extern "C" fn js_queue_microtask(callback: i64) {
    QUEUED_MICROTASKS.with(|q| {
        q.borrow_mut().push(callback);
    });
}

thread_local! {
    static QUEUED_MICROTASKS: std::cell::RefCell<Vec<i64>> = std::cell::RefCell::new(Vec::new());
}

/// Drain queued microtasks. Called by `js_promise_run_microtasks`.
#[no_mangle]
pub extern "C" fn js_drain_queued_microtasks() {
    use crate::closure::js_closure_call0;
    loop {
        let task = QUEUED_MICROTASKS.with(|q| {
            let mut queue = q.borrow_mut();
            if queue.is_empty() { None } else { Some(queue.remove(0)) }
        });
        match task {
            Some(cb) => unsafe {
                js_closure_call0(cb as *const crate::closure::ClosureHeader);
            },
            None => break,
        }
    }
}
