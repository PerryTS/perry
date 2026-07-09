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
    js_object_set_field, js_object_set_field_by_name,
};
use crate::string::StringHeader;
use crate::value::JSValue;
use std::cell::{Cell, RefCell};
use std::sync::{Once, OnceLock};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

const ENTRY_TYPE_MARK: u8 = 0;
const ENTRY_TYPE_MEASURE: u8 = 1;
const ENTRY_TYPE_RESOURCE: u8 = 2;
const ENTRY_TYPE_FUNCTION: u8 = 3;

pub(crate) const CLASS_ID_PERFORMANCE_ENTRY: u32 = 0xFFFF_0080;
pub(crate) const CLASS_ID_PERFORMANCE_MARK: u32 = 0xFFFF_0081;
pub(crate) const CLASS_ID_PERFORMANCE_MEASURE: u32 = 0xFFFF_0082;
// #3871: these three IDs previously collided with node:fs's CLASS_ID_FS_DIR
// (0x86), CLASS_ID_FS_DIRENT (0x87), and CLASS_ID_FS_READ_STREAM (0x88), so
// `performance instanceof Performance` / `list instanceof
// PerformanceObserverEntryList` were intercepted by the fs Dir/Dirent/
// ReadStream `instanceof` arms in `object/instanceof.rs` (which run before the
// perf-hooks shape check) and returned false. Moved to free IDs past fs's range
// (fs ends at 0x8C). Keep in sync with the literals in
// `perry-codegen/src/expr/instance_misc1.rs`.
pub(crate) const CLASS_ID_PERFORMANCE_RESOURCE_TIMING: u32 = 0xFFFF_008D;
pub(crate) const CLASS_ID_PERFORMANCE: u32 = 0xFFFF_008E;
pub(crate) const CLASS_ID_PERFORMANCE_OBSERVER_ENTRY_LIST: u32 = 0xFFFF_008F;

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

/// Shape id for the `{ eventLoopUtilization, nodeTiming, timeOrigin }`
/// snapshot returned by `performance.toJSON()`.
const TOJSON_SHAPE: u32 = 0x7FFF_FF50;
const TOJSON_KEYS: &[u8] = b"eventLoopUtilization\0nodeTiming\0timeOrigin\0";

/// Shape id + keys for `performance.nodeTiming` (PerformanceNodeTiming entry).
const NODE_TIMING_SHAPE: u32 = 0x7FFF_FF43;
const NODE_TIMING_KEYS: &[u8] = b"name\0entryType\0startTime\0duration\0nodeStart\0v8Start\0bootstrapComplete\0environment\0loopStart\0loopExit\0idleTime\0uvMetricsInfo\0";

/// Shape id + keys for `performance.nodeTiming.uvMetricsInfo`.
const UV_METRICS_INFO_SHAPE: u32 = 0x7FFF_FF51;
const UV_METRICS_INFO_KEYS: &[u8] = b"loopCount\0events\0eventsWaiting\0";

#[derive(Clone)]
struct PerfEntry {
    name: String,
    entry_type: u8,
    start_time: f64,
    duration: f64,
    /// NaN-boxed JSValue bits of the entry's `detail` (defaults to `null`).
    detail_bits: u64,
    /// Stable materialized entry object returned by both the creation API and
    /// later timeline queries for that entry.
    object_bits: u64,
    initiator_type: Option<String>,
}

static TIMERIFY_WRAPPER_REGISTERED: Once = Once::new();

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
        "resource" => Some(ENTRY_TYPE_RESOURCE),
        "function" => Some(ENTRY_TYPE_FUNCTION),
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
        CLASS_ID_PERFORMANCE_RESOURCE_TIMING => Some(ENTRY_TYPE_RESOURCE),
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

pub(crate) fn is_performance_object_value(value: f64) -> bool {
    let bits = value.to_bits();
    // Fast path: the exact cached singleton pointer.
    if PERFORMANCE_NS.with(|c| {
        let cached = c.get();
        cached != 0 && cached == bits
    }) {
        return true;
    }
    // #3871: `performance` (global + `node:perf_hooks` import) resolves to the
    // `perf_hooks` native-module namespace object via
    // `js_create_native_module_namespace`, whose pointer may differ from the
    // `PERFORMANCE_NS` cell (the cell is only set by `performance_namespace()`).
    // Recognize any `perf_hooks` namespace object by its stored module name so
    // `performance instanceof Performance` holds — mirrors the field-0 name
    // check used for the observer entry list below.
    unsafe {
        if let Some(obj) = as_object_ptr(value) {
            let module = js_object_get_field(obj, 0);
            if string_of(module).as_deref() == Some("perf_hooks") {
                return true;
            }
        }
    }
    false
}

pub(crate) fn is_perf_observer_list_value(value: f64) -> bool {
    unsafe {
        let Some(obj) = as_object_ptr(value) else {
            return false;
        };
        let module = js_object_get_field(obj, 0);
        string_of(module).as_deref() == Some("perf_observer_list")
    }
}

pub(crate) fn is_perf_hooks_shape_instance_of(value: f64, class_id: u32) -> Option<bool> {
    match class_id {
        CLASS_ID_PERFORMANCE => Some(is_performance_object_value(value)),
        CLASS_ID_PERFORMANCE_OBSERVER_ENTRY_LIST => Some(is_perf_observer_list_value(value)),
        _ => None,
    }
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

struct PerfClock {
    monotonic_start: Instant,
    time_origin_ms: f64,
}

/// Shared clock for `performance.timeOrigin` and `performance.now()`.
///
/// `init_time_origin()` is called from runtime initialization so user code
/// observes a process-start origin. The `OnceLock` fallback keeps direct unit
/// tests and unusual embedder paths well-defined.
static PERF_CLOCK: OnceLock<PerfClock> = OnceLock::new();

fn wall_clock_ms() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs_f64() * 1000.0)
        .unwrap_or(0.0)
}

fn perf_clock() -> &'static PerfClock {
    PERF_CLOCK.get_or_init(|| PerfClock {
        monotonic_start: Instant::now(),
        time_origin_ms: wall_clock_ms(),
    })
}

pub(crate) fn init_time_origin() {
    let _ = perf_clock();
}

pub(crate) fn time_origin_ms() -> f64 {
    perf_clock().time_origin_ms
}

pub(crate) fn performance_now_ms() -> f64 {
    perf_clock().monotonic_start.elapsed().as_secs_f64() * 1000.0
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

fn throw_type_error_with_code(msg: &str, code: &'static str) -> ! {
    crate::fs::validate::throw_type_error_with_code(msg, code)
}

fn throw_syntax_error_with_code(msg: &str, code: &'static str) -> ! {
    let msg_str = crate::string::js_string_from_bytes(msg.as_ptr(), msg.len() as u32);
    crate::node_submodules::register_error_code_pub(msg_str, code);
    let err_ptr = crate::error::js_syntaxerror_new(msg_str);
    let err_value = JSValue::pointer(err_ptr as *const u8).bits();
    crate::exception::js_throw(f64::from_bits(err_value))
}

fn validate_user_timing_timestamp(value: f64) {
    if value < 0.0 || !value.is_finite() {
        throw_type_error_with_code(
            &format!("{value} is not a valid timestamp"),
            "ERR_PERFORMANCE_INVALID_TIMESTAMP",
        );
    }
}

/// Build a NaN-boxed string value from a Rust `&str`.
fn str_value(s: &str) -> JSValue {
    let ptr = crate::string::js_string_from_bytes(s.as_ptr(), s.len() as u32);
    JSValue::string_ptr(ptr)
}

unsafe fn set_named_field(obj: *mut crate::object::ObjectHeader, name: &str, value: JSValue) {
    let key = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
    js_object_set_field_by_name(obj, key, f64::from_bits(value.bits()));
}

fn entry_type_name(entry_type: u8) -> &'static str {
    match entry_type {
        ENTRY_TYPE_MEASURE => "measure",
        ENTRY_TYPE_RESOURCE => "resource",
        ENTRY_TYPE_FUNCTION => "function",
        _ => "mark",
    }
}

/// Materialize a `PerfEntry` into a `{ name, entryType, startTime, duration,
/// detail }` JS object and return its NaN-boxed pointer bits.
unsafe fn entry_to_object(e: &PerfEntry) -> f64 {
    if e.object_bits != 0 {
        return f64::from_bits(e.object_bits);
    }
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
    js_object_set_field(obj, 0, str_value(&e.name));
    js_object_set_field(obj, 1, str_value(entry_type_name(e.entry_type)));
    js_object_set_field(obj, 2, JSValue::number(e.start_time));
    js_object_set_field(obj, 3, JSValue::number(e.duration));
    js_object_set_field(obj, 4, JSValue::from_bits(e.detail_bits));
    if let Some(initiator_type) = &e.initiator_type {
        set_named_field(obj, "initiatorType", str_value(initiator_type));
    }
    crate::value::js_nanbox_pointer(obj as i64)
}

/// `performance.now()` reading used for default mark startTimes / measure
/// endpoints: monotonic milliseconds since `performance.timeOrigin`.
fn perf_now() -> f64 {
    performance_now_ms()
}

unsafe fn option_value(options_obj: *const crate::object::ObjectHeader, key: &str) -> JSValue {
    let key_ptr = crate::string::js_string_from_bytes(key.as_ptr(), key.len() as u32);
    js_object_get_field_by_name(options_obj, key_ptr)
}

/// Read an option field that may be a non-negative timestamp or a mark-name
/// string and resolve it to a timeline value. Returns `None` when absent.
unsafe fn resolve_option_endpoint(
    options_obj: *const crate::object::ObjectHeader,
    key: &str,
) -> Option<f64> {
    let v = option_value(options_obj, key);
    if v.is_undefined() {
        return None;
    }
    Some(resolve_endpoint_value(v))
}

unsafe fn resolve_endpoint_value(v: JSValue) -> f64 {
    if let Some(n) = num_of(v) {
        validate_user_timing_timestamp(n);
        n
    } else if let Some(name) = string_of(v) {
        match lookup_mark_start(&name) {
            Some(t) => t,
            None => throw_syntax_error_with_code(
                &format!("The \"{name}\" performance mark has not been set"),
                "12",
            ),
        }
    } else {
        throw_type_error_with_code(
            "The User Timing endpoint must be a number or a performance mark name",
            "ERR_INVALID_ARG_TYPE",
        )
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
            None => throw_type_error(&format!("The \"{name}\" performance mark has not been set")),
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
    num_of(option_value(options_obj, key))
}

unsafe fn option_present(options_obj: *const crate::object::ObjectHeader, key: &str) -> bool {
    !option_value(options_obj, key).is_undefined()
}

unsafe fn option_detail_bits(options_obj: *const crate::object::ObjectHeader) -> u64 {
    let v = option_value(options_obj, "detail");
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
    if !jv.is_pointer() {
        return None;
    }
    let ptr = jv.as_pointer::<u8>();
    if ptr.is_null() || (ptr as usize) < crate::gc::GC_HEADER_SIZE + 0x1000 {
        return None;
    }
    unsafe {
        let header = &*(ptr.sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader);
        if header.obj_type != crate::gc::GC_TYPE_OBJECT {
            return None;
        }
    }
    Some(ptr as *const crate::object::ObjectHeader)
}

fn is_array_value(v: JSValue) -> bool {
    if !v.is_pointer() {
        return false;
    }
    let ptr = v.as_pointer::<u8>();
    if ptr.is_null() || (ptr as usize) < crate::gc::GC_HEADER_SIZE + 0x1000 {
        return false;
    }
    unsafe {
        let header = &*(ptr.sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader);
        header.obj_type == crate::gc::GC_TYPE_ARRAY
    }
}

fn array_ptr_from_value(v: JSValue) -> *const crate::array::ArrayHeader {
    v.as_pointer::<crate::array::ArrayHeader>()
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
                match option_number(opts, "startTime") {
                    Some(st) => {
                        validate_user_timing_timestamp(st);
                        start_time = st;
                    }
                    None => throw_type_error_with_code(
                        "The \"startTime\" option must be of type number",
                        "ERR_INVALID_ARG_TYPE",
                    ),
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
            object_bits: 0,
            initiator_type: None,
        };
        let mut entry = entry;
        let obj = entry_to_object(&entry);
        entry.object_bits = obj.to_bits();
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
        let name_jv = JSValue::from_bits(name_val.to_bits());
        let Some(name) = string_of(name_jv) else {
            throw_type_error_with_code(
                "The \"name\" argument must be of type string",
                "ERR_INVALID_ARG_TYPE",
            );
        };
        let arg2_jv = JSValue::from_bits(arg2.to_bits());

        let (start_time, duration);
        if let Some(opts) = as_object_ptr(arg2) {
            // Options form: { start?, end?, duration?, detail? }
            let start_present = option_present(opts, "start");
            let end_present = option_present(opts, "end");
            let duration_present = option_present(opts, "duration");
            if start_present && end_present && duration_present {
                throw_type_error_with_code(
                    "Must not have options.start, options.end, and options.duration specified",
                    "ERR_PERFORMANCE_MEASURE_INVALID_OPTIONS",
                );
            }
            let dur = if duration_present {
                match option_number(opts, "duration") {
                    Some(d) => {
                        validate_user_timing_timestamp(d);
                        Some(d)
                    }
                    None => throw_type_error_with_code(
                        "The \"duration\" option must be of type number",
                        "ERR_INVALID_ARG_TYPE",
                    ),
                }
            } else {
                None
            };

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
        object_bits: 0,
        initiator_type: None,
    };
    let mut entry = entry;
    let obj = entry_to_object(&entry);
    entry.object_bits = obj.to_bits();
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
        match entry_type_code(&want) {
            Some(want_type) => entries_to_array(move |e| e.entry_type == want_type),
            None => entries_to_array(|_| false),
        }
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
                "resource" => Some(ENTRY_TYPE_RESOURCE),
                "function" => Some(ENTRY_TYPE_FUNCTION),
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
// idle/active split anchored to performance.timeOrigin. The result keeps
// Node's object shape and the diff forms' utilization in [0, 1].
fn cumulative_idle_active() -> (f64, f64) {
    let elapsed = perf_now().max(0.0);
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

#[no_mangle]
pub extern "C" fn js_perf_event_loop_utilization(util1: f64, util2: f64) -> f64 {
    unsafe {
        let (idle, active) = cumulative_idle_active();
        if let Some((u1_idle, u1_active)) = read_elu_idle_active(util1) {
            if let Some((u2_idle, u2_active)) = read_elu_idle_active(util2) {
                return make_elu_object(
                    (u1_idle - u2_idle).max(0.0),
                    (u1_active - u2_active).max(0.0),
                );
            }
            return make_elu_object((idle - u1_idle).max(0.0), (active - u1_active).max(0.0));
        }
        make_elu_object(idle, active)
    }
}

unsafe fn read_elu_idle_active(value: f64) -> Option<(f64, f64)> {
    let obj = as_object_ptr(value)?;
    let field = |name: &[u8]| -> f64 {
        let key = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
        num_of(js_object_get_field_by_name(obj, key)).unwrap_or(0.0)
    };
    Some((field(b"idle"), field(b"active")))
}

// ── performance.toJSON() ─────────────────────────────────────────────────────
/// A JSON snapshot of the performance object. Node returns
/// `{ eventLoopUtilization, nodeTiming, timeOrigin }`; Perry keeps the same
/// property names and numeric subfield types while using deterministic fallback
/// values for libuv-specific counters.
#[no_mangle]
pub extern "C" fn js_perf_to_json() -> f64 {
    unsafe {
        let scope = crate::gc::RuntimeHandleScope::new();
        let (idle, active) = cumulative_idle_active();
        let elu = make_elu_object(idle, active);
        let elu_handle = scope.root_nanbox_f64(elu);
        let node_timing = js_perf_node_timing();
        let node_timing_handle = scope.root_nanbox_f64(node_timing);
        let obj = js_object_alloc_with_shape(
            TOJSON_SHAPE,
            3,
            TOJSON_KEYS.as_ptr(),
            TOJSON_KEYS.len() as u32,
        );
        js_object_set_field(obj, 0, JSValue::from_bits(elu_handle.get_nanbox_u64()));
        js_object_set_field(
            obj,
            1,
            JSValue::from_bits(node_timing_handle.get_nanbox_u64()),
        );
        js_object_set_field(obj, 2, JSValue::number(time_origin_ms()));
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
        let scope = crate::gc::RuntimeHandleScope::new();
        let node_name = str_value("node");
        let node_name_handle = scope.root_nanbox_u64(node_name.bits());
        let uv_metrics = make_uv_metrics_info_object();
        let uv_metrics_handle = scope.root_nanbox_f64(uv_metrics);
        let obj = js_object_alloc_with_shape(
            NODE_TIMING_SHAPE,
            12,
            NODE_TIMING_KEYS.as_ptr(),
            NODE_TIMING_KEYS.len() as u32,
        );
        let node_name = JSValue::from_bits(node_name_handle.get_nanbox_u64());
        js_object_set_field(obj, 0, node_name); // name
        js_object_set_field(obj, 1, node_name); // entryType
        js_object_set_field(obj, 2, JSValue::number(0.0)); // startTime
        js_object_set_field(obj, 3, JSValue::number(0.0)); // duration
        js_object_set_field(obj, 4, JSValue::number(0.0)); // nodeStart
        js_object_set_field(obj, 5, JSValue::number(0.0)); // v8Start
        js_object_set_field(obj, 6, JSValue::number(0.0)); // bootstrapComplete
        js_object_set_field(obj, 7, JSValue::number(0.0)); // environment
        js_object_set_field(obj, 8, JSValue::number(perf_now().max(0.0))); // loopStart
        js_object_set_field(obj, 9, JSValue::number(-1.0)); // loopExit (loop running)
        js_object_set_field(obj, 10, JSValue::number(0.0)); // idleTime
        js_object_set_field(
            obj,
            11,
            JSValue::from_bits(uv_metrics_handle.get_nanbox_u64()),
        );
        crate::value::js_nanbox_pointer(obj as i64)
    }
}

unsafe fn make_uv_metrics_info_object() -> f64 {
    let obj = js_object_alloc_with_shape(
        UV_METRICS_INFO_SHAPE,
        3,
        UV_METRICS_INFO_KEYS.as_ptr(),
        UV_METRICS_INFO_KEYS.len() as u32,
    );
    // Perry does not expose libuv counters; retain Node's property names and
    // numeric field types with deterministic zero counters.
    js_object_set_field(obj, 0, JSValue::number(0.0)); // loopCount
    js_object_set_field(obj, 1, JSValue::number(0.0)); // events
    js_object_set_field(obj, 2, JSValue::number(0.0)); // eventsWaiting
    crate::value::js_nanbox_pointer(obj as i64)
}

// ── clearResourceTimings() / setResourceTimingBufferSize(n) ──────────────────
#[no_mangle]
pub extern "C" fn js_perf_clear_resource_timings() -> f64 {
    PERF_ENTRIES.with(|store| {
        store
            .borrow_mut()
            .retain(|entry| entry.entry_type != ENTRY_TYPE_RESOURCE);
    });
    f64::from_bits(JSValue::undefined().bits())
}

/// Node default for the resource-timing buffer size
/// (`performance.setResourceTimingBufferSize` unset).
const RESOURCE_TIMING_BUFFER_DEFAULT: usize = 250;

thread_local! {
    /// 2026-07-09 GC audit wave 2: the setter used to be a no-op and the
    /// timeline had NO cap, so per-request `markResourceTiming` leaked
    /// entries (plus their materialized entry objects, GC-rooted via
    /// `object_bits`) forever. Node caps 'resource' entries at 250 by
    /// default and drops new ones when the buffer is full.
    static RESOURCE_TIMING_BUFFER_SIZE: Cell<usize> =
        const { Cell::new(RESOURCE_TIMING_BUFFER_DEFAULT) };
}

#[no_mangle]
pub extern "C" fn js_perf_set_resource_timing_buffer_size(n: f64) -> f64 {
    // WebIDL unsigned long conversion, saturating at 0 for junk input.
    let size = if n.is_finite() && n > 0.0 {
        n.floor().min(u32::MAX as f64) as usize
    } else {
        0
    };
    RESOURCE_TIMING_BUFFER_SIZE.with(|cell| cell.set(size));
    f64::from_bits(JSValue::undefined().bits())
}

/// True when another 'resource' entry fits in the timeline buffer.
fn resource_timing_buffer_has_room() -> bool {
    let cap = RESOURCE_TIMING_BUFFER_SIZE.with(|cell| cell.get());
    PERF_ENTRIES.with(|store| {
        store
            .borrow()
            .iter()
            .filter(|entry| entry.entry_type == ENTRY_TYPE_RESOURCE)
            .count()
            < cap
    })
}

#[no_mangle]
pub extern "C" fn js_perf_mark_resource_timing(
    timing_info: f64,
    requested_url: f64,
    initiator_type: f64,
    _global: f64,
    _cache_mode: f64,
    _body_info: f64,
    _response_status: f64,
    _delivery_type: f64,
) -> f64 {
    unsafe {
        let Some(timing_obj) = as_object_ptr(timing_info) else {
            throw_type_error_with_code(
                "The \"timingInfo\" argument must be of type object",
                "ERR_INVALID_ARG_TYPE",
            );
        };
        let name = coerce_to_string(requested_url);
        let initiator = coerce_to_string(initiator_type);
        let start_time = option_number(timing_obj, "startTime")
            .or_else(|| option_number(timing_obj, "fetchStart"))
            .unwrap_or(0.0);
        let entry = PerfEntry {
            name,
            entry_type: ENTRY_TYPE_RESOURCE,
            start_time,
            duration: f64::NAN,
            detail_bits: JSValue::null().bits(),
            object_bits: 0,
            initiator_type: Some(initiator),
        };
        let mut entry = entry;
        let obj = entry_to_object(&entry);
        entry.object_bits = obj.to_bits();
        notify_observers(&entry);
        // Timeline insertion honors the resource-timing buffer cap (observers
        // above still see the entry, matching Node: a full buffer only stops
        // timeline accumulation, not observer delivery).
        if resource_timing_buffer_has_room() {
            PERF_ENTRIES.with(|store| store.borrow_mut().push(entry));
        }
        obj
    }
}

unsafe fn collect_rest_args(rest: f64) -> Vec<f64> {
    let ptr = crate::value::js_nanbox_get_pointer(rest) as *const crate::array::ArrayHeader;
    if ptr.is_null() {
        return Vec::new();
    }
    let len = crate::array::js_array_length(ptr) as usize;
    let mut args = Vec::with_capacity(len);
    for i in 0..len {
        args.push(crate::array::js_array_get_f64(ptr, i as u32));
    }
    args
}

unsafe fn closure_ptr_from_value(value: f64) -> Option<*const crate::closure::ClosureHeader> {
    let jv = JSValue::from_bits(value.to_bits());
    if !jv.is_pointer() {
        return None;
    }
    let ptr = jv.as_pointer::<crate::closure::ClosureHeader>();
    if ptr.is_null() || (*ptr).type_tag != crate::closure::CLOSURE_MAGIC {
        return None;
    }
    Some(ptr)
}

unsafe fn function_value_name(value: f64) -> String {
    let Some(closure) = closure_ptr_from_value(value) else {
        return String::new();
    };
    crate::builtins::function_name_for_ptr((*closure).func_ptr as usize)
        .or_else(|| {
            let name_value = crate::closure::closure_get_dynamic_prop(closure as usize, "name");
            string_of(JSValue::from_bits(name_value.to_bits()))
        })
        .unwrap_or_default()
}

extern "C" fn perf_timerify_wrapper(
    closure: *const crate::closure::ClosureHeader,
    rest: f64,
) -> f64 {
    unsafe {
        let target = crate::closure::js_closure_get_capture_f64(closure, 0);
        let name_value = crate::closure::js_closure_get_capture_f64(closure, 1);
        let name = string_of(JSValue::from_bits(name_value.to_bits())).unwrap_or_default();
        let args = collect_rest_args(rest);
        let start_time = perf_now();
        let result = crate::closure::js_native_call_value(target, args.as_ptr(), args.len());
        let duration = (perf_now() - start_time).max(0.0);
        let entry = PerfEntry {
            name,
            entry_type: ENTRY_TYPE_FUNCTION,
            start_time,
            duration,
            detail_bits: JSValue::null().bits(),
            object_bits: 0,
            initiator_type: None,
        };
        let mut entry = entry;
        let obj = entry_to_object(&entry);
        entry.object_bits = obj.to_bits();
        notify_observers(&entry);
        result
    }
}

#[no_mangle]
pub extern "C" fn js_perf_timerify(fn_value: f64, _options: f64) -> f64 {
    unsafe {
        if !is_function_value(fn_value) {
            throw_type_error_with_code(
                "The \"fn\" argument must be of type function",
                "ERR_INVALID_ARG_TYPE",
            );
        }
        TIMERIFY_WRAPPER_REGISTERED.call_once(|| {
            crate::closure::js_register_closure_rest(perf_timerify_wrapper as *const u8, 0);
        });
        let name = function_value_name(fn_value);
        let closure = crate::closure::js_closure_alloc(perf_timerify_wrapper as *const u8, 2);
        crate::closure::js_closure_set_capture_f64(closure, 0, fn_value);
        let name_value = str_value(&name);
        crate::closure::js_closure_set_capture_f64(closure, 1, f64::from_bits(name_value.bits()));

        if let Some(target) = closure_ptr_from_value(fn_value) {
            if let Some(length) = crate::closure::closure_length(target) {
                crate::object::set_builtin_closure_length(closure as usize, length);
            }
        }

        let wrapper_name = if name.is_empty() {
            "timerified".to_string()
        } else {
            format!("timerified {name}")
        };
        let wrapper_name_value = str_value(&wrapper_name);
        crate::closure::closure_set_dynamic_prop(
            closure as usize,
            "name",
            f64::from_bits(wrapper_name_value.bits()),
        );
        crate::gc::runtime_write_barrier_root_heap_word(closure as u64);
        f64::from_bits(JSValue::pointer(closure as *mut u8).bits())
    }
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
    crate::object::install_native_module_vtable();
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
        "resource" => Some(ENTRY_TYPE_RESOURCE),
        "function" => Some(ENTRY_TYPE_FUNCTION),
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
        let opts_jv = JSValue::from_bits(opts.to_bits());
        if opts_jv.is_undefined() {
            throw_type_error_with_code(
                "The \"options\" argument must be specified",
                "ERR_MISSING_ARGS",
            );
        }
        let Some(opts_obj) = as_object_ptr(opts) else {
            throw_type_error_with_code(
                "The \"options\" argument must be of type object",
                "ERR_INVALID_ARG_TYPE",
            );
        };

        let entry_types_v = option_value(opts_obj, "entryTypes");
        let type_v = option_value(opts_obj, "type");
        let has_entry_types = !entry_types_v.is_undefined();
        let has_type = !type_v.is_undefined();
        if !has_entry_types && !has_type {
            throw_type_error_with_code(
                "The \"options.entryTypes\" or \"options.type\" argument must be specified",
                "ERR_MISSING_ARGS",
            );
        }
        if has_entry_types && has_type {
            throw_type_error_with_code(
                "The \"options.entryTypes\" and \"options.type\" arguments cannot both be specified",
                "ERR_INVALID_ARG_VALUE",
            );
        }

        if has_entry_types {
            if !is_array_value(entry_types_v) {
                throw_type_error_with_code(
                    "The \"options.entryTypes\" argument must be an instance of Array",
                    "ERR_INVALID_ARG_TYPE",
                );
            }
            let arr = array_ptr_from_value(entry_types_v);
            let len = crate::array::js_array_length(arr);
            for i in 0..len {
                let el = crate::array::js_array_get(arr, i);
                let Some(s) = string_of(el) else {
                    throw_type_error_with_code(
                        "The \"options.entryTypes\" argument must be an array of strings",
                        "ERR_INVALID_ARG_TYPE",
                    );
                };
                if let Some(code) = entry_type_code(&s) {
                    types.push(code);
                }
            }
        }

        if has_type {
            let Some(s) = string_of(type_v) else {
                throw_type_error_with_code(
                    "The \"options.type\" argument must be of type string",
                    "ERR_INVALID_ARG_TYPE",
                );
            };
            if let Some(code) = entry_type_code(&s) {
                types.push(code);
            }
        }

        // buffered: boolean — also deliver entries already on the timeline.
        let b_v = option_value(opts_obj, "buffered");
        let buffered = crate::value::js_is_truthy(f64::from_bits(b_v.bits())) != 0;
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
        let mut arr = crate::array::js_array_alloc(4);
        for t in ["function", "mark", "measure", "resource"] {
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
            if e.object_bits != 0 {
                visitor.visit_nanbox_u64_slot(&mut e.object_bits);
            }
        }
    });
    OBSERVERS.with(|o| {
        for obs in o.borrow_mut().iter_mut() {
            visitor.visit_nanbox_u64_slot(&mut obs.cb_bits);
            visitor.visit_nanbox_u64_slot(&mut obs.obj_bits);
            for e in obs.pending.iter_mut() {
                visitor.visit_nanbox_u64_slot(&mut e.detail_bits);
                if e.object_bits != 0 {
                    visitor.visit_nanbox_u64_slot(&mut e.object_bits);
                }
            }
        }
    });
    CURRENT_LIST.with(|c| {
        for e in c.borrow_mut().iter_mut() {
            visitor.visit_nanbox_u64_slot(&mut e.detail_bits);
            if e.object_bits != 0 {
                visitor.visit_nanbox_u64_slot(&mut e.object_bits);
            }
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
    crate::object::install_native_module_vtable();
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
