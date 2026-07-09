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
        } else if n.fract() == 0.0 && n.abs() < INT_EXACT_FASTPATH_LIMIT {
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
        } else if n.fract() == 0.0 && n.abs() < INT_EXACT_FASTPATH_LIMIT {
            println!("{}{}", p, n as i64);
        } else {
            // Match `js_console_error_dynamic` / `js_console_warn_dynamic`: use the
            // shared JS formatter (shortest round-trip + the 1e21/1e-6 exponential
            // thresholds), not Rust's `f64` Display — which prints `1e21` as
            // `1000000000000000000000` instead of Node's `1e+21` (#6127).
            println!("{}{}", p, format_finite_number_js(n));
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
        // and will be reclaimed by the next GC sweep. The winner is
        // stored in the root slot through the root barrier path.
        match crate::gc::runtime_compare_exchange_root_atomic_raw_i64(
            &CONSOLE_LOG_SINGLETON,
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

/// `console[dynamicKey]` — resolve a console method by a RUNTIME key string,
/// returning the same bound native closure a static `console.<key>` value read
/// produces. Codegen routes the computed-member form here (`console[m](...)`,
/// `const f = console[m]`, `loadEnvConfig(dir, true, console, false)` reaching
/// into a forwarded console). Without it the receiver collapsed to the
/// `GlobalGet(0)` sentinel and the method was read off numeric `0`, so the call
/// threw `(number).log is not a function` (the Next.js `_log.event` wall:
/// `prefixedLog` does `console[consoleMethod](...)`). A non-string key or an
/// unknown method name yields `undefined`, matching a real object miss.
#[no_mangle]
pub extern "C" fn js_console_method_by_value(key: f64) -> f64 {
    // Apply JS property-key coercion before the lookup: `console[0]` → "0",
    // `console[{toString:()=>'log'}]` → "log". A Symbol key coerces to a
    // symbol (not a string method name), so it falls through to `undefined`,
    // matching a real object miss.
    let coerced = unsafe { crate::object::js_to_property_key(key) };
    let name = match jsvalue_string_content(coerced) {
        Some(s) => s,
        None => return f64::from_bits(crate::value::TAG_UNDEFINED),
    };
    unsafe {
        crate::object::js_native_module_property_by_name(
            b"console".as_ptr(),
            "console".len(),
            name.as_ptr(),
            name.len(),
        )
    }
}

/// GC root scanner: pin the lazily-allocated `console.log`-as-closure
/// singleton against the next sweep.
pub fn scan_console_log_singleton_roots(mark: &mut dyn FnMut(f64)) {
    let mut visitor = crate::gc::RuntimeRootVisitor::for_copy(mark);
    scan_console_log_singleton_roots_mut(&mut visitor);
}

pub fn scan_console_log_singleton_roots_mut(visitor: &mut crate::gc::RuntimeRootVisitor<'_>) {
    visitor.visit_atomic_i64_slot(&CONSOLE_LOG_SINGLETON, Ordering::Acquire, Ordering::Release);
    let mut moved = Vec::new();
    CONSOLE_INSTANCES.with(|instances| {
        let mut instances = instances.borrow_mut();
        for (&owner, state) in instances.iter_mut() {
            let mut new_owner = owner;
            if visitor.visit_metadata_usize_slot(&mut new_owner) {
                moved.push((owner, new_owner));
            }
            visitor.visit_nanbox_f64_slot(&mut state.stdout);
            visitor.visit_nanbox_f64_slot(&mut state.stderr);
        }
        for (old_owner, new_owner) in moved.drain(..) {
            if let Some(state) = instances.remove(&old_owner) {
                instances.insert(new_owner, state);
            }
        }
    });
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
    } else if value.fract() == 0.0 && value.abs() < INT_EXACT_FASTPATH_LIMIT {
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
        } else if n.fract() == 0.0 && n.abs() < INT_EXACT_FASTPATH_LIMIT {
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
    } else if value.fract() == 0.0 && value.abs() < INT_EXACT_FASTPATH_LIMIT {
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
        } else if n.fract() == 0.0 && n.abs() < INT_EXACT_FASTPATH_LIMIT {
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
    } else if value.fract() == 0.0 && value.abs() < INT_EXACT_FASTPATH_LIMIT {
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

    crate::node_submodules::diagnostics_channel_publish_console("log", arr_ptr);
    print_console_formatted(arr_ptr, false);
}

#[no_mangle]
pub extern "C" fn js_console_info_spread(arr_ptr: *const crate::array::ArrayHeader) {
    if arr_ptr.is_null() {
        println!();
        return;
    }

    crate::node_submodules::diagnostics_channel_publish_console("info", arr_ptr);
    print_console_formatted(arr_ptr, false);
}

#[no_mangle]
pub extern "C" fn js_console_debug_spread(arr_ptr: *const crate::array::ArrayHeader) {
    if arr_ptr.is_null() {
        println!();
        return;
    }

    crate::node_submodules::diagnostics_channel_publish_console("debug", arr_ptr);
    print_console_formatted(arr_ptr, false);
}

/// Print multiple values to stderr (console.error with spread support)
#[no_mangle]
pub extern "C" fn js_console_error_spread(arr_ptr: *const crate::array::ArrayHeader) {
    if arr_ptr.is_null() {
        eprintln!();
        return;
    }

    crate::node_submodules::diagnostics_channel_publish_console("error", arr_ptr);
    print_console_formatted(arr_ptr, true);
}

/// Print multiple values to stderr (console.warn with spread support)
#[no_mangle]
pub extern "C" fn js_console_warn_spread(arr_ptr: *const crate::array::ArrayHeader) {
    // console.warn is essentially the same as console.error in Node.js
    if arr_ptr.is_null() {
        eprintln!();
        return;
    }
    crate::node_submodules::diagnostics_channel_publish_console("warn", arr_ptr);
    print_console_formatted(arr_ptr, true);
}

fn print_console_formatted(arr_ptr: *const crate::array::ArrayHeader, stderr: bool) {
    let formatted = js_util_format(arr_ptr);
    let text = jsvalue_string_content(formatted).unwrap_or_default();
    print_console_text(&text, stderr);
    crate::node_submodules::diagnostics_channel_drain_uncaught();
}

fn print_console_text(text: &str, stderr: bool) {
    let prefix = console_group_prefix();
    let mut out = String::new();
    if prefix.is_empty() {
        out.push_str(text);
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

fn console_undefined() -> f64 {
    f64::from_bits(JSValue::undefined().bits())
}

fn throw_plain_console_error(message: &str) -> ! {
    let msg = js_string_from_bytes(message.as_ptr(), message.len() as u32);
    let err = crate::error::js_error_new_with_message(msg);
    crate::exception::js_throw(crate::value::js_nanbox_pointer(err as i64))
}

fn throw_console_symbol_to_string_type_error() -> ! {
    let message = b"Cannot convert a Symbol value to a string";
    let msg = js_string_from_bytes(message.as_ptr(), message.len() as u32);
    let err = crate::error::js_typeerror_new(msg);
    crate::exception::js_throw(crate::value::js_nanbox_pointer(err as i64))
}

fn console_is_callable(value: f64) -> bool {
    let ptr = crate::value::js_nanbox_get_pointer(value) as usize;
    ptr >= 0x1000 && crate::closure::is_closure_ptr(ptr)
}

unsafe fn console_string_len(value: f64) -> Option<usize> {
    let jsval = JSValue::from_bits(value.to_bits());
    if !jsval.is_any_string() {
        return None;
    }
    let ptr = crate::value::js_jsvalue_to_string(value) as *const StringHeader;
    if ptr.is_null() {
        return None;
    }
    Some((*ptr).byte_len as usize)
}

fn console_make_named_function(
    scope: &crate::gc::RuntimeHandleScope,
    name: &str,
    func_ptr: *const u8,
    call_arity: u32,
    exposed_length: u32,
) -> f64 {
    crate::closure::js_register_closure_arity(func_ptr, call_arity);
    let closure = crate::closure::js_closure_alloc(func_ptr, 0);
    let closure_handle = scope.root_raw_mut_ptr(closure);
    crate::object::set_bound_native_closure_name(
        closure_handle.get_raw_mut_ptr::<crate::closure::ClosureHeader>(),
        name,
    );
    crate::object::set_builtin_closure_length(
        closure_handle.get_raw_mut_ptr::<crate::closure::ClosureHeader>() as usize,
        exposed_length,
    );
    let closure = closure_handle.get_raw_mut_ptr::<crate::closure::ClosureHeader>();
    crate::value::js_nanbox_pointer(closure as i64)
}

fn console_set_field(obj: *mut crate::object::ObjectHeader, name: &str, value: f64) {
    let key = js_string_from_bytes(name.as_ptr(), name.len() as u32);
    crate::object::js_object_set_field_by_name(obj, key, value);
}

extern "C" fn console_context_method_noop(_closure: *const crate::closure::ClosureHeader) -> f64 {
    console_undefined()
}

extern "C" fn console_task_run(
    _closure: *const crate::closure::ClosureHeader,
    callback: f64,
) -> f64 {
    if !console_is_callable(callback) {
        throw_plain_console_error("First argument must be a function.");
    }
    unsafe { crate::closure::js_native_call_value(callback, std::ptr::null(), 0) }
}

/// `console.context([name])` returns an inspector-scoped console object in
/// Node. Perry has no inspector context plumbing here, but the object shape is
/// observable by feature detection.
#[no_mangle]
pub extern "C" fn js_console_context(name: f64) -> f64 {
    let jsval = JSValue::from_bits(name.to_bits());
    if !jsval.is_undefined() {
        if unsafe { crate::symbol::js_is_symbol(name) != 0 } {
            throw_console_symbol_to_string_type_error();
        }
        let _ = crate::value::js_jsvalue_to_string(name);
    }

    let scope = crate::gc::RuntimeHandleScope::new();
    let obj = crate::object::js_object_alloc(0, 0);
    let obj_handle = scope.root_raw_mut_ptr(obj);
    for method in [
        "assert",
        "clear",
        "count",
        "countReset",
        "debug",
        "dir",
        "dirXml",
        "error",
        "group",
        "groupCollapsed",
        "groupEnd",
        "info",
        "log",
        "profile",
        "profileEnd",
        "table",
        "time",
        "timeEnd",
        "timeLog",
        "timeStamp",
        "trace",
        "warn",
    ] {
        let func = console_make_named_function(
            &scope,
            method,
            console_context_method_noop as *const u8,
            0,
            1,
        );
        let func_handle = scope.root_nanbox_f64(func);
        console_set_field(
            obj_handle.get_raw_mut_ptr::<crate::object::ObjectHeader>(),
            method,
            func_handle.get_nanbox_f64(),
        );
    }

    crate::value::js_nanbox_pointer(
        obj_handle.get_raw_mut_ptr::<crate::object::ObjectHeader>() as i64
    )
}

/// `console.createTask(name)` is V8 inspector async-stack tagging. The
/// compatibility surface returns `{ run(fn) }`; `run` executes and forwards
/// the callback result while preserving thrown errors.
#[no_mangle]
pub extern "C" fn js_console_create_task(name: f64) -> f64 {
    let len = unsafe { console_string_len(name) };
    if len.unwrap_or(0) == 0 {
        throw_plain_console_error("First argument must be a non-empty string.");
    }

    let scope = crate::gc::RuntimeHandleScope::new();
    let obj = crate::object::js_object_alloc(0, 0);
    let obj_handle = scope.root_raw_mut_ptr(obj);
    let run = console_make_named_function(&scope, "run", console_task_run as *const u8, 1, 0);
    let run_handle = scope.root_nanbox_f64(run);
    console_set_field(
        obj_handle.get_raw_mut_ptr::<crate::object::ObjectHeader>(),
        "run",
        run_handle.get_nanbox_f64(),
    );
    crate::value::js_nanbox_pointer(
        obj_handle.get_raw_mut_ptr::<crate::object::ObjectHeader>() as i64
    )
}

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
    static CONSOLE_INSTANCES: RefCell<HashMap<usize, ConsoleInstanceState>> = RefCell::new(HashMap::new());
}

pub(crate) const CONSOLE_INSTANCE_CLASS_ID: u32 = 0xFFFF_0083;

pub(crate) fn is_console_instance_method_name(method_name: &str) -> bool {
    matches!(
        method_name,
        "log"
            | "info"
            | "debug"
            | "dir"
            | "dirxml"
            | "error"
            | "warn"
            | "count"
            | "countReset"
            | "group"
            | "groupCollapsed"
            | "groupEnd"
            | "clear"
            | "profile"
            | "profileEnd"
            | "timeStamp"
    )
}

pub(crate) fn is_console_instance_value(value: f64) -> bool {
    let jsval = JSValue::from_bits(value.to_bits());
    if !jsval.is_pointer() {
        return false;
    }
    let obj = jsval.as_pointer::<crate::object::ObjectHeader>();
    if crate::value::addr_class::is_handle_band(obj as usize) {
        return false;
    }
    unsafe { (*obj).class_id == CONSOLE_INSTANCE_CLASS_ID }
}

struct ConsoleInstanceState {
    stdout: f64,
    stderr: f64,
    counters: HashMap<String, u64>,
    indent: usize,
    _ignore_errors: bool,
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

fn undefined_value() -> f64 {
    f64::from_bits(crate::value::TAG_UNDEFINED)
}

fn null_or_undefined(value: f64) -> bool {
    matches!(
        value.to_bits(),
        crate::value::TAG_NULL | crate::value::TAG_UNDEFINED
    )
}

fn object_property(value: f64, name: &[u8]) -> f64 {
    let jsval = JSValue::from_bits(value.to_bits());
    if !jsval.is_pointer() {
        return undefined_value();
    }
    let obj = jsval.as_pointer::<crate::object::ObjectHeader>();
    if obj.is_null() {
        return undefined_value();
    }
    let key = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
    f64::from_bits(crate::object::js_object_get_field_by_name(obj, key).bits())
}

fn value_gc_type(value: f64) -> Option<u8> {
    let jsval = JSValue::from_bits(value.to_bits());
    if !jsval.is_pointer() {
        return None;
    }
    let ptr = jsval.as_pointer::<u8>();
    if ptr.is_null() || (ptr as usize) < 0x10000 || ((ptr as u64) >> 48) != 0 {
        return None;
    }
    let gc_header = unsafe { &*(ptr.sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader) };
    Some(gc_header.obj_type)
}

fn is_plain_object(value: f64) -> bool {
    value_gc_type(value) == Some(crate::gc::GC_TYPE_OBJECT)
}

#[cold]
fn throw_console_writable_stream(stream_name: &str) -> ! {
    // Mirror Node's `ERR_CONSOLE_WRITABLE_STREAM` wording, which names the
    // offending stream (e.g. "... instance for stdout").
    let msg = format!("Console expects a writable stream instance for {stream_name}");
    let bytes = msg.as_bytes();
    let s = crate::string::js_string_from_bytes(bytes.as_ptr(), bytes.len() as u32);
    crate::node_submodules::register_error_code_pub(s, "ERR_CONSOLE_WRITABLE_STREAM");
    let err = crate::error::js_typeerror_new(s);
    crate::exception::js_throw(crate::value::js_nanbox_pointer(err as i64))
}

/// True when `value` is callable, matching Node's `typeof x === "function"`.
/// Reuses the canonical `typeof` implementation so closures, function
/// references and class refs are all classified consistently.
fn is_callable(value: f64) -> bool {
    let header = crate::builtins::arithmetic::js_value_typeof(value);
    if header.is_null() {
        return false;
    }
    unsafe {
        let len = (*header).byte_len as usize;
        let data = (header as *const u8).add(std::mem::size_of::<crate::string::StringHeader>());
        std::slice::from_raw_parts(data, len) == b"function"
    }
}

/// True when `value` is an object that exposes a callable `write` method, which
/// is Node's definition of an acceptable Console stream.
fn has_write_method(value: f64) -> bool {
    if null_or_undefined(value) {
        return false;
    }
    is_callable(object_property(value, b"write"))
}

fn color_mode_received(value: f64) -> String {
    let jsval = JSValue::from_bits(value.to_bits());
    if jsval.is_null() {
        return "null".to_string();
    }
    if jsval.is_undefined() {
        return "undefined".to_string();
    }
    if jsval.is_bool() {
        return jsval.as_bool().to_string();
    }
    if jsval.is_int32() {
        return jsval.as_int32().to_string();
    }
    if jsval.is_number() {
        let n = jsval.as_number();
        if n.fract() == 0.0 && n.is_finite() {
            return (n as i64).to_string();
        }
        return n.to_string();
    }
    if jsval.is_any_string() {
        return format!("'{}'", jsvalue_string_content(value).unwrap_or_default());
    }
    format_jsvalue(value, 0)
}

#[cold]
fn throw_invalid_color_mode(value: f64) -> ! {
    let message = format!(
        "The argument 'colorMode' must be one of: 'auto', true, false. Received {}",
        color_mode_received(value)
    );
    crate::fs::validate::throw_type_error_with_code(&message, "ERR_INVALID_ARG_VALUE");
}

fn validate_console_constructor_options(options: f64) {
    if !is_plain_object(options) {
        return;
    }

    let inspect_options = object_property(options, b"inspectOptions");
    if !JSValue::from_bits(inspect_options.to_bits()).is_undefined()
        && !is_plain_object(inspect_options)
    {
        let message = format!(
            "The \"options.inspectOptions\" property must be of type object. Received {}",
            crate::fs::validate::describe_received(inspect_options)
        );
        crate::fs::validate::throw_type_error_with_code(&message, "ERR_INVALID_ARG_TYPE");
    }

    let color_mode = object_property(options, b"colorMode");
    let color_mode_value = JSValue::from_bits(color_mode.to_bits());
    if !color_mode_value.is_undefined()
        && !color_mode_value.is_bool()
        && !(color_mode_value.is_any_string()
            && jsvalue_string_content(color_mode).as_deref() == Some("auto"))
    {
        throw_invalid_color_mode(color_mode);
    }

    let group_indentation = object_property(options, b"groupIndentation");
    if !JSValue::from_bits(group_indentation.to_bits()).is_undefined() {
        crate::fs::validate::validate_int32(group_indentation, "groupIndentation", 0, 1000);
    }
}

#[no_mangle]
pub extern "C" fn js_console_new(options: f64) -> f64 {
    // Both `new Console(stdout, stderr?)` and `new Console(options)` are
    // normalized by codegen into a single value exposing `.stdout` / `.stderr`.
    // The lone exception is `new Console(stream)` where the single positional
    // argument is itself a writable stream — there it has no `.stdout` of its
    // own, so detect that case by checking whether the argument is itself a
    // writable stream.
    let stdout_prop = object_property(options, b"stdout");
    let stdout;
    let stderr_candidate;
    let options_value;
    if !null_or_undefined(stdout_prop) {
        stdout = stdout_prop;
        stderr_candidate = object_property(options, b"stderr");
        options_value = options;
    } else if has_write_method(options) {
        // Single positional stream form: the argument is stdout itself.
        stdout = options;
        stderr_candidate = undefined_value();
        options_value = undefined_value();
    } else {
        // Options-object form whose stdout is missing/nullish.
        stdout = stdout_prop;
        stderr_candidate = object_property(options, b"stderr");
        options_value = options;
    }

    let ignore_errors = unsafe { decode_dir_bool_option(options, "ignoreErrors") }.unwrap_or(true);
    js_console_new_resolved(stdout, stderr_candidate, ignore_errors, options_value)
}

#[no_mangle]
pub extern "C" fn js_console_new2(stdout: f64, stderr_candidate: f64) -> f64 {
    let has_options_stdout = !null_or_undefined(object_property(stdout, b"stdout"));
    if null_or_undefined(stderr_candidate) && has_options_stdout {
        return js_console_new(stdout);
    }
    js_console_new_resolved(stdout, stderr_candidate, true, undefined_value())
}

fn js_console_new_resolved(
    stdout: f64,
    stderr_candidate: f64,
    ignore_errors: bool,
    options_value: f64,
) -> f64 {
    if !has_write_method(stdout) {
        throw_console_writable_stream("stdout");
    }
    // stderr is validated only when explicitly provided (non-nullish);
    // otherwise it defaults to stdout, matching Node.
    if !null_or_undefined(stderr_candidate) && !has_write_method(stderr_candidate) {
        throw_console_writable_stream("stderr");
    }
    validate_console_constructor_options(options_value);
    let stderr = if null_or_undefined(stderr_candidate) {
        stdout
    } else {
        stderr_candidate
    };
    let obj = crate::object::js_object_alloc(CONSOLE_INSTANCE_CLASS_ID, 0);
    let value = crate::value::js_nanbox_pointer(obj as i64);
    CONSOLE_INSTANCES.with(|instances| {
        instances.borrow_mut().insert(
            obj as usize,
            ConsoleInstanceState {
                stdout,
                stderr,
                counters: HashMap::new(),
                indent: 0,
                _ignore_errors: ignore_errors,
            },
        );
    });
    value
}

enum ConsoleInstanceAction {
    Write {
        stream: f64,
        line: String,
        stderr: bool,
    },
    Noop,
}

fn format_console_instance_args(args: &[f64]) -> String {
    let arr = crate::array::js_array_alloc(args.len() as u32);
    for arg in args {
        crate::array::js_array_push_f64(arr, *arg);
    }
    jsvalue_string_content(js_util_format(arr)).unwrap_or_default()
}

fn console_instance_label(args: &[f64]) -> String {
    let label_value = args.first().copied().unwrap_or_else(undefined_value);
    unsafe { label_from_str_ptr(console_label_from_value(label_value)) }
}

fn console_instance_write(stream: f64, line: &str, stderr: bool) {
    if null_or_undefined(stream) {
        if stderr {
            eprintln!("{line}");
        } else {
            println!("{line}");
        }
        return;
    }

    let mut chunk = String::with_capacity(line.len() + 1);
    chunk.push_str(line);
    chunk.push('\n');
    let chunk_ptr = crate::string::js_string_from_bytes(chunk.as_ptr(), chunk.len() as u32);
    let chunk_value = f64::from_bits(JSValue::string_ptr(chunk_ptr).bits());
    let args = [chunk_value];
    unsafe {
        let _ = crate::object::js_native_call_method(
            stream,
            b"write".as_ptr() as *const i8,
            5,
            args.as_ptr(),
            args.len(),
        );
    }
}

pub(crate) unsafe fn try_console_instance_method_dispatch(
    obj: *const crate::object::ObjectHeader,
    method_name: &str,
    args_ptr: *const f64,
    args_len: usize,
) -> Option<f64> {
    if obj.is_null() || (*obj).class_id != CONSOLE_INSTANCE_CLASS_ID {
        return None;
    }
    let args = if args_ptr.is_null() || args_len == 0 {
        &[][..]
    } else {
        std::slice::from_raw_parts(args_ptr, args_len)
    };

    let action = CONSOLE_INSTANCES.with(|instances| {
        let mut instances = instances.borrow_mut();
        let state = instances.get_mut(&(obj as usize))?;
        let indent = "  ".repeat(state.indent);
        match method_name {
            "log" | "info" | "debug" | "dir" | "dirxml" => Some(ConsoleInstanceAction::Write {
                stream: state.stdout,
                line: format!("{indent}{}", format_console_instance_args(args)),
                stderr: false,
            }),
            "error" | "warn" => Some(ConsoleInstanceAction::Write {
                stream: state.stderr,
                line: format!("{indent}{}", format_console_instance_args(args)),
                stderr: true,
            }),
            "count" => {
                let label = console_instance_label(args);
                let count = state.counters.entry(label.clone()).or_insert(0);
                *count += 1;
                Some(ConsoleInstanceAction::Write {
                    stream: state.stdout,
                    line: format!("{indent}{label}: {count}"),
                    stderr: false,
                })
            }
            "countReset" => {
                let label = console_instance_label(args);
                if state.counters.remove(&label).is_none() {
                    Some(ConsoleInstanceAction::Write {
                        stream: state.stderr,
                        line: format!("Warning: Count for '{label}' does not exist"),
                        stderr: true,
                    })
                } else {
                    Some(ConsoleInstanceAction::Noop)
                }
            }
            "group" | "groupCollapsed" => {
                let line = if args.is_empty() {
                    None
                } else {
                    Some(format!("{indent}{}", format_console_instance_args(args)))
                };
                state.indent += 1;
                line.map(|line| ConsoleInstanceAction::Write {
                    stream: state.stdout,
                    line,
                    stderr: false,
                })
                .or(Some(ConsoleInstanceAction::Noop))
            }
            "groupEnd" => {
                if state.indent > 0 {
                    state.indent -= 1;
                }
                Some(ConsoleInstanceAction::Noop)
            }
            "clear" | "profile" | "profileEnd" | "timeStamp" => Some(ConsoleInstanceAction::Noop),
            _ => None,
        }
    })?;

    match action {
        ConsoleInstanceAction::Write {
            stream,
            line,
            stderr,
        } => console_instance_write(stream, &line, stderr),
        ConsoleInstanceAction::Noop => {}
    }
    Some(undefined_value())
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

fn warn_console_count_missing(label: &str) {
    eprintln!("Warning: Count for '{}' does not exist", label);
}

#[no_mangle]
pub extern "C" fn js_console_count_reset_value(label_value: f64) {
    let jsval = JSValue::from_bits(label_value.to_bits());
    if jsval.is_undefined() || jsval.is_any_string() {
        js_console_count_reset(console_label_from_value(label_value));
        return;
    }
    if jsval.is_pointer() {
        let ptr = jsval.as_pointer::<u8>() as usize;
        if crate::symbol::is_registered_symbol(ptr) {
            console_type_error_for_symbol_label();
        }
    }

    // Node's current countReset checks the raw label against its counter map
    // before deleting `${label}`. Non-string labels counted under their string
    // form therefore warn and do not reset.
    let label_ptr = js_string_coerce(label_value) as *const StringHeader;
    let label = unsafe { label_from_str_ptr(label_ptr) };
    warn_console_count_missing(&label);
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
        warn_console_count_missing(&label);
        return;
    }
    CONSOLE_COUNTERS.with(|c| {
        let mut map = c.borrow_mut();
        if map.remove(&label).is_none() {
            warn_console_count_missing(&label);
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
// Node writes `Trace: <msg>` + a JS stack trace to **stderr**. Perry's stack
// traces are intentionally coarse (see `error.rs` `make_stack`: "Real stack
// traces are not implemented"), so console.trace emits the same `at <anonymous>`
// frame `Error().stack` does. We deliberately do NOT use
// `std::backtrace::Backtrace::force_capture()`: it pulled the std DWARF
// symbolizer (gimli/addr2line/dwarf, ~143KB) into every binary, and on stripped
// release builds every frame symbolicated to `__mh_execute_header` anyway.
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
    // Perry stack traces are intentionally coarse (error.rs make_stack:
    // "Real stack traces are not implemented") — match `Error().stack`
    // and drop the std DWARF symbolizer (gimli/addr2line/dwarf ~143KB) that
    // `Backtrace::force_capture()` pulls into every binary; on stripped
    // release builds those frames are all `__mh_execute_header` anyway.
    emit_console_trace_stack();
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
    // Coarse JS frame, consistent with `Error().stack`. Avoids
    // `std::backtrace::Backtrace` so the std DWARF symbolizer is not linked.
    eprintln!("    at <anonymous>");
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
pub(crate) unsafe fn decode_dir_depth_option(options_value: f64) -> Option<usize> {
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
    decode_dir_bool_option(options_value, "showHidden")
}

/// Generic boolean-option decoder for the options object passed to
/// `console.dir` / `util.inspect`. Honors Node's truthy/falsy coercion for
/// the common scalar shapes (bool, int, number, null/undefined). Returns
/// `None` when the option is absent so callers can supply a default.
pub(crate) unsafe fn decode_dir_bool_option(options_value: f64, option_name: &str) -> Option<bool> {
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
    let target = option_name.as_bytes();
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
        if key_bytes != target {
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
    // Node's `console.dir` defaults to `customInspect: false`, surfacing the
    // hook as a regular `[Symbol(nodejs.util.inspect.custom)]: ...` property
    // instead of invoking it. The option is overridable via the second arg.
    // Refs #1201.
    let custom_inspect = decode_dir_bool_option(options_value, "customInspect").unwrap_or(false);
    let getters = decode_dir_bool_option(options_value, "getters").unwrap_or(false);
    let sorted = decode_dir_bool_option(options_value, "sorted").unwrap_or(false);
    let compact = decode_dir_bool_option(options_value, "compact").unwrap_or(true);
    let _depth_guard = InspectDepthLimitGuard::new(max_depth);
    let _hidden_guard = InspectShowHiddenGuard::new(show_hidden);
    let _custom_guard = InspectCustomInspectGuard::new(custom_inspect);
    let _getters_guard = InspectGettersGuard::new(getters);
    let _sorted_guard = InspectSortedGuard::new(sorted);
    let _compact_guard = InspectCompactGuard::new(compact);
    println!("{}", format_jsvalue(value, 0));
}

#[cfg(test)]
pub(crate) fn test_set_console_log_singleton(ptr: i64) {
    crate::gc::runtime_store_root_atomic_raw_i64(&CONSOLE_LOG_SINGLETON, ptr, Ordering::Release);
}

#[cfg(test)]
pub(crate) fn test_console_log_singleton() -> i64 {
    CONSOLE_LOG_SINGLETON.load(Ordering::Acquire)
}
