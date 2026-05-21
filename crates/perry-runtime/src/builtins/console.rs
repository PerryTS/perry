//! `console.*` runtime entry points.
//!
//! Split out of the original monolithic `builtins.rs` (#topic: split-large-files).
//! Covers every `js_console_*` FFI symbol — log/error/warn variants,
//! group/groupEnd, assert, trace, clear, dir, count, time/timeEnd/timeLog —
//! along with the lazy `console.log`-as-closure singleton and the shared
//! `console.group` indent prefix helper.

#[cfg(feature = "ohos-napi")]
use super::println;
use super::*;

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
        if raw_bits > 0x1000
            && (raw_bits >> 48) == 0
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
            if n > 0.0 {
                println!("{}Infinity", p);
            } else {
                println!("{}-Infinity", p);
            }
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
        let fresh = crate::closure::js_closure_alloc(console_log_callable_thunk as *const u8, 0);
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
    let mut visitor = crate::gc::RuntimeRootVisitor::for_copy(mark);
    scan_console_log_singleton_roots_mut(&mut visitor);
}

pub fn scan_console_log_singleton_roots_mut(visitor: &mut crate::gc::RuntimeRootVisitor<'_>) {
    visitor.visit_atomic_i64_slot(&CONSOLE_LOG_SINGLETON, Ordering::Acquire, Ordering::Release);
}

/// Print a number to stdout (optimized path for known numbers)
#[no_mangle]
pub extern "C" fn js_console_log_number(value: f64) {
    if is_negative_zero(value) {
        println!("-0");
    } else if value.is_nan() {
        println!("NaN");
    } else if value.is_infinite() {
        if value > 0.0 {
            println!("Infinity");
        } else {
            println!("-Infinity");
        }
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
            if n > 0.0 {
                eprintln!("Infinity");
            } else {
                eprintln!("-Infinity");
            }
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
            if n > 0.0 {
                eprintln!("Infinity");
            } else {
                eprintln!("-Infinity");
            }
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

#[no_mangle]
pub extern "C" fn js_console_log_spread(arr_ptr: *const crate::array::ArrayHeader) {
    if arr_ptr.is_null() {
        println!();
        return;
    }

    print_console_formatted(arr_ptr, false);
}

/// Print multiple values to stderr (console.error with spread support)
#[no_mangle]
pub extern "C" fn js_console_error_spread(arr_ptr: *const crate::array::ArrayHeader) {
    if arr_ptr.is_null() {
        eprintln!();
        return;
    }

    print_console_formatted(arr_ptr, true);
}

/// Print multiple values to stderr (console.warn with spread support)
#[no_mangle]
pub extern "C" fn js_console_warn_spread(arr_ptr: *const crate::array::ArrayHeader) {
    // console.warn is essentially the same as console.error in Node.js
    js_console_error_spread(arr_ptr);
}

fn print_console_formatted(arr_ptr: *const crate::array::ArrayHeader, stderr: bool) {
    let formatted = js_util_format(arr_ptr);
    let text = jsvalue_string_content(formatted).unwrap_or_default();
    print_console_text(&text, stderr);
}

fn print_console_text(text: &str, stderr: bool) {
    let prefix = console_group_prefix();
    let mut out = String::new();
    if prefix.is_empty() {
        out.push_str(&text);
    } else {
        for (i, line) in text.split('\n').enumerate() {
            if i > 0 {
                out.push('\n');
            }
            out.push_str(&prefix);
            out.push_str(line);
        }
    }
    if stderr {
        eprintln!("{}", out);
    } else {
        println!("{}", out);
    }
}

#[no_mangle]
pub extern "C" fn js_console_noop() {}

/// Debug trace for module initialization order.
/// Called before each _perry_init_* call to identify which module crashes.
/// No-op in release builds; re-enable eprintln for debugging.
#[no_mangle]
pub extern "C" fn perry_debug_trace_init(_index: i64, _name_ptr: *const u8, _name_len: i64) {}

#[no_mangle]
pub extern "C" fn perry_debug_trace_init_done(_index: i64) {}

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
            eprintln!(
                "Warning: Label '{}' already exists for console.time()",
                label
            );
        }
        map.insert(label, start);
    });
}

fn console_type_error_for_symbol_label() -> ! {
    let msg = "Cannot convert a Symbol value to a string";
    let msg_str = crate::string::js_string_from_bytes(msg.as_ptr(), msg.len() as u32);
    let err_ptr = crate::error::js_typeerror_new(msg_str);
    let err_value = crate::value::JSValue::pointer(err_ptr as *const u8).bits();
    crate::exception::js_throw(f64::from_bits(err_value))
}

fn console_label_from_value(value: f64) -> *const StringHeader {
    let jsval = JSValue::from_bits(value.to_bits());
    if jsval.is_undefined() {
        let s = "default";
        return crate::string::js_string_from_bytes(s.as_ptr(), s.len() as u32);
    }
    if jsval.is_pointer() {
        let ptr = jsval.as_pointer::<u8>() as usize;
        if crate::symbol::is_registered_symbol(ptr) {
            console_type_error_for_symbol_label();
        }
    }
    js_string_coerce(value) as *const StringHeader
}

#[no_mangle]
pub extern "C" fn js_console_time_value(label_value: f64) {
    js_console_time(console_label_from_value(label_value));
}

#[no_mangle]
pub extern "C" fn js_console_time_end_value(label_value: f64) {
    js_console_time_end(console_label_from_value(label_value));
}

#[no_mangle]
pub extern "C" fn js_console_time_log_value(label_value: f64) {
    js_console_time_log(console_label_from_value(label_value));
}

#[no_mangle]
pub extern "C" fn js_console_count_value(label_value: f64) {
    js_console_count(console_label_from_value(label_value));
}

#[no_mangle]
pub extern "C" fn js_console_count_reset_value(label_value: f64) {
    js_console_count_reset(console_label_from_value(label_value));
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

#[no_mangle]
pub extern "C" fn js_console_time_log_spread(
    label_value: f64,
    args_arr: *const crate::array::ArrayHeader,
) {
    let label_ptr = console_label_from_value(label_value);
    let label = unsafe { label_from_str_ptr(label_ptr) };
    CONSOLE_TIMERS.with(|t| {
        let map = t.borrow();
        match map.get(&label) {
            Some(start) => {
                let mut line = format!("{}: {}", label, format_elapsed(start.elapsed()));
                if !args_arr.is_null() {
                    let formatted = js_util_format(args_arr);
                    let extra = jsvalue_string_content(formatted).unwrap_or_default();
                    if !extra.is_empty() {
                        line.push(' ');
                        line.push_str(&extra);
                    }
                }
                println!("{}", line);
            }
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
    if label == "NaN" {
        eprintln!("Warning: Count for '{}' does not exist", label);
        return;
    }
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
    pub(crate) static CONSOLE_GROUP_INDENT: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
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
    if js_is_truthy(cond) != 0 {
        return;
    }
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
    if js_is_truthy(cond) != 0 {
        return;
    }

    let arr_ptr = (args_arr_handle & 0x0000_FFFF_FFFF_FFFF) as *const crate::array::ArrayHeader;
    if arr_ptr.is_null() {
        eprintln!("Assertion failed");
        return;
    }
    unsafe {
        if (*arr_ptr).length == 0 {
            eprintln!("Assertion failed");
            return;
        }
        let elements = (arr_ptr as *const u8).add(std::mem::size_of::<crate::array::ArrayHeader>())
            as *const f64;
        let first = JSValue::from_bits((*elements).to_bits());
        let formatted = js_util_format(arr_ptr);
        let msg = jsvalue_string_content(formatted).unwrap_or_default();
        if msg.is_empty() {
            eprintln!("Assertion failed");
        } else if first.is_any_string() {
            eprintln!("Assertion failed: {}", msg);
        } else {
            eprintln!("Assertion failed {}", msg);
        }
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
    let is_header =
        |t: &str| t.chars().next().is_some_and(|c| c.is_ascii_digit()) && t.contains(':');
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

#[no_mangle]
pub extern "C" fn js_console_trace_spread(arr_ptr: *const crate::array::ArrayHeader) {
    if arr_ptr.is_null() {
        js_console_trace(f64::from_bits(JSValue::undefined().bits()));
        return;
    }
    let formatted = js_util_format(arr_ptr);
    let text = jsvalue_string_content(formatted).unwrap_or_default();
    if text.is_empty() {
        eprintln!("Trace");
    } else {
        eprintln!("Trace: {}", text);
    }
    emit_console_trace_stack();
}

fn emit_console_trace_stack() {
    let bt = std::backtrace::Backtrace::force_capture();
    let rendered = format!("{}", bt);
    let noise = [
        "backtrace",
        "Backtrace::",
        "js_console_trace",
        "js_console_trace_spread",
        "emit_console_trace_stack",
    ];
    let is_header =
        |t: &str| t.chars().next().is_some_and(|c| c.is_ascii_digit()) && t.contains(':');
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
        for c in cont.into_iter().take(2) {
            eprintln!("        {}", c);
        }
        emitted += 1;
        prev_sym = Some(sym);
        if emitted >= 8 {
            break;
        }
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

/// Decode `options.depth` from a NaN-boxed `console.dir(value, options)`
/// second arg. Mirrors Node:
///   - missing key / non-object options → return `None` (caller defaults to 2)
///   - `null` → `Some(usize::MAX)` (unlimited)
///   - non-negative integer → that many levels of nesting
///   - negative or non-finite → clamp to 0 (matches Node's coerce-to-zero)
///
/// # Safety
///
/// `options_value` must be a valid NaN-boxed JSValue.
unsafe fn decode_dir_depth_option(options_value: f64) -> Option<usize> {
    let jsval = JSValue::from_bits(options_value.to_bits());
    if !jsval.is_pointer() {
        return None;
    }
    let ptr: *const crate::array::ArrayHeader = jsval.as_pointer();
    if ptr.is_null() || (ptr as usize) < 0x10000 || ((ptr as u64) >> 48) != 0 {
        return None;
    }
    // Confirm this is a regular object before dereferencing as ObjectHeader.
    let gc_header = (ptr as *const u8).sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader;
    if (*gc_header).obj_type != crate::gc::GC_TYPE_OBJECT {
        return None;
    }
    let obj_ptr = ptr as *const crate::object::ObjectHeader;
    let keys_array = (*obj_ptr).keys_array;
    if keys_array.is_null() {
        return None;
    }
    let key_count = crate::array::js_array_length(keys_array) as usize;
    for i in 0..key_count {
        let key_val = crate::array::js_array_get(keys_array, i as u32);
        if !key_val.is_string() {
            continue;
        }
        let key_ptr = key_val.as_string_ptr();
        if key_ptr.is_null() {
            continue;
        }
        let key_len = (*key_ptr).byte_len as usize;
        let key_data = (key_ptr as *const u8).add(std::mem::size_of::<StringHeader>());
        let key_bytes = std::slice::from_raw_parts(key_data, key_len);
        if key_bytes != b"depth" {
            continue;
        }
        let raw = crate::object::js_object_get_field_f64(obj_ptr, i as u32);
        let v = JSValue::from_bits(raw.to_bits());
        if v.is_null() {
            return Some(usize::MAX);
        }
        if v.is_int32() {
            let n = v.as_int32();
            return Some(if n < 0 { 0 } else { n as usize });
        }
        if v.is_number() {
            let n = v.as_number();
            if n.is_nan() {
                return Some(0);
            }
            if n.is_infinite() {
                return if n > 0.0 { Some(usize::MAX) } else { Some(0) };
            }
            let n_i = n as i64;
            return Some(if n_i < 0 { 0 } else { n_i as usize });
        }
        return None;
    }
    None
}

/// Decode `options.showHidden` from a NaN-boxed `console.dir` second arg.
/// Returns the bool value when present; `None` when the key is missing
/// or the options arg isn't an object. Node coerces any truthy value to
/// `true`; we accept either explicit `true`/`false` or non-zero numeric
/// values to match.
///
/// # Safety
///
/// `options_value` must be a valid NaN-boxed JSValue.
unsafe fn decode_dir_show_hidden_option(options_value: f64) -> Option<bool> {
    let jsval = JSValue::from_bits(options_value.to_bits());
    if !jsval.is_pointer() {
        return None;
    }
    let ptr: *const crate::array::ArrayHeader = jsval.as_pointer();
    if ptr.is_null() || (ptr as usize) < 0x10000 || ((ptr as u64) >> 48) != 0 {
        return None;
    }
    let gc_header = (ptr as *const u8).sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader;
    if (*gc_header).obj_type != crate::gc::GC_TYPE_OBJECT {
        return None;
    }
    let obj_ptr = ptr as *const crate::object::ObjectHeader;
    let keys_array = (*obj_ptr).keys_array;
    if keys_array.is_null() {
        return None;
    }
    let key_count = crate::array::js_array_length(keys_array) as usize;
    for i in 0..key_count {
        let key_val = crate::array::js_array_get(keys_array, i as u32);
        if !key_val.is_string() {
            continue;
        }
        let key_ptr = key_val.as_string_ptr();
        if key_ptr.is_null() {
            continue;
        }
        let key_len = (*key_ptr).byte_len as usize;
        let key_data = (key_ptr as *const u8).add(std::mem::size_of::<StringHeader>());
        let key_bytes = std::slice::from_raw_parts(key_data, key_len);
        if key_bytes != b"showHidden" {
            continue;
        }
        let raw = crate::object::js_object_get_field_f64(obj_ptr, i as u32);
        let v = JSValue::from_bits(raw.to_bits());
        if v.is_bool() {
            return Some(v.as_bool());
        }
        if v.is_int32() {
            return Some(v.as_int32() != 0);
        }
        if v.is_number() {
            let n = v.as_number();
            return Some(!n.is_nan() && n != 0.0);
        }
        if v.is_null() || v.is_undefined() {
            return Some(false);
        }
        // Any other pointer-shaped value is truthy in Node's coercion.
        return Some(true);
    }
    None
}

/// `console.dir(value, options)` — formats `value` with the same surface used
/// by `console.log`, but honors `options.depth` (Node default: 2; #1199) and
/// `options.showHidden` (default: false; #1200).
///
/// # Safety
///
/// Both args must be valid NaN-boxed JSValues.
#[no_mangle]
pub unsafe extern "C" fn js_console_dir_with_options(value: f64, options_value: f64) {
    let max_depth = decode_dir_depth_option(options_value).unwrap_or(2);
    let show_hidden = decode_dir_show_hidden_option(options_value).unwrap_or(false);
    let _depth_guard = InspectDepthLimitGuard::new(max_depth);
    let _hidden_guard = InspectShowHiddenGuard::new(show_hidden);
    println!("{}", format_jsvalue(value, 0));
}

#[cfg(test)]
pub(crate) fn test_set_console_log_singleton(ptr: i64) {
    CONSOLE_LOG_SINGLETON.store(ptr, Ordering::Release);
}

#[cfg(test)]
pub(crate) fn test_console_log_singleton() -> i64 {
    CONSOLE_LOG_SINGLETON.load(Ordering::Acquire)
}
