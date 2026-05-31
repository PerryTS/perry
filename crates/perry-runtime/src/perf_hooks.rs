//! node:perf_hooks runtime support — W3C User Timing (`performance.mark` /
//! `performance.measure` + the timeline query/clear methods),
//! `performance.timeOrigin`, and `performance.eventLoopUtilization`.
//!
//! `performance` is bound (in HIR lowering) to a native-module namespace
//! object tagged `"perf_hooks"`, so:
//!   * `typeof performance` → "object"
//!   * `performance.mark(...)` / `.measure(...)` / `.getEntries*` / `.clear*`
//!     dispatch here via `dispatch_native_module_method`
//!   * `performance.now` / `.mark` / … read as values resolve to bound-method
//!     closures (`is_native_module_callable_export`)
//!   * `performance.timeOrigin` resolves via `get_native_module_constant`
//!
//! The timeline is a per-thread `Vec<PerfEntry>`. Mark/Measure result objects
//! are plain shaped objects with the Node fields
//! `{ name, entryType, startTime, duration, detail }`. The `detail` slot can
//! hold an arbitrary heap JSValue, so the store is registered as a GC root
//! scanner (`scan_perf_entries_roots_mut`).

use crate::object::{
    js_object_alloc_with_shape, js_object_get_field, js_object_get_field_by_name,
    js_object_set_field,
};
use crate::string::StringHeader;
use crate::value::JSValue;
use std::cell::{Cell, RefCell};
use std::sync::OnceLock;
use std::time::{SystemTime, UNIX_EPOCH};

const ENTRY_TYPE_MARK: u8 = 0;
const ENTRY_TYPE_MEASURE: u8 = 1;

pub(crate) const CLASS_ID_PERFORMANCE_ENTRY: u32 = 0xFFFF_0080;
pub(crate) const CLASS_ID_PERFORMANCE_MARK: u32 = 0xFFFF_0081;
pub(crate) const CLASS_ID_PERFORMANCE_MEASURE: u32 = 0xFFFF_0082;

/// Shape id for the `{ name, entryType, startTime, duration, detail }` object
/// returned by mark/measure and the getEntries* arrays.
const PERF_ENTRY_SHAPE: u32 = 0x7FFF_FF40;
const PERF_ENTRY_KEYS: &[u8] = b"name\0entryType\0startTime\0duration\0detail\0";

/// Distinct shape for the plain object returned by `PerformanceEntry#toJSON()`
/// (#1387). Same field names as the entry, but a different shape id so its
/// `keys_array` allocation differs from the entry's — `is_perf_entry_object`
/// then reports `false` for the toJSON result, matching Node where the
/// serialized object is a plain object with no `toJSON` method of its own.
const PERF_ENTRY_JSON_SHAPE: u32 = 0x7FFF_FF42;

/// Shape id for the `{ idle, active, utilization }` eventLoopUtilization object.
const ELU_SHAPE: u32 = 0x7FFF_FF41;
const ELU_KEYS: &[u8] = b"idle\0active\0utilization\0";

/// Shape id for the `{ timeOrigin }` snapshot returned by `performance.toJSON()`.
const TOJSON_SHAPE: u32 = 0x7FFF_FF42;
const TOJSON_KEYS: &[u8] = b"timeOrigin\0";

/// Shape id + keys for `performance.nodeTiming` (PerformanceNodeTiming entry).
const NODE_TIMING_SHAPE: u32 = 0x7FFF_FF43;
const NODE_TIMING_KEYS: &[u8] = b"name\0entryType\0startTime\0duration\0nodeStart\0v8Start\0bootstrapComplete\0environment\0loopStart\0loopExit\0idleTime\0";

#[derive(Clone)]
struct PerfEntry {
    name: String,
    entry_type: u8,
    start_time: f64,
    duration: f64,
    /// NaN-boxed JSValue bits of the entry's `detail` (defaults to `null`).
    detail_bits: u64,
}

thread_local! {
    static PERF_ENTRIES: RefCell<Vec<PerfEntry>> = const { RefCell::new(Vec::new()) };
    /// Cached `performance` namespace object (NaN-boxed bits, 0 = uninit).
    /// Singleton so the named import and `globalThis.performance` are the same
    /// object (Node identity). GC-rooted in `scan_perf_entries_roots_mut`.
    static PERFORMANCE_NS: Cell<u64> = const { Cell::new(0) };

    /// The `keys_array` pointer shared by every entry object on this thread.
    /// `js_object_alloc_with_shape` caches one `keys_array` per shape id, so
    /// all `PERF_ENTRY_SHAPE` objects share the same allocation — recording it
    /// once lets `is_perf_entry_object` recognize an entry with a single
    /// pointer compare (no per-key string matching, no GC-tracked registry of
    /// movable entry pointers). Set on the first `entry_to_object` call.
    static PERF_ENTRY_KEYS_ARRAY: Cell<usize> = const { Cell::new(0) };
}

/// True when `obj` is a mark/measure entry object produced by
/// `entry_to_object` — i.e. its `keys_array` is the recorded shared
/// `PERF_ENTRY_SHAPE` allocation. The toJSON-result object uses a different
/// shape, so it deliberately does not match. (#1387)
pub(crate) unsafe fn is_perf_entry_object(obj: *const crate::object::ObjectHeader) -> bool {
    if obj.is_null() {
        return false;
    }
    let recorded = PERF_ENTRY_KEYS_ARRAY.with(|c| c.get());
    recorded != 0 && (*obj).keys_array as usize == recorded
}

unsafe fn perf_entry_type(obj: *const crate::object::ObjectHeader) -> Option<u8> {
    let entry_type = string_of(js_object_get_field(obj, 1))?;
    match entry_type.as_str() {
        "mark" => Some(ENTRY_TYPE_MARK),
        "measure" => Some(ENTRY_TYPE_MEASURE),
        _ => None,
    }
}

pub(crate) unsafe fn is_perf_entry_object_instance_of(
    obj: *const crate::object::ObjectHeader,
    class_id: u32,
) -> Option<bool> {
    let want = match class_id {
        CLASS_ID_PERFORMANCE_ENTRY => None,
        CLASS_ID_PERFORMANCE_MARK => Some(ENTRY_TYPE_MARK),
        CLASS_ID_PERFORMANCE_MEASURE => Some(ENTRY_TYPE_MEASURE),
        _ => return None,
    };
    if !is_perf_entry_object(obj) {
        return Some(false);
    }
    Some(match want {
        None => true,
        Some(kind) => perf_entry_type(obj) == Some(kind),
    })
}

/// Build the plain object returned by `PerformanceEntry#toJSON()` — a copy of
/// the entry's `{ name, entryType, startTime, duration, detail }` fields under
/// a distinct shape so the result is itself a plain object (no synthesized
/// `toJSON`). Mirrors Node's serialization. (#1387)
pub(crate) unsafe fn perf_entry_to_json(this: f64) -> f64 {
    let jv = JSValue::from_bits(this.to_bits());
    if !jv.is_pointer() {
        return f64::from_bits(crate::value::TAG_UNDEFINED);
    }
    let src = jv.as_pointer::<crate::object::ObjectHeader>();
    if src.is_null() {
        return f64::from_bits(crate::value::TAG_UNDEFINED);
    }
    // Snapshot the 5 fields BEFORE allocating `out` — the alloc can trigger a
    // GC that relocates `src`, invalidating this raw pointer.
    let fields: [JSValue; 5] = std::array::from_fn(|i| js_object_get_field(src, i as u32));
    let out = js_object_alloc_with_shape(
        PERF_ENTRY_JSON_SHAPE,
        5,
        PERF_ENTRY_KEYS.as_ptr(),
        PERF_ENTRY_KEYS.len() as u32,
    );
    for (i, v) in fields.iter().enumerate() {
        js_object_set_field(out, i as u32, *v);
    }
    crate::value::js_nanbox_pointer(out as i64)
}

/// The per-thread singleton `performance` namespace object (perf_hooks-tagged).
/// Both the `node:perf_hooks` named import and `globalThis.performance` resolve
/// through here so `globalThis.performance === require("perf_hooks").performance`
/// holds, matching Node.
pub fn performance_namespace() -> f64 {
    let cached = PERFORMANCE_NS.with(|c| c.get());
    if cached != 0 {
        return f64::from_bits(cached);
    }
    let module = b"perf_hooks";
    let ns =
        unsafe { crate::object::js_create_native_module_namespace(module.as_ptr(), module.len()) };
    PERFORMANCE_NS.with(|c| c.set(ns.to_bits()));
    ns
}

/// `performance.timeOrigin` — ms since the Unix epoch captured at first read.
/// Node fixes this at process start; capturing it lazily (process-global via
/// `OnceLock`) is close enough and is always a positive number.
static TIME_ORIGIN_MS: OnceLock<f64> = OnceLock::new();

pub(crate) fn time_origin_ms() -> f64 {
    *TIME_ORIGIN_MS.get_or_init(|| {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs_f64() * 1000.0)
            .unwrap_or(0.0)
    })
}

/// Read a `*StringHeader` into an owned `String`.
unsafe fn header_to_string(p: *const StringHeader) -> String {
    if p.is_null() {
        return String::new();
    }
    let len = (*p).byte_len as usize;
    let data = (p as *const u8).add(std::mem::size_of::<StringHeader>());
    std::str::from_utf8(std::slice::from_raw_parts(data, len))
        .unwrap_or("")
        .to_string()
}

/// JS string-coerce an arg (`${value}`) into an owned `String`.
unsafe fn coerce_to_string(value: f64) -> String {
    let ptr = crate::builtins::js_string_coerce(value);
    header_to_string(ptr)
}

/// Decode a JSValue to an owned `String` iff it actually *is* a string,
/// accepting BOTH heap `STRING_TAG` pointers and inline `SHORT_STRING_TAG`
/// (SSO) values. Returns `None` for non-strings.
///
/// #1781: `is_string()` is STRING_TAG-only, so the old
/// `v.is_string() { header_to_string(v.as_string_ptr()) }` shape silently
/// dropped every short mark/measure/type name — and the common literals
/// `"mark"` (4 bytes) and observer `entryTypes: ["mark"]` are inline SSO.
unsafe fn string_of(v: JSValue) -> Option<String> {
    if v.is_string() {
        Some(header_to_string(v.as_string_ptr()))
    } else if v.is_short_string() {
        let mut buf = [0u8; crate::value::SHORT_STRING_MAX_LEN];
        let n = v.short_string_to_buf(&mut buf);
        Some(std::str::from_utf8(&buf[..n]).unwrap_or("").to_string())
    } else {
        None
    }
}

/// Format a timestamp the way Node does in the
/// `<n> is not a valid timestamp` message (JS number formatting: integral
/// values print without a trailing `.0`).
unsafe fn format_timestamp(n: f64) -> String {
    coerce_to_string(f64::from_bits(JSValue::number(n).bits()))
}

/// Read a JS value as an f64 if it is numeric, accepting both the int32 and
/// double NaN-box representations (`is_number()` alone misses int32 since
/// INT32_TAG falls inside the tagged range). Returns `None` otherwise.
fn num_of(v: JSValue) -> Option<f64> {
    if v.is_int32() {
        Some(v.as_int32() as f64)
    } else if v.is_number() {
        Some(v.as_number())
    } else {
        None
    }
}

/// Throw a `TypeError` with `msg` (catchable by user `try/catch` as a
/// TypeError, matching Node's input-validation errors). Never returns.
fn throw_type_error(msg: &str) -> ! {
    let msg_str = crate::string::js_string_from_bytes(msg.as_ptr(), msg.len() as u32);
    let err_ptr = crate::error::js_typeerror_new(msg_str);
    let err_value = JSValue::pointer(err_ptr as *const u8).bits();
    crate::exception::js_throw(f64::from_bits(err_value))
}

/// Throw a `SyntaxError` with `msg`. Node surfaces a missing-named-mark in
/// `measure()` as a DOMException with `name === "SyntaxError"`; Perry doesn't
/// implement DOMException, so a `SyntaxError` matches the `err.name` /
/// `err.message` user code observes. Never returns.
fn throw_syntax_error(msg: &str) -> ! {
    let msg_str = crate::string::js_string_from_bytes(msg.as_ptr(), msg.len() as u32);
    let err_ptr = crate::error::js_syntaxerror_new(msg_str);
    let err_value = JSValue::pointer(err_ptr as *const u8).bits();
    crate::exception::js_throw(f64::from_bits(err_value))
}

/// Node's runtime type label for a JS value (used in ERR_INVALID_ARG_TYPE
/// "Received ..." suffixes). Approximates `node:internal/errors` formatting
/// closely enough for the common validation cases.
unsafe fn received_label(v: f64) -> String {
    let jv = JSValue::from_bits(v.to_bits());
    if jv.is_undefined() {
        return "undefined".to_string();
    }
    if crate::symbol::js_is_symbol(v) != 0 {
        let desc = crate::symbol::js_symbol_description(v);
        let d = string_of(JSValue::from_bits(desc.to_bits())).unwrap_or_default();
        return format!("type symbol (Symbol({d}))");
    }
    let p = crate::builtins::js_value_typeof(v) as *const StringHeader;
    match header_to_string(p).as_str() {
        "object" if jv.is_null() => "null".to_string(),
        // Objects/arrays render as `an instance of <Ctor>` in Node's
        // ERR_INVALID_ARG_TYPE formatting.
        "object" => {
            if crate::array::js_array_is_array(v).to_bits() == crate::value::TAG_TRUE {
                "an instance of Array".to_string()
            } else {
                "an instance of Object".to_string()
            }
        }
        "number" | "boolean" => {
            let s = coerce_to_string(v);
            format!("type {} ({s})", header_to_string(p))
        }
        "string" => {
            // Node single-quotes the received string value: `type string ('x')`.
            let s = coerce_to_string(v);
            format!("type string ('{s}')")
        }
        ty => format!("type {ty}"),
    }
}

/// Render a value for the `Received ...` suffix of the both-specified
/// ERR_INVALID_ARG_VALUE message — Node uses `util.inspect`, which prints a
/// string array of entry types as e.g. `[ 'measure' ]`. Only the array form
/// matters here (the value is `options.entryTypes`).
unsafe fn format_value_for_error(v: f64) -> String {
    let jv = JSValue::from_bits(v.to_bits());
    if jv.is_pointer() && crate::array::js_array_is_array(v).to_bits() == crate::value::TAG_TRUE {
        let arr = jv.as_pointer::<crate::array::ArrayHeader>();
        let len = crate::array::js_array_length(arr);
        if len == 0 {
            return "[]".to_string();
        }
        let mut parts: Vec<String> = Vec::with_capacity(len as usize);
        for i in 0..len {
            let el = crate::array::js_array_get(arr, i);
            match string_of(el) {
                Some(s) => parts.push(format!("'{s}'")),
                None => parts.push(coerce_to_string(f64::from_bits(el.bits()))),
            }
        }
        return format!("[ {} ]", parts.join(", "));
    }
    coerce_to_string(v)
}

/// Build a NaN-boxed string value from a Rust `&str`.
fn str_value(s: &str) -> JSValue {
    let ptr = crate::string::js_string_from_bytes(s.as_ptr(), s.len() as u32);
    JSValue::string_ptr(ptr)
}

/// Materialize a `PerfEntry` into a `{ name, entryType, startTime, duration,
/// detail }` JS object and return its NaN-boxed pointer bits.
unsafe fn entry_to_object(e: &PerfEntry) -> f64 {
    let obj = js_object_alloc_with_shape(
        PERF_ENTRY_SHAPE,
        5,
        PERF_ENTRY_KEYS.as_ptr(),
        PERF_ENTRY_KEYS.len() as u32,
    );
    // Record the shared keys_array so `is_perf_entry_object` can recognize
    // entries by pointer identity (see PERF_ENTRY_KEYS_ARRAY). All entries on
    // this thread share it, so a single store on the first call suffices.
    let keys_ptr = (*obj).keys_array as usize;
    PERF_ENTRY_KEYS_ARRAY.with(|c| {
        if c.get() == 0 {
            c.set(keys_ptr);
        }
    });
    let type_str = if e.entry_type == ENTRY_TYPE_MEASURE {
        "measure"
    } else {
        "mark"
    };
    js_object_set_field(obj, 0, str_value(&e.name));
    js_object_set_field(obj, 1, str_value(type_str));
    js_object_set_field(obj, 2, JSValue::number(e.start_time));
    js_object_set_field(obj, 3, JSValue::number(e.duration));
    js_object_set_field(obj, 4, JSValue::from_bits(e.detail_bits));
    crate::value::js_nanbox_pointer(obj as i64)
}

/// `performance.now()` reading used for default mark startTimes / measure
/// endpoints. Mirrors `js_performance_now` (ms since epoch); the absolute
/// origin is irrelevant to User Timing arithmetic since marks share it.
fn perf_now() -> f64 {
    crate::date::js_performance_now()
}

/// Read an option field that may be a number or a mark-name string and
/// resolve it to a timeline value. Returns `None` when the field is absent
/// (undefined). Strings resolve to the most-recent same-named mark's
/// startTime (0 if not found, matching nothing-thrown lenient behavior).
unsafe fn resolve_option_endpoint(
    options_obj: *const crate::object::ObjectHeader,
    key: &str,
) -> Option<f64> {
    let key_ptr = crate::string::js_string_from_bytes(key.as_ptr(), key.len() as u32);
    let v = js_object_get_field_by_name(options_obj, key_ptr);
    if v.is_undefined() {
        return None;
    }
    Some(resolve_endpoint_value(v))
}

unsafe fn resolve_endpoint_value(v: JSValue) -> f64 {
    if let Some(n) = num_of(v) {
        n
    } else if let Some(name) = string_of(v) {
        // #3008: a string endpoint must name an existing mark — Node throws a
        // (DOMException) SyntaxError when it doesn't, rather than the old
        // silent-0 fallback.
        match lookup_mark_start(&name) {
            Some(t) => t,
            None => {
                throw_syntax_error(&format!("The \"{name}\" performance mark has not been set"))
            }
        }
    } else {
        0.0
    }
}

/// Resolve a positional `measure(name, startMark, endMark?)` endpoint. A number
/// passes through; a string must name an existing mark — Node throws when it
/// doesn't (the silent-0 fallback used by the options form isn't valid for
/// positional start/end marks).
unsafe fn resolve_positional_endpoint(v: JSValue) -> f64 {
    if let Some(n) = num_of(v) {
        n
    } else if let Some(name) = string_of(v) {
        match lookup_mark_start(&name) {
            Some(t) => t,
            None => {
                throw_syntax_error(&format!("The \"{name}\" performance mark has not been set"))
            }
        }
    } else {
        0.0
    }
}

/// Most-recent mark startTime for `name`, if any.
fn lookup_mark_start(name: &str) -> Option<f64> {
    PERF_ENTRIES.with(|store| {
        store
            .borrow()
            .iter()
            .rev()
            .find(|e| e.entry_type == ENTRY_TYPE_MARK && e.name == name)
            .map(|e| e.start_time)
    })
}

unsafe fn option_number(options_obj: *const crate::object::ObjectHeader, key: &str) -> Option<f64> {
    let key_ptr = crate::string::js_string_from_bytes(key.as_ptr(), key.len() as u32);
    num_of(js_object_get_field_by_name(options_obj, key_ptr))
}

unsafe fn option_present(options_obj: *const crate::object::ObjectHeader, key: &str) -> bool {
    let key_ptr = crate::string::js_string_from_bytes(key.as_ptr(), key.len() as u32);
    !js_object_get_field_by_name(options_obj, key_ptr).is_undefined()
}

unsafe fn option_detail_bits(options_obj: *const crate::object::ObjectHeader) -> u64 {
    let key = b"detail";
    let key_ptr = crate::string::js_string_from_bytes(key.as_ptr(), key.len() as u32);
    let v = js_object_get_field_by_name(options_obj, key_ptr);
    if v.is_undefined() {
        JSValue::null().bits()
    } else {
        // #1513: Functions are not structured-cloneable — Node throws
        // DataCloneError. Perry's structuredClone passes closures through
        // silently, so detect the case up-front and throw a TypeError
        // (Perry doesn't implement DOMException; the test only checks
        // that *something* throws).
        if v.is_pointer() {
            let ptr = (v.bits() & 0x0000_FFFF_FFFF_FFFF) as usize;
            if crate::closure::is_closure_ptr(ptr) {
                throw_type_error("could not be cloned: a Function is not structured-cloneable");
            }
        }
        // Node structured-clones `detail`, so the stored value deep-equals the
        // input but is a distinct reference (mutating the original afterward
        // doesn't affect the entry).
        crate::builtins::js_structured_clone(f64::from_bits(v.bits())).to_bits()
    }
}

fn as_object_ptr(v: f64) -> Option<*const crate::object::ObjectHeader> {
    let jv = JSValue::from_bits(v.to_bits());
    if jv.is_pointer() {
        Some(jv.as_pointer::<crate::object::ObjectHeader>() as *const _)
    } else {
        None
    }
}

// ── performance.mark(name, options?) ─────────────────────────────────────────
/// Returns a PerformanceMark object and appends it to the timeline.
#[no_mangle]
pub extern "C" fn js_perf_mark(name_val: f64, options_val: f64) -> f64 {
    unsafe {
        // A Symbol name cannot be coerced to a string (Node throws TypeError).
        if crate::symbol::js_is_symbol(name_val) != 0 {
            throw_type_error("Cannot convert a Symbol value to a string");
        }
        let name = coerce_to_string(name_val);
        let mut start_time = perf_now();
        let mut detail_bits = JSValue::null().bits();
        if let Some(opts) = as_object_ptr(options_val) {
            // startTime, when present, must be a finite number (Node:
            // ERR_INVALID_ARG_TYPE → a TypeError).
            if option_present(opts, "startTime") {
                let key = b"startTime";
                let kp = crate::string::js_string_from_bytes(key.as_ptr(), key.len() as u32);
                let raw = js_object_get_field_by_name(opts, kp);
                match num_of(raw) {
                    // #3008: a negative startTime is not a valid timestamp.
                    Some(st) if st < 0.0 => throw_type_error(&format!(
                        "{} is not a valid timestamp",
                        format_timestamp(st)
                    )),
                    Some(st) => start_time = st,
                    None => throw_type_error(&format!(
                        "The \"startTime\" argument must be of type number. Received {}",
                        received_label(f64::from_bits(raw.bits()))
                    )),
                }
            }
            detail_bits = option_detail_bits(opts);
        }
        let entry = PerfEntry {
            name,
            entry_type: ENTRY_TYPE_MARK,
            start_time,
            duration: 0.0,
            detail_bits,
        };
        let obj = entry_to_object(&entry);
        notify_observers(&entry);
        PERF_ENTRIES.with(|store| store.borrow_mut().push(entry));
        obj
    }
}

// ── performance.measure(name, startOrOptions?, end?) ─────────────────────────
/// Computes startTime/duration from positional marks or an options object,
/// appends a PerformanceMeasure to the timeline, and returns it.
#[no_mangle]
pub extern "C" fn js_perf_measure(name_val: f64, arg2: f64, arg3: f64) -> f64 {
    unsafe {
        // #3088: Node requires `measure(name)` to be a string (unlike `mark`,
        // which string-coerces). Every non-string name (undefined, null,
        // number, boolean, object, array, symbol) throws
        // TypeError [ERR_INVALID_ARG_TYPE].
        let name = match string_of(JSValue::from_bits(name_val.to_bits())) {
            Some(s) => s,
            None => throw_type_error(&format!(
                "The \"name\" argument must be of type string. Received {}",
                received_label(name_val)
            )),
        };
        let arg2_jv = JSValue::from_bits(arg2.to_bits());

        let (start_time, duration);
        if let Some(opts) = as_object_ptr(arg2) {
            // Options form: { start?, end?, duration?, detail? }
            let start_present = option_present(opts, "start");
            let end_present = option_present(opts, "end");
            let dur_present = option_present(opts, "duration");
            let dur = option_number(opts, "duration");

            // #3008: { start, end, duration } may not all be specified together
            // (Node: ERR_PERFORMANCE_MEASURE_INVALID_OPTIONS).
            if start_present && end_present && dur_present {
                throw_type_error(
                    "Must not have options.start, options.end, and options.duration specified",
                );
            }
            // #3008: a negative numeric duration is not a valid timestamp.
            if let Some(d) = dur {
                if d < 0.0 {
                    throw_type_error(&format!("{} is not a valid timestamp", format_timestamp(d)));
                }
            }

            let start_resolved = resolve_option_endpoint(opts, "start");
            let end_resolved = resolve_option_endpoint(opts, "end");

            let end = if end_present {
                end_resolved.unwrap_or(0.0)
            } else if let (Some(d), Some(s)) = (dur, start_resolved) {
                s + d
            } else {
                perf_now()
            };
            let start = if start_present {
                start_resolved.unwrap_or(0.0)
            } else if let Some(d) = dur {
                if end_present {
                    end - d
                } else {
                    0.0
                }
            } else {
                0.0
            };
            start_time = start;
            duration = dur.unwrap_or(end - start);

            let detail_bits = option_detail_bits(opts);
            return finish_measure(name, start_time, duration, detail_bits);
        } else if arg2_jv.is_any_string() {
            // Positional form: measure(name, startMark, endMark?)
            let start = resolve_positional_endpoint(arg2_jv);
            let arg3_jv = JSValue::from_bits(arg3.to_bits());
            let end = if arg3_jv.is_any_string() || arg3_jv.is_number() {
                resolve_positional_endpoint(arg3_jv)
            } else {
                perf_now()
            };
            start_time = start;
            duration = end - start;
        } else {
            // measure(name) — from time origin (0) to now.
            start_time = 0.0;
            duration = perf_now();
        }

        finish_measure(name, start_time, duration, JSValue::null().bits())
    }
}

unsafe fn finish_measure(name: String, start_time: f64, duration: f64, detail_bits: u64) -> f64 {
    let entry = PerfEntry {
        name,
        entry_type: ENTRY_TYPE_MEASURE,
        start_time,
        duration,
        detail_bits,
    };
    let obj = entry_to_object(&entry);
    notify_observers(&entry);
    PERF_ENTRIES.with(|store| store.borrow_mut().push(entry));
    obj
}

// ── getEntries / getEntriesByType / getEntriesByName ─────────────────────────
/// Order entries by startTime ascending, stable on ties (matches the order
/// Node returns from `getEntries*` and observer lists).
fn sort_entries_by_start_time(entries: &mut [PerfEntry]) {
    entries.sort_by(|a, b| {
        a.start_time
            .partial_cmp(&b.start_time)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
}

unsafe fn entries_to_array(filter: impl Fn(&PerfEntry) -> bool) -> f64 {
    let mut snapshot: Vec<PerfEntry> = PERF_ENTRIES.with(|store| {
        store
            .borrow()
            .iter()
            .filter(|e| filter(e))
            .cloned()
            .collect()
    });
    // Node returns timeline entries ordered by startTime (stable on ties).
    sort_entries_by_start_time(&mut snapshot);
    let mut arr = crate::array::js_array_alloc(snapshot.len() as u32);
    for e in &snapshot {
        let obj = entry_to_object(e);
        arr = crate::array::js_array_push(arr, JSValue::from_bits(obj.to_bits()));
    }
    crate::value::js_nanbox_pointer(arr as i64)
}

#[no_mangle]
pub extern "C" fn js_perf_get_entries() -> f64 {
    unsafe { entries_to_array(|_| true) }
}

#[no_mangle]
pub extern "C" fn js_perf_get_entries_by_type(type_val: f64) -> f64 {
    unsafe {
        let want = coerce_to_string(type_val);
        let want_type = if want == "measure" {
            ENTRY_TYPE_MEASURE
        } else {
            ENTRY_TYPE_MARK
        };
        // Only "mark"/"measure" are tracked; an unknown type yields [].
        if want != "mark" && want != "measure" {
            return entries_to_array(|_| false);
        }
        entries_to_array(move |e| e.entry_type == want_type)
    }
}

#[no_mangle]
pub extern "C" fn js_perf_get_entries_by_name(name_val: f64, type_val: f64) -> f64 {
    unsafe {
        let want_name = coerce_to_string(name_val);
        let type_jv = JSValue::from_bits(type_val.to_bits());
        let want_type: Option<u8> = if let Some(t) = string_of(type_jv) {
            match t.as_str() {
                "mark" => Some(ENTRY_TYPE_MARK),
                "measure" => Some(ENTRY_TYPE_MEASURE),
                _ => Some(255),
            }
        } else {
            None
        };
        entries_to_array(move |e| {
            e.name == want_name && want_type.map(|t| t == e.entry_type).unwrap_or(true)
        })
    }
}

// ── clearMarks / clearMeasures ───────────────────────────────────────────────
// `clearMarks()` / `clearMarks(undefined)` clear all marks; `clearMarks(name)`
// clears only same-named marks (Node parity). Return `undefined`.
unsafe fn clear_entries(entry_type: u8, name_val: f64) -> f64 {
    // A Symbol name cannot be coerced to a string (Node throws TypeError).
    if crate::symbol::js_is_symbol(name_val) != 0 {
        throw_type_error("Cannot convert a Symbol value to a string");
    }
    let name = if JSValue::from_bits(name_val.to_bits()).is_undefined() {
        None
    } else {
        Some(coerce_to_string(name_val))
    };
    PERF_ENTRIES.with(|store| {
        store.borrow_mut().retain(|e| {
            if e.entry_type != entry_type {
                return true;
            }
            match &name {
                Some(n) => &e.name != n,
                None => false,
            }
        });
    });
    f64::from_bits(JSValue::undefined().bits())
}

#[no_mangle]
pub extern "C" fn js_perf_clear_marks(name_val: f64) -> f64 {
    unsafe { clear_entries(ENTRY_TYPE_MARK, name_val) }
}

#[no_mangle]
pub extern "C" fn js_perf_clear_measures(name_val: f64) -> f64 {
    unsafe { clear_entries(ENTRY_TYPE_MEASURE, name_val) }
}

// ── eventLoopUtilization ─────────────────────────────────────────────────────
// Perry has no libuv event loop to instrument, so report a stable cumulative
// idle/active split anchored to wall-clock since timeOrigin. The result keeps
// Node's object shape and the diff form's utilization in [0, 1].
fn cumulative_idle_active() -> (f64, f64) {
    let elapsed = (perf_now() - time_origin_ms()).max(0.0);
    let active = elapsed * 0.05;
    let idle = elapsed - active;
    (idle, active)
}

unsafe fn make_elu_object(idle: f64, active: f64) -> f64 {
    let util = if idle + active > 0.0 {
        active / (idle + active)
    } else {
        0.0
    };
    let obj = js_object_alloc_with_shape(ELU_SHAPE, 3, ELU_KEYS.as_ptr(), ELU_KEYS.len() as u32);
    js_object_set_field(obj, 0, JSValue::number(idle));
    js_object_set_field(obj, 1, JSValue::number(active));
    js_object_set_field(obj, 2, JSValue::number(util));
    crate::value::js_nanbox_pointer(obj as i64)
}

/// Read the `idle`/`active` fields out of a prior ELU object value (0 if the
/// value is not an object or the field is missing).
unsafe fn elu_idle_active(v: f64) -> Option<(f64, f64)> {
    let obj = as_object_ptr(v)?;
    let read = |k: &[u8]| -> f64 {
        let kp = crate::string::js_string_from_bytes(k.as_ptr(), k.len() as u32);
        num_of(js_object_get_field_by_name(obj, kp)).unwrap_or(0.0)
    };
    Some((read(b"idle"), read(b"active")))
}

/// `performance.eventLoopUtilization(util1?, util2?)`:
///  * zero-arg → cumulative `{ idle, active, utilization }`
///  * one-arg `(u1)` → diff between the current reading and `u1`
///  * two-arg `(u1, u2)` → diff between `u1` and `u2` (#3011)
#[no_mangle]
pub extern "C" fn js_perf_event_loop_utilization(util1: f64, util2: f64) -> f64 {
    unsafe {
        let util1_defined = !JSValue::from_bits(util1.to_bits()).is_undefined();
        let util2_defined = !JSValue::from_bits(util2.to_bits()).is_undefined();

        // Two-argument form: diff of the two supplied readings (util1 - util2).
        if util1_defined && util2_defined {
            if let (Some((i1, a1)), Some((i2, a2))) =
                (elu_idle_active(util1), elu_idle_active(util2))
            {
                return make_elu_object((i1 - i2).max(0.0), (a1 - a2).max(0.0));
            }
        }

        let (idle, active) = cumulative_idle_active();
        // One-argument form: current reading minus the supplied prior reading.
        if util1_defined {
            if let Some((pidle, pactive)) = elu_idle_active(util1) {
                return make_elu_object((idle - pidle).max(0.0), (active - pactive).max(0.0));
            }
        }
        make_elu_object(idle, active)
    }
}

// ── performance.toJSON() ─────────────────────────────────────────────────────
/// A JSON snapshot of the performance object. Node returns
/// `{ nodeTiming, timeOrigin, ... }`; Perry currently surfaces `timeOrigin`
/// (a positive ms value), which is the field user code reads when serializing
/// `performance`. Forward-compatible with adding `nodeTiming` later (#1337).
#[no_mangle]
pub extern "C" fn js_perf_to_json() -> f64 {
    unsafe {
        let obj = js_object_alloc_with_shape(
            TOJSON_SHAPE,
            1,
            TOJSON_KEYS.as_ptr(),
            TOJSON_KEYS.len() as u32,
        );
        js_object_set_field(obj, 0, JSValue::number(time_origin_ms()));
        crate::value::js_nanbox_pointer(obj as i64)
    }
}

// ── performance.nodeTiming (PerformanceNodeTiming) ───────────────────────────
/// A PerformanceNodeTiming entry (entryType "node") exposing the Node bootstrap
/// milestones. Perry has no libuv bootstrap to instrument, so the milestones
/// are 0 relative to timeOrigin (loopStart reflects time since origin, loopExit
/// is -1 while the loop is running); every field is numeric, matching Node's
/// shape.
#[no_mangle]
pub extern "C" fn js_perf_node_timing() -> f64 {
    unsafe {
        let obj = js_object_alloc_with_shape(
            NODE_TIMING_SHAPE,
            11,
            NODE_TIMING_KEYS.as_ptr(),
            NODE_TIMING_KEYS.len() as u32,
        );
        js_object_set_field(obj, 0, str_value("node")); // name
        js_object_set_field(obj, 1, str_value("node")); // entryType
        js_object_set_field(obj, 2, JSValue::number(0.0)); // startTime
        js_object_set_field(obj, 3, JSValue::number(0.0)); // duration
        js_object_set_field(obj, 4, JSValue::number(0.0)); // nodeStart
        js_object_set_field(obj, 5, JSValue::number(0.0)); // v8Start
        js_object_set_field(obj, 6, JSValue::number(0.0)); // bootstrapComplete
        js_object_set_field(obj, 7, JSValue::number(0.0)); // environment
        js_object_set_field(
            obj,
            8,
            JSValue::number((perf_now() - time_origin_ms()).max(0.0)),
        ); // loopStart
        js_object_set_field(obj, 9, JSValue::number(-1.0)); // loopExit (loop running)
        js_object_set_field(obj, 10, JSValue::number(0.0)); // idleTime
        crate::value::js_nanbox_pointer(obj as i64)
    }
}

// ── clearResourceTimings() / setResourceTimingBufferSize(n) ──────────────────
// Perry has no Resource Timing buffer (no PerformanceResourceTiming entries are
// ever recorded), so these are no-ops matching Node's signatures — both return
// `undefined`. They exist so user code that manages the resource-timing buffer
// runs unchanged.
#[no_mangle]
pub extern "C" fn js_perf_clear_resource_timings() -> f64 {
    f64::from_bits(JSValue::undefined().bits())
}

#[no_mangle]
pub extern "C" fn js_perf_set_resource_timing_buffer_size(_n: f64) -> f64 {
    f64::from_bits(JSValue::undefined().bits())
}

// ── PerformanceObserver ──────────────────────────────────────────────────────
// Observers are stored in a per-thread registry; the JS-visible observer
// object is a `perf_observer`-tagged native-module namespace object whose
// field[1] holds the registry index (so `obs.observe(...)` /
// `obs.disconnect()` / `obs.takeRecords()` route through
// `dispatch_native_module_method` like any namespace method). Buffered
// entries are delivered to the callback asynchronously: a single
// `setTimeout(flush, 0)` is scheduled the first time any observer buffers an
// entry, and the flush builds a `perf_observer_list`-tagged list object and
// invokes each callback with it. This matches Node's "queued, delivered on a
// later turn" semantics closely enough for User Timing.

struct Observer {
    cb_bits: u64,
    /// NaN-boxed value of the observer's own JS object (what `new
    /// PerformanceObserver` returned). Passed as the callback's 2nd argument
    /// so `(list, observer)` satisfies `observer === obs`. The GC root scanner
    /// keeps it alive and forwards it, so identity survives evacuation.
    obj_bits: u64,
    entry_types: Vec<u8>,
    pending: Vec<PerfEntry>,
    active: bool,
}

thread_local! {
    static OBSERVERS: RefCell<Vec<Observer>> = const { RefCell::new(Vec::new()) };
    static FLUSH_SCHEDULED: Cell<bool> = const { Cell::new(false) };
    /// Entries exposed to the observer callback's `list` arg during a flush.
    static CURRENT_LIST: RefCell<Vec<PerfEntry>> = const { RefCell::new(Vec::new()) };
}

/// Build the `perf_observer` namespace object carrying the registry index.
unsafe fn make_observer_object(id: usize) -> f64 {
    let obj = crate::object::js_object_alloc(crate::object::NATIVE_MODULE_CLASS_ID, 2);
    let module = b"perf_observer";
    let mname = crate::string::js_string_from_bytes(module.as_ptr(), module.len() as u32);
    js_object_set_field(obj, 0, JSValue::string_ptr(mname));
    js_object_set_field(obj, 1, JSValue::number(id as f64));
    let mut keys = crate::array::js_array_alloc(2);
    for k in [b"__module__".as_slice(), b"__observer_id__".as_slice()] {
        let kp = crate::string::js_string_from_bytes(k.as_ptr(), k.len() as u32);
        keys = crate::array::js_array_push(keys, JSValue::string_ptr(kp));
    }
    crate::object::js_object_set_keys(obj, keys);
    crate::value::js_nanbox_pointer(obj as i64)
}

/// True if `v` is callable (matches `typeof v === "function"`) — covers
/// closures, V8 handles, and class refs uniformly.
unsafe fn is_function_value(v: f64) -> bool {
    let p = crate::builtins::js_value_typeof(v) as *const StringHeader;
    header_to_string(p) == "function"
}

/// `new PerformanceObserver(callback)` — register the observer and return its
/// namespace object. Throws a TypeError when `callback` is not a function
/// (Node: ERR_INVALID_ARG_TYPE), including the no-argument
/// `new PerformanceObserver()` form.
#[no_mangle]
pub extern "C" fn js_perf_observer_new(cb: f64) -> f64 {
    unsafe {
        if !is_function_value(cb) {
            throw_type_error("The \"callback\" argument must be of type function");
        }
        let id = OBSERVERS.with(|o| {
            let mut o = o.borrow_mut();
            o.push(Observer {
                cb_bits: cb.to_bits(),
                obj_bits: JSValue::undefined().bits(),
                entry_types: Vec::new(),
                pending: Vec::new(),
                active: false,
            });
            o.len() - 1
        });
        // Remember the returned object so the flush can hand the *same* object
        // back as the callback's 2nd arg (identity: `observer === obs`).
        let obj = make_observer_object(id);
        OBSERVERS.with(|o| o.borrow_mut()[id].obj_bits = obj.to_bits());
        obj
    }
}

fn entry_type_code(name: &str) -> Option<u8> {
    match name {
        "mark" => Some(ENTRY_TYPE_MARK),
        "measure" => Some(ENTRY_TYPE_MEASURE),
        _ => None,
    }
}

/// Read the registry index out of a `perf_observer` namespace object value's
/// field[1].
pub fn observer_id_from_value(obs_val: f64) -> usize {
    unsafe {
        match as_object_ptr(obs_val) {
            Some(obj) => {
                observer_id_from_field(crate::object::js_object_get_field(obj as *mut _, 1))
            }
            None => 0,
        }
    }
}

/// `observer.observe({ entryTypes: [...] } | { type: "..." })`. `obs_val` is the
/// `perf_observer` namespace object.
#[no_mangle]
pub extern "C" fn js_perf_observer_observe(obs_val: f64, opts: f64) -> f64 {
    unsafe {
        let id = observer_id_from_value(obs_val);
        let mut types: Vec<u8> = Vec::new();
        let mut buffered = false;

        // #3010: validate the options object the way Node does.
        let opts_jv = JSValue::from_bits(opts.to_bits());
        // `observe(null)` (and any non-object, non-undefined value) →
        // ERR_INVALID_ARG_TYPE. `observe()` / `observe(undefined)` falls
        // through to the missing-args check below.
        if !opts_jv.is_undefined() && as_object_ptr(opts).is_none() {
            throw_type_error(&format!(
                "The \"options\" argument must be of type object. Received {}",
                received_label(opts)
            ));
        }
        let opts_obj = as_object_ptr(opts);

        // Determine which of entryTypes / type were specified.
        let entry_types_v = opts_obj.map(|o| {
            let key = b"entryTypes";
            let kp = crate::string::js_string_from_bytes(key.as_ptr(), key.len() as u32);
            js_object_get_field_by_name(o, kp)
        });
        let type_v = opts_obj.map(|o| {
            let tkey = b"type";
            let tkp = crate::string::js_string_from_bytes(tkey.as_ptr(), tkey.len() as u32);
            js_object_get_field_by_name(o, tkp)
        });
        let has_entry_types = entry_types_v.map(|v| !v.is_undefined()).unwrap_or(false);
        let has_type = type_v.map(|v| !v.is_undefined()).unwrap_or(false);

        // Neither specified (incl. `observe()` / `observe({})`) → ERR_MISSING_ARGS.
        if !has_entry_types && !has_type {
            throw_type_error(
                "The \"options.entryTypes\" and \"options.type\" arguments must be specified",
            );
        }
        // Both specified → ERR_INVALID_ARG_VALUE.
        if has_entry_types && has_type {
            let et = entry_types_v.unwrap();
            throw_type_error(&format!(
                "The property 'options.entryTypes' options.entryTypes can not set with options.type together. Received {}",
                format_value_for_error(f64::from_bits(et.bits()))
            ));
        }
        // entryTypes present → must be an array (string[]).
        if has_entry_types {
            let et = entry_types_v.unwrap();
            if crate::array::js_array_is_array(f64::from_bits(et.bits())).to_bits()
                != crate::value::TAG_TRUE
            {
                throw_type_error(&format!(
                    "The \"options.entryTypes\" property must be string[]. Received {}",
                    received_label(f64::from_bits(et.bits()))
                ));
            }
        }

        if let Some(opts_obj) = opts_obj {
            // entryTypes: string[]
            let key = b"entryTypes";
            let kp = crate::string::js_string_from_bytes(key.as_ptr(), key.len() as u32);
            let arr_v = js_object_get_field_by_name(opts_obj, kp);
            if arr_v.is_pointer()
                && crate::array::js_array_is_array(f64::from_bits(arr_v.bits())).to_bits()
                    == crate::value::TAG_TRUE
            {
                let arr = arr_v.as_pointer::<crate::array::ArrayHeader>();
                let len = crate::array::js_array_length(arr);
                for i in 0..len {
                    let el = crate::array::js_array_get(arr, i);
                    if let Some(s) = string_of(el) {
                        if let Some(code) = entry_type_code(&s) {
                            types.push(code);
                        }
                    }
                }
            }
            // type: string (single-type form)
            let tkey = b"type";
            let tkp = crate::string::js_string_from_bytes(tkey.as_ptr(), tkey.len() as u32);
            let t_v = js_object_get_field_by_name(opts_obj, tkp);
            if let Some(s) = string_of(t_v) {
                if let Some(code) = entry_type_code(&s) {
                    types.push(code);
                }
            }
            // buffered: boolean — also deliver entries already on the timeline.
            let bkey = b"buffered";
            let bkp = crate::string::js_string_from_bytes(bkey.as_ptr(), bkey.len() as u32);
            let b_v = js_object_get_field_by_name(opts_obj, bkp);
            buffered = crate::value::js_is_truthy(f64::from_bits(b_v.bits())) != 0;
        }
        let observed = types.clone();
        OBSERVERS.with(|o| {
            if let Some(obs) = o.borrow_mut().get_mut(id) {
                obs.entry_types = types;
                obs.active = true;
            }
        });
        // `buffered: true` delivers entries created before observe() was
        // called. Queue the matching timeline entries and arm the async flush
        // so the callback fires on a later turn (Node's buffered semantics).
        if buffered {
            let pre: Vec<PerfEntry> = PERF_ENTRIES.with(|store| {
                store
                    .borrow()
                    .iter()
                    .filter(|e| observed.contains(&e.entry_type))
                    .cloned()
                    .collect()
            });
            if !pre.is_empty() {
                OBSERVERS.with(|o| {
                    if let Some(obs) = o.borrow_mut().get_mut(id) {
                        obs.pending.extend(pre);
                    }
                });
                schedule_flush();
            }
        }
        f64::from_bits(JSValue::undefined().bits())
    }
}

/// `observer.disconnect()`.
#[no_mangle]
pub extern "C" fn js_perf_observer_disconnect(obs_val: f64) -> f64 {
    let id = observer_id_from_value(obs_val);
    OBSERVERS.with(|o| {
        if let Some(obs) = o.borrow_mut().get_mut(id) {
            obs.active = false;
            obs.pending.clear();
        }
    });
    f64::from_bits(JSValue::undefined().bits())
}

/// `observer.takeRecords()` — drain + return the observer's buffered entries.
#[no_mangle]
pub extern "C" fn js_perf_observer_take_records(obs_val: f64) -> f64 {
    unsafe {
        let id = observer_id_from_value(obs_val);
        let entries: Vec<PerfEntry> = OBSERVERS.with(|o| {
            o.borrow_mut()
                .get_mut(id)
                .map(|obs| std::mem::take(&mut obs.pending))
                .unwrap_or_default()
        });
        let mut arr = crate::array::js_array_alloc(entries.len() as u32);
        for e in &entries {
            let obj = entry_to_object(e);
            arr = crate::array::js_array_push(arr, JSValue::from_bits(obj.to_bits()));
        }
        crate::value::js_nanbox_pointer(arr as i64)
    }
}

/// Read the registry index out of a `perf_observer` namespace object's field[1].
pub fn observer_id_from_field(v: JSValue) -> usize {
    num_of(v).map(|n| n as usize).unwrap_or(0)
}

/// Buffer an entry into every active observer that subscribes to its type and
/// arm a single async flush.
fn notify_observers(entry: &PerfEntry) {
    let mut any = false;
    OBSERVERS.with(|o| {
        for obs in o.borrow_mut().iter_mut() {
            if obs.active && obs.entry_types.contains(&entry.entry_type) {
                obs.pending.push(entry.clone());
                any = true;
            }
        }
    });
    if any {
        schedule_flush();
    }
}

fn schedule_flush() {
    if FLUSH_SCHEDULED.with(|f| f.get()) {
        return;
    }
    FLUSH_SCHEDULED.with(|f| f.set(true));
    unsafe {
        let closure =
            crate::closure::js_closure_alloc_singleton(js_perf_observer_flush_all as *const u8);
        crate::timer::js_set_timeout_callback(closure as i64, 0.0);
    }
}

/// Timer callback: deliver each observer's buffered entries via its callback.
#[no_mangle]
pub extern "C" fn js_perf_observer_flush_all(
    _closure: *const crate::closure::ClosureHeader,
) -> f64 {
    FLUSH_SCHEDULED.with(|f| f.set(false));
    let work: Vec<(u64, u64, Vec<PerfEntry>)> = OBSERVERS.with(|o| {
        o.borrow_mut()
            .iter_mut()
            .filter(|obs| obs.active && !obs.pending.is_empty())
            .map(|obs| (obs.cb_bits, obs.obj_bits, std::mem::take(&mut obs.pending)))
            .collect()
    });
    for (cb_bits, obj_bits, entries) in work {
        unsafe {
            CURRENT_LIST.with(|c| *c.borrow_mut() = entries);
            let module = b"perf_observer_list";
            let list =
                crate::object::js_create_native_module_namespace(module.as_ptr(), module.len());
            let cb_jv = JSValue::from_bits(cb_bits);
            if cb_jv.is_pointer() {
                let cb_closure = cb_jv.as_pointer::<crate::closure::ClosureHeader>();
                // Node invokes the callback as `(list, observer)`.
                crate::closure::js_closure_call2(cb_closure, list, f64::from_bits(obj_bits));
            }
            CURRENT_LIST.with(|c| c.borrow_mut().clear());
        }
    }
    f64::from_bits(JSValue::undefined().bits())
}

/// Build an array from the in-flight observer `list` entries (for the
/// `perf_observer_list` namespace methods).
pub unsafe fn current_list_to_array(filter: impl Fn(&PerfEntry) -> bool) -> f64 {
    let mut snapshot: Vec<PerfEntry> =
        CURRENT_LIST.with(|c| c.borrow().iter().filter(|e| filter(e)).cloned().collect());
    sort_entries_by_start_time(&mut snapshot);
    let mut arr = crate::array::js_array_alloc(snapshot.len() as u32);
    for e in &snapshot {
        let obj = entry_to_object(e);
        arr = crate::array::js_array_push(arr, JSValue::from_bits(obj.to_bits()));
    }
    crate::value::js_nanbox_pointer(arr as i64)
}

pub unsafe fn current_list_get_entries() -> f64 {
    current_list_to_array(|_| true)
}

pub unsafe fn current_list_get_by_type(type_val: f64) -> f64 {
    let want = coerce_to_string(type_val);
    match entry_type_code(&want) {
        Some(code) => current_list_to_array(move |e| e.entry_type == code),
        None => current_list_to_array(|_| false),
    }
}

pub unsafe fn current_list_get_by_name(name_val: f64) -> f64 {
    let want = coerce_to_string(name_val);
    current_list_to_array(move |e| e.name == want)
}

/// Build the `PerformanceObserver.supportedEntryTypes` array.
#[no_mangle]
pub extern "C" fn js_perf_supported_entry_types() -> f64 {
    unsafe {
        let mut arr = crate::array::js_array_alloc(2);
        for t in ["mark", "measure"] {
            arr = crate::array::js_array_push(arr, str_value(t));
        }
        crate::value::js_nanbox_pointer(arr as i64)
    }
}

// ── GC root scanner ──────────────────────────────────────────────────────────
/// Keep `detail` JSValues stored in the timeline + observer buffers, and the
/// observer callbacks, alive across GC.
pub fn scan_perf_entries_roots_mut(visitor: &mut crate::gc::RuntimeRootVisitor<'_>) {
    PERF_ENTRIES.with(|store| {
        for e in store.borrow_mut().iter_mut() {
            visitor.visit_nanbox_u64_slot(&mut e.detail_bits);
        }
    });
    OBSERVERS.with(|o| {
        for obs in o.borrow_mut().iter_mut() {
            visitor.visit_nanbox_u64_slot(&mut obs.cb_bits);
            visitor.visit_nanbox_u64_slot(&mut obs.obj_bits);
            for e in obs.pending.iter_mut() {
                visitor.visit_nanbox_u64_slot(&mut e.detail_bits);
            }
        }
    });
    CURRENT_LIST.with(|c| {
        for e in c.borrow_mut().iter_mut() {
            visitor.visit_nanbox_u64_slot(&mut e.detail_bits);
        }
    });
    // Keep the cached `performance` namespace alive + forwarded so the
    // singleton identity (named import === globalThis.performance) survives GC.
    PERFORMANCE_NS.with(|c| {
        let mut bits = c.get();
        if bits != 0 {
            visitor.visit_nanbox_u64_slot(&mut bits);
            c.set(bits);
        }
    });
}

// ── Histograms (perf_histogram namespace) ────────────────────────────────────
// `monitorEventLoopDelay()` returns an IntervalHistogram and
// `createHistogram()` returns a RecordableHistogram. Perry doesn't actually
// sample event-loop delay or record user-supplied values yet — every stat
// reads as 0, and enable/disable/reset/record/recordDelta/add are no-ops.
// The shape is enough to satisfy feature-detection (`typeof h.record ===
// "function"`, `typeof h.mean === "number"`) and the trivial-call paths
// that user code drives through these histograms. Issue #1336.

/// Build a `perf_histogram`-tagged namespace object. Distinguishing
/// IntervalHistogram vs RecordableHistogram is unnecessary for the stub
/// surface — every method/property is shared and trivial — so the same
/// shape covers both. The receiver-less property reads route through
/// `is_native_module_callable_export` (methods) and
/// `get_native_module_constant` (numeric accessors).
unsafe fn make_histogram_object() -> f64 {
    let obj = crate::object::js_object_alloc(crate::object::NATIVE_MODULE_CLASS_ID, 1);
    let module = b"perf_histogram";
    let mname = crate::string::js_string_from_bytes(module.as_ptr(), module.len() as u32);
    js_object_set_field(obj, 0, JSValue::string_ptr(mname));
    let mut keys = crate::array::js_array_alloc(1);
    let kp = crate::string::js_string_from_bytes(b"__module__".as_ptr(), 10);
    keys = crate::array::js_array_push(keys, JSValue::string_ptr(kp));
    crate::object::js_object_set_keys(obj, keys);
    crate::value::js_nanbox_pointer(obj as i64)
}

/// `perf_hooks.monitorEventLoopDelay(options?)` — returns an IntervalHistogram.
#[no_mangle]
pub extern "C" fn js_perf_monitor_event_loop_delay(_options: f64) -> f64 {
    unsafe { make_histogram_object() }
}

/// `perf_hooks.createHistogram(options?)` — returns a RecordableHistogram.
#[no_mangle]
pub extern "C" fn js_perf_create_histogram(_options: f64) -> f64 {
    unsafe { make_histogram_object() }
}

/// `histogram.enable()` / `.disable()` / `.reset()` / `.record(n)` /
/// `.recordDelta()` / `.add(other)` — no-ops on the stub. Returns
/// `undefined` per Node's signature for the void-returning methods;
/// `.enable()` actually returns `true` in Node (was it running before?),
/// but `undefined` is what the unobserved-stub case warrants.
#[no_mangle]
pub extern "C" fn js_perf_histogram_noop() -> f64 {
    f64::from_bits(JSValue::undefined().bits())
}

/// `histogram.percentile(p)` — returns 0 (no recorded samples).
#[no_mangle]
pub extern "C" fn js_perf_histogram_percentile(_p: f64) -> f64 {
    0.0
}

#[cfg(test)]
mod sso_tests_1781 {
    use super::*;

    /// #1781: perf entry-type/name strings are frequently <= 5 bytes — the
    /// literal `"mark"` (4 bytes) and observer `entryTypes: ["mark"]` are
    /// inline SSO values. `is_string()` (STRING_TAG-only) missed them, so
    /// mark/measure resolution, type filters and observer registration all
    /// silently dropped short names. `string_of` is the shared SSO-aware
    /// decoder every one of those sites now routes through.
    #[test]
    fn string_of_decodes_sso_and_heap_strings() {
        unsafe {
            let sso = JSValue::try_short_string(b"mark").unwrap();
            assert!(sso.is_short_string());
            assert_eq!(string_of(sso).as_deref(), Some("mark"));

            let heap =
                JSValue::string_ptr(crate::string::js_string_from_bytes(b"measure".as_ptr(), 7));
            assert_eq!(string_of(heap).as_deref(), Some("measure"));

            // non-strings (undefined / number) return None.
            assert_eq!(
                string_of(JSValue::from_bits(crate::value::TAG_UNDEFINED)),
                None
            );
            assert_eq!(string_of(JSValue::from_bits(3.0f64.to_bits())), None);
        }
    }

    /// End-to-end: `getEntriesByName(name, "mark")` with the SSO literal
    /// `"mark"` must still filter to the mark entry (site #509).
    #[test]
    fn get_entries_by_name_filters_on_sso_type() {
        unsafe {
            let undef = f64::from_bits(crate::value::TAG_UNDEFINED);
            let name =
                JSValue::string_ptr(crate::string::js_string_from_bytes(b"phase".as_ptr(), 5));
            let name_f = f64::from_bits(name.bits());
            js_perf_mark(name_f, undef);

            // "mark" (4 bytes) is an inline SSO type filter.
            let ty = JSValue::try_short_string(b"mark").unwrap();
            assert!(ty.is_short_string());
            let arr = js_perf_get_entries_by_name(name_f, f64::from_bits(ty.bits()));
            let arr_ptr =
                crate::value::js_nanbox_get_pointer(arr) as *const crate::array::ArrayHeader;
            assert!(!arr_ptr.is_null());
            assert_eq!(
                crate::array::js_array_length(arr_ptr),
                1,
                "SSO type filter 'mark' should match the mark entry"
            );
        }
    }
}
