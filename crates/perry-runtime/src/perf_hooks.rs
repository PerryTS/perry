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

mod resource_timing;
pub use resource_timing::{
    js_perf_clear_resource_timings, js_perf_mark_resource_timing,
    js_perf_set_resource_timing_buffer_size,
};

mod timerify;
pub use timerify::js_perf_timerify;

use crate::object::{
    js_object_alloc_with_shape, js_object_get_field, js_object_get_field_by_name,
    js_object_set_field, js_object_set_field_by_name,
};
use crate::string::StringHeader;
use crate::value::JSValue;
use std::cell::{Cell, RefCell};
use std::sync::OnceLock;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

mod prototypes;

pub(crate) use prototypes::{attach_perf_hooks_constructor, perf_supported_entry_types_value};
use prototypes::{is_perf_constructor_name, link_perf_prototype};

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

/// Shape for a `PerformanceResourceTiming` entry. Node exposes these as
/// prototype accessors (so `Object.keys(entry)` is empty there and non-empty
/// here), but the value set and its `toJSON()` projection are exactly this
/// list. `name`/`entryType`/`startTime`/`duration` stay at indices 0..3 so the
/// shared `perf_entry_type` / instanceof / sorting paths keep working.
const RESOURCE_ENTRY_SHAPE: u32 = 0x7FFF_FF44;
const RESOURCE_ENTRY_JSON_SHAPE: u32 = 0x7FFF_FF45;
const RESOURCE_ENTRY_KEYS: &[u8] = b"name\0entryType\0startTime\0duration\0initiatorType\0nextHopProtocol\0workerStart\0redirectStart\0redirectEnd\0fetchStart\0domainLookupStart\0domainLookupEnd\0connectStart\0connectEnd\0secureConnectionStart\0requestStart\0responseStart\0responseEnd\0transferSize\0encodedBodySize\0decodedBodySize\0responseStatus\0deliveryType\0";
const RESOURCE_ENTRY_FIELD_COUNT: u32 = 23;

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

/// Shape id + keys for the plain object `performance.nodeTiming.toJSON()`
/// returns — the nodeTiming fields minus `uvMetricsInfo`.
const NODE_TIMING_JSON_SHAPE: u32 = 0x7FFF_FF52;
const NODE_TIMING_JSON_KEYS: &[u8] = b"name\0entryType\0startTime\0duration\0nodeStart\0v8Start\0bootstrapComplete\0environment\0loopStart\0loopExit\0idleTime\0";

/// Shape id + keys for `performance.nodeTiming.uvMetricsInfo`.
const UV_METRICS_INFO_SHAPE: u32 = 0x7FFF_FF51;
const UV_METRICS_INFO_KEYS: &[u8] = b"loopCount\0events\0eventsWaiting\0";

#[derive(Clone)]
pub(crate) struct PerfEntry {
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

    /// Same trick for the wider `PerformanceResourceTiming` shape.
    static RESOURCE_ENTRY_KEYS_ARRAY: Cell<usize> = const { Cell::new(0) };

    /// Cached `performance.nodeTiming` entry (NaN-boxed bits, 0 = uninit).
    /// Node returns one PerformanceNodeTiming instance for the process, and
    /// `performance.toJSON().nodeTiming` is that same object — both
    /// `timing === performance.nodeTiming` and the toJSON snapshot's
    /// non-freshness are observable. GC-rooted below.
    static NODE_TIMING: Cell<u64> = const { Cell::new(0) };

    /// Cached frozen `PerformanceObserver.supportedEntryTypes` array. GC-rooted
    /// below.
    static SUPPORTED_ENTRY_TYPES: Cell<u64> = const { Cell::new(0) };

    /// `keys_array` pointer shared by the nodeTiming entry — the same
    /// single-compare recognition trick `PERF_ENTRY_KEYS_ARRAY` uses, so
    /// `nodeTiming.toJSON()` can be synthesized without giving the entry an own
    /// `toJSON` key (Node's is on the prototype, and
    /// `Object.keys(nodeTiming)` must stay at the 12 milestone names).
    static NODE_TIMING_KEYS_ARRAY: Cell<usize> = const { Cell::new(0) };

    /// `performance.timeOrigin`-relative timestamp of the first event-loop
    /// turn, or -1 while the loop has not started. Node's `nodeTiming.loopStart`
    /// sentinel, and the gate `eventLoopUtilization()` reads before reporting
    /// anything but zeros.
    static LOOP_START_MS: Cell<f64> = const { Cell::new(-1.0) };
}

/// Called from the first event-loop turn (`js_callback_timer_tick`) to stamp
/// `nodeTiming.loopStart`. Idempotent and cheap enough for the tick path.
pub(crate) fn note_event_loop_start() {
    LOOP_START_MS.with(|c| {
        if c.get() < 0.0 {
            c.set(performance_now_ms().max(0.0));
        }
    });
}

/// True when `obj` is a mark/measure entry object produced by
/// `entry_to_object` — i.e. its `keys_array` is the recorded shared
/// `PERF_ENTRY_SHAPE` allocation. The toJSON-result object uses a different
/// shape, so it deliberately does not match. (#1387)
pub(crate) unsafe fn is_perf_entry_object(obj: *const crate::object::ObjectHeader) -> bool {
    if obj.is_null() {
        return false;
    }
    let keys = crate::object::object_keys_array(obj) as usize;
    let recorded = PERF_ENTRY_KEYS_ARRAY.with(|c| c.get());
    if recorded != 0 && keys == recorded {
        return true;
    }
    let resource = RESOURCE_ENTRY_KEYS_ARRAY.with(|c| c.get());
    resource != 0 && keys == resource
}

/// True when `obj` is a `PerformanceResourceTiming` entry — the wider shape
/// whose `toJSON()` projects 23 keys rather than the base entry's 5.
pub(crate) unsafe fn is_resource_entry_object(obj: *const crate::object::ObjectHeader) -> bool {
    if obj.is_null() {
        return false;
    }
    let recorded = RESOURCE_ENTRY_KEYS_ARRAY.with(|c| c.get());
    recorded != 0 && crate::object::object_keys_array(obj) as usize == recorded
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
    // PerformanceNodeTiming is a PerformanceEntry (entryType "node") but not a
    // mark/measure/resource, so it answers only the base-class question.
    if is_node_timing_object(obj) {
        return Some(want.is_none());
    }
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
    if is_resource_entry_object(src) {
        let n = RESOURCE_ENTRY_FIELD_COUNT as usize;
        let fields: Vec<JSValue> = (0..n).map(|i| js_object_get_field(src, i as u32)).collect();
        let out = js_object_alloc_with_shape(
            RESOURCE_ENTRY_JSON_SHAPE,
            RESOURCE_ENTRY_FIELD_COUNT,
            RESOURCE_ENTRY_KEYS.as_ptr(),
            RESOURCE_ENTRY_KEYS.len() as u32,
        );
        for (i, v) in fields.iter().enumerate() {
            js_object_set_field(out, i as u32, *v);
        }
        return crate::value::js_nanbox_pointer(out as i64);
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
    let ns = crate::object::js_create_native_module_namespace(module.as_ptr(), module.len());
    let ns = link_perf_prototype(ns, "Performance");
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

/// A `TypeError` with no Node `code` — what surfaces when Node reads an
/// internal symbol off a nullish argument before its own validation runs
/// (`histogram.add(undefined)`).
pub(crate) fn throw_plain_type_error(msg: &str) -> ! {
    throw_type_error(msg)
}

fn throw_type_error_with_code(msg: &str, code: &'static str) -> ! {
    crate::fs::validate::throw_type_error_with_code(msg, code)
}

/// Throw a `DOMException` with the given `name`. The User Timing and
/// PerformanceObserver specs raise DOMExceptions, not Error subclasses, so
/// `error.name` is the observable — `SyntaxError` for an unset mark endpoint,
/// `InvalidModificationError` for an observer mode switch.
fn throw_dom_exception(msg: &str, name: &str) -> ! {
    let message = f64::from_bits(str_value(msg).bits());
    let name = f64::from_bits(str_value(name).bits());
    let err = crate::event_target::js_dom_exception_new(message, name);
    crate::exception::js_throw(crate::value::js_nanbox_pointer(err as i64))
}

fn throw_syntax_error_with_code(msg: &str, code: &'static str) -> ! {
    let msg_str = crate::string::js_string_from_bytes(msg.as_ptr(), msg.len() as u32);
    crate::node_submodules::register_error_code_pub(msg_str, code);
    let err_ptr = crate::error::js_syntaxerror_new(msg_str);
    let err_value = JSValue::pointer(err_ptr as *const u8).bits();
    crate::exception::js_throw(f64::from_bits(err_value))
}

/// The `nodeTiming` milestones Node refuses as user-timing mark names — a
/// `performance.mark("nodeStart")` would shadow the milestone that
/// `measure({ start: "nodeStart" })` resolves against.
const RESERVED_MILESTONE_NAMES: &[&str] = &[
    "nodeStart",
    "v8Start",
    "environment",
    "loopStart",
    "loopExit",
    "bootstrapComplete",
];

fn reject_reserved_milestone_name(name: &str) {
    if RESERVED_MILESTONE_NAMES.contains(&name) {
        throw_type_error_with_code(
            &format!("The argument 'name' must not be one of the node timing milestones. Received '{name}'"),
            "ERR_INVALID_ARG_VALUE",
        );
    }
}

/// Milestone lookup for `measure({ start: "nodeStart" })` — the reserved names
/// resolve against `nodeTiming`, not the mark timeline.
fn milestone_start(name: &str) -> Option<f64> {
    match name {
        "nodeStart" | "v8Start" | "environment" | "bootstrapComplete" => Some(0.0),
        "loopStart" => Some(LOOP_START_MS.with(|c| c.get())),
        "loopExit" => Some(-1.0),
        _ => None,
    }
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
    // `detail` can be an arbitrary heap value. Every allocation below may
    // trigger a moving collection, so keep it rooted until the field and any
    // timerify argument aliases have been installed on the entry.
    let scope = crate::gc::RuntimeHandleScope::new();
    let detail_handle = scope.root_nanbox_f64(f64::from_bits(e.detail_bits));
    let obj = js_object_alloc_with_shape(
        PERF_ENTRY_SHAPE,
        5,
        PERF_ENTRY_KEYS.as_ptr(),
        PERF_ENTRY_KEYS.len() as u32,
    );
    let obj_handle = scope.root_nanbox_f64(crate::value::js_nanbox_pointer(obj as i64));
    let current_obj = || {
        crate::value::js_nanbox_get_pointer(obj_handle.get_nanbox_f64())
            as *mut crate::object::ObjectHeader
    };
    // Record the shared keys_array so `is_perf_entry_object` can recognize
    // entries by pointer identity (see PERF_ENTRY_KEYS_ARRAY). All entries on
    // this thread share it, so a single store on the first call suffices.
    let keys_ptr = crate::object::object_keys_array(obj) as usize;
    PERF_ENTRY_KEYS_ARRAY.with(|c| {
        if c.get() == 0 {
            c.set(keys_ptr);
        }
    });
    let name = str_value(&e.name);
    js_object_set_field(current_obj(), 0, name);
    let entry_type = str_value(entry_type_name(e.entry_type));
    js_object_set_field(current_obj(), 1, entry_type);
    js_object_set_field(current_obj(), 2, JSValue::number(e.start_time));
    js_object_set_field(current_obj(), 3, JSValue::number(e.duration));
    js_object_set_field(
        current_obj(),
        4,
        JSValue::from_bits(detail_handle.get_nanbox_f64().to_bits()),
    );
    // Node exposes timerify's call arguments twice: as `entry.detail` and as
    // enumerable indexed properties on the PerformanceEntry itself.
    if e.entry_type == ENTRY_TYPE_FUNCTION {
        let detail = crate::value::js_nanbox_get_pointer(detail_handle.get_nanbox_f64())
            as *const crate::array::ArrayHeader;
        if !detail.is_null() {
            let len = crate::array::js_array_length(detail);
            for i in 0..len {
                let key_text = i.to_string();
                let key =
                    crate::string::js_string_from_bytes(key_text.as_ptr(), key_text.len() as u32);
                let detail = crate::value::js_nanbox_get_pointer(detail_handle.get_nanbox_f64())
                    as *const crate::array::ArrayHeader;
                let value = crate::array::js_array_get_f64(detail, i);
                js_object_set_field_by_name(current_obj(), key, value);
            }
        }
    }
    if let Some(initiator_type) = &e.initiator_type {
        let value = str_value(initiator_type);
        set_named_field(current_obj(), "initiatorType", value);
    }
    let class_name = match e.entry_type {
        ENTRY_TYPE_MARK => "PerformanceMark",
        ENTRY_TYPE_MEASURE => "PerformanceMeasure",
        ENTRY_TYPE_RESOURCE => "PerformanceResourceTiming",
        _ => "PerformanceEntry",
    };
    link_perf_prototype(obj_handle.get_nanbox_f64(), class_name)
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
            None => throw_syntax_error_with_code(
                &format!("The \"{name}\" performance mark has not been set"),
                "12",
            ),
        }
    } else {
        0.0
    }
}

/// Most-recent mark startTime for `name`, if any.
fn lookup_mark_start(name: &str) -> Option<f64> {
    if let Some(milestone) = milestone_start(name) {
        return Some(milestone);
    }
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
        // Node structured-clones `detail`, so the stored value deep-equals the
        // input but is a distinct reference (mutating the original afterward
        // doesn't affect the entry).
        crate::builtins::js_structured_clone(f64::from_bits(v.bits())).to_bits()
    }
}

pub(crate) fn as_object_ptr(v: f64) -> Option<*const crate::object::ObjectHeader> {
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
        reject_reserved_milestone_name(&name);
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

/// `new PerformanceMark(name, options?)` creates a detached mark. Node clones
/// `detail` exactly like `performance.mark`, but does not append the result to
/// the global performance timeline or notify observers.
#[no_mangle]
pub extern "C" fn js_perf_mark_constructor(name_val: f64, options_val: f64) -> f64 {
    unsafe {
        if crate::symbol::js_is_symbol(name_val) != 0 {
            throw_type_error("Cannot convert a Symbol value to a string");
        }
        let name = coerce_to_string(name_val);
        let mut start_time = perf_now();
        let mut detail_bits = JSValue::null().bits();
        if let Some(opts) = as_object_ptr(options_val) {
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
        entry_to_object(&entry)
    }
}

#[no_mangle]
pub extern "C" fn js_perf_illegal_constructor() -> f64 {
    throw_type_error_with_code("Illegal constructor", "ERR_ILLEGAL_CONSTRUCTOR")
}

pub(crate) unsafe fn construct_perf_hooks_class(
    class_name: &str,
    args_ptr: *const f64,
    args_len: usize,
) -> Option<f64> {
    if !is_perf_constructor_name(class_name) {
        return None;
    }
    let args = if args_ptr.is_null() {
        &[][..]
    } else {
        std::slice::from_raw_parts(args_ptr, args_len)
    };
    let undefined = f64::from_bits(crate::value::TAG_UNDEFINED);
    Some(match class_name {
        "PerformanceMark" => js_perf_mark_constructor(
            args.first().copied().unwrap_or(undefined),
            args.get(1).copied().unwrap_or(undefined),
        ),
        "PerformanceObserver" => js_perf_observer_new(args.first().copied().unwrap_or(undefined)),
        _ => js_perf_illegal_constructor(),
    })
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
        require_entry_query_argument(type_val, "type");
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
        require_entry_query_argument(name_val, "name");
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

/// Shared guard for `getEntriesByName(name)` / `getEntriesByType(type)`: Node
/// throws `ERR_MISSING_ARGS` when the query argument is absent, and a plain
/// TypeError for a Symbol (which cannot be coerced to a string).
///
/// Perry's lowering passes `undefined` for an omitted trailing argument, so an
/// explicit `getEntriesByName(undefined)` is indistinguishable from the
/// zero-argument form and takes the same throw. Node would treat the explicit
/// form as the literal name "undefined".
unsafe fn require_entry_query_argument(value: f64, arg_name: &str) {
    if crate::symbol::js_is_symbol(value) != 0 {
        throw_type_error("Cannot convert a Symbol value to a string");
    }
    if JSValue::from_bits(value.to_bits()).is_undefined() {
        throw_type_error_with_code(
            &format!("The \"{arg_name}\" argument must be specified"),
            "ERR_MISSING_ARGS",
        );
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
        let name = coerce_to_string(name_val);
        if entry_type == ENTRY_TYPE_MARK {
            reject_reserved_milestone_name(&name);
        }
        Some(name)
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
    // Node's `eventLoopUtilization()` short-circuits to all-zeros while
    // `nodeTiming.loopStart <= 0` — i.e. for every call made during module
    // evaluation, before the loop has run a turn. That gate holds for the
    // two-argument diff form too, which is why a synthetic
    // `eventLoopUtilization(newer, older)` reports 0/0 rather than the
    // arithmetic difference of its arguments.
    if LOOP_START_MS.with(|c| c.get()) <= 0.0 {
        return (0.0, 0.0);
    }
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
        if LOOP_START_MS.with(|c| c.get()) <= 0.0 {
            return make_elu_object(0.0, 0.0);
        }
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
    let cached = NODE_TIMING.with(|c| c.get());
    if cached != 0 {
        return f64::from_bits(cached);
    }
    let value = unsafe { make_node_timing_object() };
    NODE_TIMING.with(|c| c.set(value.to_bits()));
    value
}

unsafe fn make_node_timing_object() -> f64 {
    {
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
        js_object_set_field(obj, 8, JSValue::number(LOOP_START_MS.with(|c| c.get()))); // loopStart
        js_object_set_field(obj, 9, JSValue::number(-1.0)); // loopExit (loop running)
        js_object_set_field(obj, 10, JSValue::number(0.0)); // idleTime
        js_object_set_field(
            obj,
            11,
            JSValue::from_bits(uv_metrics_handle.get_nanbox_u64()),
        );
        NODE_TIMING_KEYS_ARRAY.with(|c| c.set(crate::object::object_keys_array(obj) as usize));
        crate::value::js_nanbox_pointer(obj as i64)
    }
}

/// True when `obj` is the `performance.nodeTiming` entry.
pub(crate) unsafe fn is_node_timing_object(obj: *const crate::object::ObjectHeader) -> bool {
    if obj.is_null() {
        return false;
    }
    let recorded = NODE_TIMING_KEYS_ARRAY.with(|c| c.get());
    recorded != 0 && crate::object::object_keys_array(obj) as usize == recorded
}

/// `performance.nodeTiming.toJSON()` — the milestone numbers plus the entry
/// header, minus `uvMetricsInfo` (Node's serializer omits it).
pub(crate) unsafe fn node_timing_to_json(this: f64) -> f64 {
    let jv = JSValue::from_bits(this.to_bits());
    if !jv.is_pointer() {
        return f64::from_bits(crate::value::TAG_UNDEFINED);
    }
    let src = jv.as_pointer::<crate::object::ObjectHeader>();
    if src.is_null() {
        return f64::from_bits(crate::value::TAG_UNDEFINED);
    }
    // Snapshot before allocating: the alloc can move `src`.
    let fields: [JSValue; 11] = std::array::from_fn(|i| js_object_get_field(src, i as u32));
    let out = js_object_alloc_with_shape(
        NODE_TIMING_JSON_SHAPE,
        11,
        NODE_TIMING_JSON_KEYS.as_ptr(),
        NODE_TIMING_JSON_KEYS.len() as u32,
    );
    for (i, v) in fields.iter().enumerate() {
        js_object_set_field(out, i as u32, *v);
    }
    crate::value::js_nanbox_pointer(out as i64)
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
    /// True from the moment an entry is buffered until the queued flush runs.
    /// `takeRecords()` empties `pending` but leaves this set: Node's dispatch
    /// was already scheduled and still fires, with an empty entry list. Keying
    /// the flush on `!pending.is_empty()` instead swallowed that call.
    flush_queued: bool,
    /// `Some(true)` once `observe({ type })` has subscribed this observer,
    /// `Some(false)` for `observe({ entryTypes })`. The two modes are not
    /// interchangeable: an active observer that switches raises
    /// `InvalidModificationError`. Cleared by `disconnect()`.
    single_mode: Option<bool>,
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
    // These namespace tags never appear in user source (they are handed out as
    // return values), so codegen emits no `js_nm_install_perf()` for them and
    // the dispatch bucket would be empty — every method call on the object
    // would resolve to `undefined` and silently do nothing. Arm it here.
    crate::object::js_nm_install_perf();
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
    link_perf_prototype(
        crate::value::js_nanbox_pointer(obj as i64),
        "PerformanceObserver",
    )
}

fn is_perf_observer_value(value: f64) -> bool {
    unsafe {
        let Some(obj) = as_object_ptr(value) else {
            return false;
        };
        string_of(js_object_get_field(obj, 0)).as_deref() == Some("perf_observer")
    }
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
                flush_queued: false,
                single_mode: None,
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
    match as_object_ptr(obs_val) {
        Some(obj) => observer_id_from_field(crate::object::js_object_get_field(obj as *mut _, 1)),
        None => 0,
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

        // Node returns early when the request resolves to no supported entry
        // type at all — `observe({ entryTypes: [] })` and
        // `observe({ type: "bogus" })` are no-ops, and in particular they do
        // NOT pin the observer's subscription mode. Checking the mode first
        // would reject a later `observe({ type })` that Node accepts.
        if types.is_empty() {
            return f64::from_bits(JSValue::undefined().bits());
        }

        // An active observer cannot move between the two subscription modes.
        let previous_mode = OBSERVERS.with(|o| {
            o.borrow()
                .get(id)
                .filter(|obs| obs.active)
                .and_then(|obs| obs.single_mode)
        });
        if previous_mode.is_some_and(|previous| previous != has_type) {
            throw_dom_exception(
                "Cannot change to a different PerformanceObserver subscription mode",
                "InvalidModificationError",
            );
        }

        // buffered: boolean — also deliver entries already on the timeline.
        let b_v = option_value(opts_obj, "buffered");
        let buffered = crate::value::js_is_truthy(f64::from_bits(b_v.bits())) != 0;
        OBSERVERS.with(|o| {
            if let Some(obs) = o.borrow_mut().get_mut(id) {
                // `observe({ type })` ADDS to the subscription (Node's single
                // mode accumulates across calls); `observe({ entryTypes })`
                // REPLACES it wholesale.
                if has_type && obs.active {
                    for code in &types {
                        if !obs.entry_types.contains(code) {
                            obs.entry_types.push(*code);
                        }
                    }
                } else {
                    obs.entry_types = types;
                }
                obs.active = true;
                obs.single_mode = Some(has_type);
            }
        });
        let observed = OBSERVERS
            .with(|o| o.borrow().get(id).map(|obs| obs.entry_types.clone()))
            .unwrap_or_default();
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
                        obs.flush_queued = true;
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
            obs.single_mode = None;
            obs.flush_queued = false;
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
                obs.flush_queued = true;
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
    {
        // Node dispatches observer callbacks from the check (setImmediate)
        // phase, so a `setImmediate` a caller registers AFTER creating an
        // entry still runs after the callback. A `setTimeout(0)` here lands in
        // the timer phase instead, i.e. before an immediate the test awaits —
        // which is why `await new Promise(r => setImmediate(r))` saw
        // "not delivered".
        let closure =
            crate::closure::js_closure_alloc_singleton(js_perf_observer_flush_all as *const u8);
        crate::timer::js_set_immediate_callback(closure as i64);
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
            .filter(|obs| obs.active && obs.flush_queued)
            .map(|obs| {
                obs.flush_queued = false;
                (obs.cb_bits, obs.obj_bits, std::mem::take(&mut obs.pending))
            })
            .collect()
    });
    for (cb_bits, obj_bits, entries) in work {
        {
            CURRENT_LIST.with(|c| *c.borrow_mut() = entries);
            // These namespace tags never appear in user source (they are handed out as
            // return values), so codegen emits no `js_nm_install_perf()` for them and
            // the dispatch bucket would be empty — every method call on the object
            // would resolve to `undefined` and silently do nothing. Arm it here.
            crate::object::js_nm_install_perf();
            let module = b"perf_observer_list";
            let list =
                crate::object::js_create_native_module_namespace(module.as_ptr(), module.len());
            let list = link_perf_prototype(list, "PerformanceObserverEntryList");
            let cb_jv = JSValue::from_bits(cb_bits);
            if cb_jv.is_pointer() {
                // Node invokes the callback as `(list, observer)` with `this`
                // bound to the observer, so a `function () { this === observer }`
                // callback sees it. Route through Reflect.apply rather than the
                // plain closure call, which leaves `this` undefined.
                let mut args = crate::array::js_array_alloc(2);
                args = crate::array::js_array_push(args, JSValue::from_bits(list.to_bits()));
                args = crate::array::js_array_push(args, JSValue::from_bits(obj_bits));
                crate::proxy::js_reflect_apply(
                    f64::from_bits(cb_bits),
                    f64::from_bits(obj_bits),
                    crate::value::js_nanbox_pointer(args as i64),
                );
            }
            CURRENT_LIST.with(|c| c.borrow_mut().clear());
        }
    }
    f64::from_bits(JSValue::undefined().bits())
}

/// Build an array from the in-flight observer `list` entries (for the
/// `perf_observer_list` namespace methods).
pub(crate) unsafe fn current_list_to_array(filter: impl Fn(&PerfEntry) -> bool) -> f64 {
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

pub(crate) fn validate_perf_list_filter_arg(value: f64, name: &str, missing: bool) {
    if missing || JSValue::from_bits(value.to_bits()).is_undefined() {
        throw_type_error_with_code(
            &format!("The \"{name}\" argument must be specified"),
            "ERR_MISSING_ARGS",
        );
    }
    if unsafe { crate::symbol::js_is_symbol(value) } != 0 {
        throw_type_error("Cannot convert a Symbol value to a string");
    }
}

pub unsafe fn current_list_get_by_type(type_val: f64) -> f64 {
    require_entry_query_argument(type_val, "type");
    let want = coerce_to_string(type_val);
    match entry_type_code(&want) {
        Some(code) => current_list_to_array(move |e| e.entry_type == code),
        None => current_list_to_array(|_| false),
    }
}

pub unsafe fn current_list_get_by_name(name_val: f64) -> f64 {
    require_entry_query_argument(name_val, "name");
    let want = coerce_to_string(name_val);
    current_list_to_array(move |e| e.name == want)
}

/// `PerformanceObserver.supportedEntryTypes` — Node's exact list, frozen, and
/// the SAME array on every read (the getter caches it, so
/// `types !== PerformanceObserver.supportedEntryTypes` is false). Perry only
/// *produces* mark/measure/resource/function entries; the list is a static
/// declaration of the spec's entry-type vocabulary, which Node reports in full
/// regardless of what the process has emitted.
#[no_mangle]
pub extern "C" fn js_perf_supported_entry_types() -> f64 {
    let cached = SUPPORTED_ENTRY_TYPES.with(|c| c.get());
    if cached != 0 {
        return f64::from_bits(cached);
    }
    let mut arr = crate::array::js_array_alloc(10);
    for t in [
        "dns", "function", "gc", "http", "http2", "mark", "measure", "net", "quic", "resource",
    ] {
        arr = crate::array::js_array_push(arr, str_value(t));
    }
    unsafe {
        let gc_header = (arr as *mut u8).sub(crate::gc::GC_HEADER_SIZE) as *mut crate::gc::GcHeader;
        (*gc_header)._reserved |=
            crate::gc::OBJ_FLAG_FROZEN | crate::gc::OBJ_FLAG_SEALED | crate::gc::OBJ_FLAG_NO_EXTEND;
    }
    let value = crate::value::js_nanbox_pointer(arr as i64);
    SUPPORTED_ENTRY_TYPES.with(|c| c.set(value.to_bits()));
    value
}

// ── GC root scanner ──────────────────────────────────────────────────────────
/// Keep `detail` JSValues stored in the timeline + observer buffers, and the
/// observer callbacks, alive across GC.
pub fn scan_perf_entries_roots_mut(visitor: &mut crate::gc::RuntimeRootVisitor<'_>) {
    for cell in [&NODE_TIMING, &SUPPORTED_ENTRY_TYPES] {
        cell.with(|c| {
            let mut bits = c.get();
            if bits != 0 {
                visitor.visit_nanbox_u64_slot(&mut bits);
                c.set(bits);
            }
        });
    }
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
    // These are identity indices into structurally rooted entry objects, not
    // owners. Follow a forwarding address without keeping the keys array
    // alive on its own.
    for cell in [
        &PERF_ENTRY_KEYS_ARRAY,
        &RESOURCE_ENTRY_KEYS_ARRAY,
        &NODE_TIMING_KEYS_ARRAY,
    ] {
        cell.with(|c| {
            let mut addr = c.get();
            if addr != 0 && visitor.visit_metadata_usize_slot(&mut addr) {
                c.set(addr);
            }
        });
    }
}

#[cfg(test)]
pub(crate) fn test_seed_perf_entry_keys_array(addr: usize) {
    PERF_ENTRY_KEYS_ARRAY.with(|slot| slot.set(addr));
}

#[cfg(test)]
pub(crate) fn test_perf_entry_keys_array() -> usize {
    PERF_ENTRY_KEYS_ARRAY.with(|slot| slot.get())
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
        {
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
