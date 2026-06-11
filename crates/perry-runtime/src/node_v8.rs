//! `node:v8` public API surface.
//!
//! Implements the subset of the Node `v8` module that Perry can back with
//! native internals:
//!
//! * `v8.serialize(value)` / `v8.deserialize(buf)` (#3137) — reuse the V8
//!   structured-clone codec that already backs `child_process` advanced IPC
//!   (`child_process::v8_serialize` / `v8_deserialize`). `serialize` wraps the
//!   bytes in a Node `Buffer`; `deserialize` reads the bytes back out of a
//!   Buffer / TypedArray. The wire framing is Perry's own (host-object
//!   discriminator etc.), so it is NOT byte-compatible with V8's exact output,
//!   but `deserialize(serialize(x))` round-trips faithfully which is all the
//!   public contract guarantees.
//! * `v8.getHeapStatistics()` / `getHeapCodeStatistics()` /
//!   `getHeapSpaceStatistics()` / `cachedDataVersionTag()` (#3138) — return the
//!   Node-compatible object/array *shapes* with numeric values sourced from
//!   Perry's arena / RSS counters. The field *names and types* match Node so
//!   package feature-detection works; the values reflect Perry internals.
//! * `v8.getHeapSnapshot([options])` / `writeHeapSnapshot([filename[, options]])`
//!   (#3140, #4916) — a real V8-format heap snapshot built from Perry's GC heap
//!   walk (`gc::gc_build_v8_heap_snapshot_json`): actual nodes, sizes, and
//!   reference edges for the calling thread's heap. The `options` bag
//!   (`exposeInternals` etc.) is validated but ignored.
//! * `v8.GCProfiler` (#3142) — `new v8.GCProfiler()` allocates a small native
//!   instance; `start()` returns `undefined` and `stop()` returns a
//!   `{ version, startTime, statistics, endTime }` report object only after the
//!   profiler has been started.

use crate::object::ObjectHeader;
use crate::string::js_string_from_bytes;
use crate::value::JSValue;

// Symbol retention: these `#[no_mangle]` entry points are emitted only by
// codegen's `node:v8` dispatch — no Rust caller references them, so the
// auto-optimize whole-program-LLVM build would dead-strip them without an
// anchor (see node_stream_keepalive.rs). Pin each via a `#[used]` static.
#[used]
static KEEP_V8_SERIALIZE: extern "C" fn(f64) -> f64 = js_v8_serialize;
#[used]
static KEEP_V8_DESERIALIZE: extern "C" fn(f64) -> f64 = js_v8_deserialize;
#[used]
static KEEP_V8_HEAP_STATS: extern "C" fn() -> f64 = js_v8_get_heap_statistics;
#[used]
static KEEP_V8_CODE_STATS: extern "C" fn() -> f64 = js_v8_get_heap_code_statistics;
#[used]
static KEEP_V8_SPACE_STATS: extern "C" fn() -> f64 = js_v8_get_heap_space_statistics;
#[used]
static KEEP_V8_VERSION_TAG: extern "C" fn() -> f64 = js_v8_cached_data_version_tag;
#[used]
static KEEP_V8_GET_HEAP_SNAPSHOT: extern "C" fn(f64) -> f64 = js_v8_get_heap_snapshot;
#[used]
static KEEP_V8_WRITE_HEAP_SNAPSHOT: extern "C" fn(f64, f64) -> f64 = js_v8_write_heap_snapshot;
#[used]
static KEEP_V8_GC_PROFILER_NEW: extern "C" fn() -> f64 = js_v8_gc_profiler_new;
#[used]
static KEEP_V8_GC_PROFILER_START: extern "C" fn(f64) -> f64 = js_v8_gc_profiler_start;
#[used]
static KEEP_V8_GC_PROFILER_STOP: extern "C" fn(f64) -> f64 = js_v8_gc_profiler_stop;
#[used]
static KEEP_V8_GC_PROFILER_REPORT: extern "C" fn() -> f64 = js_v8_gc_profiler_report;
// #3680: Serializer / Deserializer class constructors.
#[used]
static KEEP_V8_SERIALIZER_NEW: extern "C" fn(f64) -> f64 = js_v8_serializer_new;
#[used]
static KEEP_V8_DESERIALIZER_NEW: extern "C" fn(f64) -> f64 = js_v8_deserializer_new;
// #3679: lifecycle / diagnostic-control surface.
#[used]
static KEEP_V8_NOOP_UNDEFINED: extern "C" fn() -> f64 = js_v8_noop_undefined;
#[used]
static KEEP_V8_IS_BUILDING_SNAPSHOT: extern "C" fn() -> f64 = js_v8_is_building_snapshot;
#[used]
static KEEP_V8_NAMESPACE: extern "C" fn(*const u8, usize) -> f64 = js_v8_namespace;
#[used]
static KEEP_V8_THROW_NOT_BUILDING: extern "C" fn() -> f64 = js_v8_throw_not_building_snapshot;
#[used]
static KEEP_V8_PROMISE_HOOK_REGISTER: extern "C" fn() -> f64 = js_v8_promise_hook_register;

const TAG_UNDEFINED_BITS: u64 = 0x7FFC_0000_0000_0001;

fn undefined() -> f64 {
    f64::from_bits(TAG_UNDEFINED_BITS)
}

/// Build a plain object from `(name, value)` numeric/any pairs.
unsafe fn build_object(pairs: &[(&str, f64)]) -> f64 {
    let obj = crate::object::js_object_alloc(0, pairs.len() as u32);
    for (name, value) in pairs {
        let key = js_string_from_bytes(name.as_ptr(), name.len() as u32);
        crate::object::js_object_set_field_by_name(obj, key, *value);
    }
    f64::from_bits(JSValue::pointer(obj as *const u8).bits())
}

/// Read the raw bytes backing a deserialize input. Accepts Node `Buffer`,
/// `Uint8Array` / other TypedArrays, and `ArrayBuffer`. Returns `None` for
/// anything else (caller throws `ERR_INVALID_ARG_TYPE` like Node).
unsafe fn input_bytes(value: f64) -> Option<Vec<u8>> {
    let jsv = JSValue::from_bits(value.to_bits());
    if !jsv.is_pointer() {
        return None;
    }
    let addr = (value.to_bits() & crate::value::POINTER_MASK) as usize;
    if addr < 0x10000 {
        return None;
    }
    if crate::buffer::is_registered_buffer(addr) {
        let data = crate::buffer::js_native_buffer_data_ptr(value);
        let len = crate::buffer::js_native_buffer_byte_len(value);
        if data.is_null() || len == 0 {
            return Some(Vec::new());
        }
        return Some(std::slice::from_raw_parts(data, len).to_vec());
    }
    if crate::typedarray::lookup_typed_array_kind(addr).is_some() {
        let ta = addr as *const crate::typedarray::TypedArrayHeader;
        return Some(
            crate::typedarray::typed_array_bytes(ta)
                .map(|b| b.to_vec())
                .unwrap_or_default(),
        );
    }
    None
}

fn is_valid_heap_snapshot_options(value: f64) -> bool {
    let jsv = JSValue::from_bits(value.to_bits());
    if jsv.is_undefined() {
        return true;
    }
    if !jsv.is_pointer() || jsv.is_any_string() {
        return false;
    }
    let ptr = jsv.as_pointer::<u8>() as usize;
    if ptr < crate::gc::GC_HEADER_SIZE + 0x1000 || crate::closure::is_closure_ptr(ptr) {
        return false;
    }
    unsafe {
        let header =
            &*((ptr as *const u8).sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader);
        header.obj_type == crate::gc::GC_TYPE_OBJECT
    }
}

fn validate_heap_snapshot_options(value: f64) {
    if is_valid_heap_snapshot_options(value) {
        return;
    }
    let message = format!(
        "The \"options\" argument must be of type object. Received {}",
        crate::fs::validate::describe_received(value)
    );
    crate::fs::validate::throw_type_error_with_code(&message, "ERR_INVALID_ARG_TYPE");
}

fn default_heap_snapshot_path() -> String {
    let millis = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    let name = format!("Heap-{}-{}.heapsnapshot", std::process::id(), millis);
    std::env::current_dir()
        .unwrap_or_else(|_| std::path::PathBuf::from("."))
        .join(name)
        .to_string_lossy()
        .into_owned()
}

fn string_value(value: &str) -> f64 {
    let ptr = js_string_from_bytes(value.as_ptr(), value.len() as u32);
    f64::from_bits(JSValue::string_ptr(ptr).bits())
}

fn snapshot_readable_stream(json: &str) -> f64 {
    let chunk = bytes_to_buffer(json.as_bytes());
    let mut chunks = crate::array::js_array_alloc(1);
    chunks = crate::array::js_array_push_f64(chunks, chunk);
    let chunks_value = f64::from_bits(JSValue::pointer(chunks as *const u8).bits());
    let opts = crate::object::js_object_alloc(0, 1);
    let object_mode_key = b"objectMode";
    let object_mode = js_string_from_bytes(object_mode_key.as_ptr(), object_mode_key.len() as u32);
    crate::object::js_object_set_field_by_name(
        opts,
        object_mode,
        f64::from_bits(JSValue::bool(false).bits()),
    );
    let opts_value = f64::from_bits(JSValue::pointer(opts as *const u8).bits());
    crate::node_stream::js_node_stream_readable_from_options(chunks_value, opts_value)
}

/// `v8.serialize(value)` → Node `Buffer` holding the structured-clone payload.
#[no_mangle]
pub extern "C" fn js_v8_serialize(value: f64) -> f64 {
    let bytes = crate::child_process::v8_serialize(value);
    let buf = crate::buffer::js_buffer_alloc(bytes.len() as i32, 0);
    if buf.is_null() {
        return undefined();
    }
    unsafe {
        let data = (buf as *mut u8).add(std::mem::size_of::<crate::buffer::BufferHeader>());
        if !bytes.is_empty() {
            std::ptr::copy_nonoverlapping(bytes.as_ptr(), data, bytes.len());
        }
        (*buf).length = bytes.len() as u32;
    }
    f64::from_bits(JSValue::pointer(buf as *const u8).bits())
}

/// `v8.deserialize(buffer)` → reconstructed JS value.
#[no_mangle]
pub extern "C" fn js_v8_deserialize(value: f64) -> f64 {
    let bytes = unsafe { input_bytes(value) };
    match bytes {
        Some(bytes) => crate::child_process::v8_deserialize(&bytes),
        None => crate::fs::validate::throw_type_error_with_code(
            "The \"buffer\" argument must be an instance of Buffer, TypedArray, or DataView.",
            "ERR_INVALID_ARG_TYPE",
        ),
    }
}

/// `v8.getHeapStatistics()` — Node-shaped heap stats with numeric values.
#[no_mangle]
pub extern "C" fn js_v8_get_heap_statistics() -> f64 {
    let mut heap_used: u64 = 0;
    let mut heap_total: u64 = 0;
    crate::arena::js_arena_stats(&mut heap_used, &mut heap_total);
    let rss = crate::process::get_rss_bytes();
    // A plausible default V8 old-space limit; not enforced by Perry.
    let heap_size_limit: u64 = 2_197_815_296;
    unsafe {
        build_object(&[
            ("total_heap_size", heap_total as f64),
            ("total_heap_size_executable", 0.0),
            ("total_physical_size", rss as f64),
            (
                "total_available_size",
                heap_size_limit.saturating_sub(heap_used) as f64,
            ),
            ("used_heap_size", heap_used as f64),
            ("heap_size_limit", heap_size_limit as f64),
            ("malloced_memory", heap_total as f64),
            ("peak_malloced_memory", heap_total as f64),
            ("does_zap_garbage", 0.0),
            ("number_of_native_contexts", 1.0),
            ("number_of_detached_contexts", 0.0),
            ("total_global_handles_size", 0.0),
            ("used_global_handles_size", 0.0),
            ("external_memory", 0.0),
            ("total_allocated_bytes", heap_total as f64),
        ])
    }
}

/// `v8.getHeapCodeStatistics()` — Node-shaped code stats (numeric values).
#[no_mangle]
pub extern "C" fn js_v8_get_heap_code_statistics() -> f64 {
    unsafe {
        build_object(&[
            ("code_and_metadata_size", 0.0),
            ("bytecode_and_metadata_size", 0.0),
            ("external_script_source_size", 0.0),
            ("cpu_profiler_metadata_size", 0.0),
        ])
    }
}

/// `v8.getHeapSpaceStatistics()` — array of space-stat objects.
#[no_mangle]
pub extern "C" fn js_v8_get_heap_space_statistics() -> f64 {
    let mut heap_used: u64 = 0;
    let mut heap_total: u64 = 0;
    crate::arena::js_arena_stats(&mut heap_used, &mut heap_total);
    let rss = crate::process::get_rss_bytes();
    let spaces: &[&str] = &[
        "read_only_space",
        "new_space",
        "old_space",
        "code_space",
        "shared_space",
        "new_large_object_space",
        "large_object_space",
        "code_large_object_space",
        "shared_large_object_space",
    ];
    let arr = crate::array::js_array_alloc(spaces.len() as u32);
    unsafe {
        for (i, name) in spaces.iter().enumerate() {
            // Attribute all live usage to old_space, the rest report empty —
            // a Node-compatible shape with non-negative numeric fields.
            let (size, used, avail) = if i == 2 {
                (
                    heap_total as f64,
                    heap_used as f64,
                    (heap_total.saturating_sub(heap_used)) as f64,
                )
            } else {
                (0.0, 0.0, 0.0)
            };
            let name_str = js_string_from_bytes(name.as_ptr(), name.len() as u32);
            let name_val = f64::from_bits(JSValue::string_ptr(name_str).bits());
            let entry = build_object(&[
                ("space_size", size),
                ("space_used_size", used),
                ("space_available_size", avail),
                ("physical_space_size", if i == 2 { rss as f64 } else { 0.0 }),
            ]);
            // Set space_name (a string) separately to keep build_object numeric.
            let entry_obj = (entry.to_bits() & crate::value::POINTER_MASK) as *mut ObjectHeader;
            let key = js_string_from_bytes(b"space_name".as_ptr(), 10);
            crate::object::js_object_set_field_by_name(entry_obj, key, name_val);
            crate::array::js_array_push_f64(arr, entry);
        }
    }
    f64::from_bits(JSValue::pointer(arr as *const u8).bits())
}

/// `v8.cachedDataVersionTag()` — a stable numeric tag for this build.
#[no_mangle]
pub extern "C" fn js_v8_cached_data_version_tag() -> f64 {
    // Node returns a uint32 derived from V8/flags; we return a stable
    // build-specific tag. The contract only requires a number. A plain
    // (non-integer-tagged) f64 is a valid JS number value.
    0x5045_5252u32 as f64
}

/// `v8.getHeapSnapshot([options])` → Readable stream of heap-snapshot JSON.
///
/// The document is a real object graph from Perry's GC heap walk
/// (#4916): nodes are the calling thread's live arena/malloc GC
/// allocations after a full collection, edges are the reference slots
/// the collector itself traces. The JSON must be fully built BEFORE
/// any JS-heap allocation (the stream below) happens — see the safety
/// note in `gc/heap_snapshot.rs`.
#[no_mangle]
pub extern "C" fn js_v8_get_heap_snapshot(options: f64) -> f64 {
    validate_heap_snapshot_options(options);
    let json = crate::gc::gc_build_v8_heap_snapshot_json();
    snapshot_readable_stream(&json)
}

/// `v8.writeHeapSnapshot([filename[, options]])` → written filename.
#[no_mangle]
pub extern "C" fn js_v8_write_heap_snapshot(filename: f64, options: f64) -> f64 {
    let filename_value = JSValue::from_bits(filename.to_bits());
    let path = if filename_value.is_undefined() {
        default_heap_snapshot_path()
    } else {
        crate::fs::validate::validate_path("path", filename);
        unsafe {
            crate::fs::decode_path_value(filename)
                .unwrap_or_else(|| crate::fs::validate::throw_invalid_path_arg("path", filename))
        }
    };
    validate_heap_snapshot_options(options);
    let json = crate::gc::gc_build_v8_heap_snapshot_json();
    match std::fs::write(&path, json.as_bytes()) {
        Ok(()) => string_value(&path),
        Err(err) => unsafe {
            crate::exception::js_throw(crate::fs::build_fs_error_value(&err, "open", &path))
        },
    }
}

// ============================================================
// #3680: `v8.Serializer` / `v8.Deserializer` class instances
// ============================================================
//
// Each instance is a `NATIVE_MODULE_CLASS_ID` namespace object whose field[0]
// holds the module tag (`"v8.Serializer"` etc.) and field[1] the registry id
// in `child_process::v8_serde`. Instance method calls land in
// `dispatch_native_module_method`, which re-derives the id from field[1] and
// calls the `instance_*` helpers below. This mirrors the `PerformanceObserver`
// pattern and avoids any new HIR/codegen variant.

/// Build a 2-field native-module namespace object: field[0] = `module` tag,
/// field[1] = registry `id`. NOT cached (every `new` must be a fresh instance).
unsafe fn build_v8_instance(module: &str, id: usize) -> f64 {
    let obj = crate::object::js_object_alloc(crate::object::NATIVE_MODULE_CLASS_ID, 2);
    let mname = js_string_from_bytes(module.as_ptr(), module.len() as u32);
    crate::object::js_object_set_field(obj, 0, JSValue::string_ptr(mname));
    crate::object::js_object_set_field(obj, 1, JSValue::number(id as f64));
    let mut keys = crate::array::js_array_alloc(2);
    for k in [b"__module__".as_slice(), b"__v8_id__".as_slice()] {
        let kp = js_string_from_bytes(k.as_ptr(), k.len() as u32);
        keys = crate::array::js_array_push(keys, JSValue::string_ptr(kp));
    }
    crate::object::js_object_set_keys(obj, keys);
    f64::from_bits(JSValue::pointer(obj as *const u8).bits())
}

/// Read the registry id out of a v8 instance object's field[1].
pub(crate) fn v8_instance_id_from_value(val: f64) -> usize {
    let jsv = JSValue::from_bits(val.to_bits());
    if !jsv.is_pointer() {
        return 0;
    }
    unsafe {
        let obj = (val.to_bits() & crate::value::POINTER_MASK) as *mut ObjectHeader;
        let f = crate::object::js_object_get_field(obj, 1);
        if f.is_number() {
            f.as_number() as usize
        } else {
            0
        }
    }
}

/// `new v8.Serializer()` / `new v8.DefaultSerializer()` (the `flag` selects the
/// module tag; both share the same backing codec — our writer already treats
/// ArrayBufferViews as host objects, matching `DefaultSerializer`).
#[no_mangle]
pub extern "C" fn js_v8_serializer_new(default_flag: f64) -> f64 {
    let id = crate::child_process::v8_class_serializer_new();
    // `default_flag` is TAG_TRUE for the `DefaultSerializer` subclass.
    let module = if default_flag.to_bits() == 0x7FFC_0000_0000_0004 {
        "v8.DefaultSerializer"
    } else {
        "v8.Serializer"
    };
    unsafe { build_v8_instance(module, id) }
}

/// `new v8.Deserializer(buffer)` / `new v8.DefaultDeserializer(buffer)`.
#[no_mangle]
pub extern "C" fn js_v8_deserializer_new(buffer: f64) -> f64 {
    let bytes = unsafe { input_bytes(buffer) };
    let Some(bytes) = bytes else {
        crate::fs::validate::throw_type_error_with_code(
            "The \"buffer\" argument must be an instance of Buffer, TypedArray, or DataView.",
            "ERR_INVALID_ARG_TYPE",
        );
    };
    let id = crate::child_process::v8_class_deserializer_new(bytes);
    unsafe { build_v8_instance("v8.Deserializer", id) }
}

/// Wrap a byte vector into a Node `Buffer` value.
fn bytes_to_buffer(bytes: &[u8]) -> f64 {
    let buf = crate::buffer::js_buffer_alloc(bytes.len() as i32, 0);
    if buf.is_null() {
        return undefined();
    }
    unsafe {
        let data = (buf as *mut u8).add(std::mem::size_of::<crate::buffer::BufferHeader>());
        if !bytes.is_empty() {
            std::ptr::copy_nonoverlapping(bytes.as_ptr(), data, bytes.len());
        }
        (*buf).length = bytes.len() as u32;
    }
    f64::from_bits(JSValue::pointer(buf as *const u8).bits())
}

// ── Serializer instance methods (called from dispatch_native_module_method) ──

pub(crate) fn v8_serializer_write_header(recv: f64) -> f64 {
    crate::child_process::v8_class_serializer_write_header(v8_instance_id_from_value(recv));
    undefined()
}

pub(crate) fn v8_serializer_write_value(recv: f64, value: f64) -> f64 {
    crate::child_process::v8_class_serializer_write_value(v8_instance_id_from_value(recv), value);
    // Node returns `true` on success.
    f64::from_bits(0x7FFC_0000_0000_0004)
}

pub(crate) fn v8_serializer_write_uint32(recv: f64, value: f64) -> f64 {
    crate::child_process::v8_class_serializer_write_uint32(
        v8_instance_id_from_value(recv),
        as_u32(value),
    );
    undefined()
}

pub(crate) fn v8_serializer_write_uint64(recv: f64, hi: f64, lo: f64) -> f64 {
    crate::child_process::v8_class_serializer_write_uint64(
        v8_instance_id_from_value(recv),
        as_u32(hi),
        as_u32(lo),
    );
    undefined()
}

pub(crate) fn v8_serializer_write_double(recv: f64, value: f64) -> f64 {
    let n = JSValue::from_bits(value.to_bits());
    let d = if n.is_number() { n.as_number() } else { 0.0 };
    crate::child_process::v8_class_serializer_write_double(v8_instance_id_from_value(recv), d);
    undefined()
}

pub(crate) fn v8_serializer_write_raw_bytes(recv: f64, buffer: f64) -> f64 {
    if let Some(bytes) = unsafe { input_bytes(buffer) } {
        crate::child_process::v8_class_serializer_write_raw_bytes(
            v8_instance_id_from_value(recv),
            &bytes,
        );
    }
    undefined()
}

pub(crate) fn v8_serializer_release_buffer(recv: f64) -> f64 {
    let bytes = crate::child_process::v8_class_serializer_release(v8_instance_id_from_value(recv));
    bytes_to_buffer(&bytes)
}

// ── Deserializer instance methods ──

pub(crate) fn v8_deserializer_read_header(recv: f64) -> f64 {
    crate::child_process::v8_class_deserializer_read_header(v8_instance_id_from_value(recv));
    // Node returns `true`.
    f64::from_bits(0x7FFC_0000_0000_0004)
}

pub(crate) fn v8_deserializer_read_value(recv: f64) -> f64 {
    crate::child_process::v8_class_deserializer_read_value(v8_instance_id_from_value(recv))
}

pub(crate) fn v8_deserializer_read_uint32(recv: f64) -> f64 {
    crate::child_process::v8_class_deserializer_read_uint32(v8_instance_id_from_value(recv)) as f64
}

pub(crate) fn v8_deserializer_read_uint64(recv: f64) -> f64 {
    // Node returns `[hi, lo]`.
    let (hi, lo) =
        crate::child_process::v8_class_deserializer_read_uint64(v8_instance_id_from_value(recv));
    unsafe {
        let arr = crate::array::js_array_alloc(2);
        crate::array::js_array_push_f64(arr, hi as f64);
        crate::array::js_array_push_f64(arr, lo as f64);
        f64::from_bits(JSValue::pointer(arr as *const u8).bits())
    }
}

pub(crate) fn v8_deserializer_read_double(recv: f64) -> f64 {
    crate::child_process::v8_class_deserializer_read_double(v8_instance_id_from_value(recv))
}

pub(crate) fn v8_deserializer_read_raw_bytes(recv: f64, len: f64) -> f64 {
    let n = as_u32(len) as usize;
    let bytes = crate::child_process::v8_class_deserializer_read_raw_bytes(
        v8_instance_id_from_value(recv),
        n,
    );
    bytes_to_buffer(&bytes)
}

/// Coerce a NaN-boxed JS value to a u32 (int32-tagged fast path + double).
fn as_u32(value: f64) -> u32 {
    let bits = value.to_bits();
    if (bits >> 48) == 0x7FFE {
        return (bits & 0xFFFF_FFFF) as u32;
    }
    let jsv = JSValue::from_bits(bits);
    if jsv.is_number() {
        let n = jsv.as_number();
        if n.is_finite() {
            return n as i64 as u32;
        }
    }
    0
}

// ============================================================
// #3679: lifecycle / diagnostic-control namespaces & helpers
// ============================================================
//
// These mirror Node's *shape* only — Perry has no V8 engine to drive real
// startup snapshots, coverage capture, or promise-lifecycle hooks. Top-level
// `v8.setFlagsFromString` / `takeCoverage` / `stopCoverage` are no-op functions
// returning `undefined`; `v8.startupSnapshot` / `v8.promiseHooks` are namespace
// objects whose methods route through `dispatch_native_module_method`.

/// A callable that returns `undefined` (setFlagsFromString / takeCoverage /
/// stopCoverage / promiseHooks.onInit&c. registration → returns a stop fn that
/// is itself this no-op).
#[no_mangle]
pub extern "C" fn js_v8_noop_undefined() -> f64 {
    undefined()
}

/// `v8.startupSnapshot.isBuildingSnapshot()` — Node returns the *number* `0`
/// (never building a snapshot under Perry), NOT a boolean.
#[no_mangle]
pub extern "C" fn js_v8_is_building_snapshot() -> f64 {
    0.0
}

/// Build a `v8.<sub>` native-module namespace object (startupSnapshot /
/// promiseHooks). Method calls on it dispatch through the native-module table.
#[no_mangle]
pub extern "C" fn js_v8_namespace(name_ptr: *const u8, name_len: usize) -> f64 {
    crate::object::js_create_native_module_namespace(name_ptr, name_len)
}

/// `v8.startupSnapshot.addSerializeCallback()` &c. outside a snapshot-building
/// context — Node throws `ERR_NOT_BUILDING_SNAPSHOT`.
#[no_mangle]
pub extern "C" fn js_v8_throw_not_building_snapshot() -> f64 {
    // #3141: Node's `ERR_NOT_BUILDING_SNAPSHOT` is a plain `Error` (name
    // "Error"), not a `TypeError` — use the generic Error-with-code thrower.
    crate::fs::validate::throw_error_with_code(
        "Operation not allowed when not building startup snapshot.",
        "ERR_NOT_BUILDING_SNAPSHOT",
    )
}

/// `v8.promiseHooks.onInit(fn)` &c. — Node returns a callable that removes the
/// hook. We have no real promise-lifecycle hooks, so return a no-op callable so
/// `const stop = onInit(fn); stop()` round-trips.
#[no_mangle]
pub extern "C" fn js_v8_promise_hook_register() -> f64 {
    let c = crate::closure::js_closure_alloc_singleton(js_v8_noop_undefined as *const u8);
    crate::value::js_nanbox_pointer(c as i64)
}

/// `new v8.GCProfiler()` → fresh profiler object.
#[no_mangle]
pub extern "C" fn js_v8_gc_profiler_new() -> f64 {
    unsafe {
        let module = "v8.GCProfiler";
        let obj = crate::object::js_object_alloc(crate::object::NATIVE_MODULE_CLASS_ID, 2);
        let module_name = js_string_from_bytes(module.as_ptr(), module.len() as u32);
        crate::object::js_object_set_field(obj, 0, JSValue::string_ptr(module_name));
        crate::object::js_object_set_field(obj, 1, JSValue::bool(false));

        let mut keys = crate::array::js_array_alloc(1);
        let key = b"__module__";
        let key_ptr = js_string_from_bytes(key.as_ptr(), key.len() as u32);
        keys = crate::array::js_array_push(keys, JSValue::string_ptr(key_ptr));
        crate::object::js_object_set_keys(obj, keys);
        f64::from_bits(JSValue::pointer(obj as *const u8).bits())
    }
}

fn gc_profiler_object(recv: f64) -> Option<*mut ObjectHeader> {
    let value = JSValue::from_bits(recv.to_bits());
    if !value.is_pointer() {
        return None;
    }
    let obj = (recv.to_bits() & crate::value::POINTER_MASK) as *mut ObjectHeader;
    if obj.is_null() {
        return None;
    }
    Some(obj)
}

/// `(new v8.GCProfiler()).start()` → `undefined`.
#[no_mangle]
pub extern "C" fn js_v8_gc_profiler_start(recv: f64) -> f64 {
    if let Some(obj) = gc_profiler_object(recv) {
        unsafe {
            crate::object::js_object_set_field(obj, 1, JSValue::bool(true));
        }
    }
    undefined()
}

/// `(new v8.GCProfiler()).stop()` → report after start, otherwise `undefined`.
#[no_mangle]
pub extern "C" fn js_v8_gc_profiler_stop(recv: f64) -> f64 {
    let Some(obj) = gc_profiler_object(recv) else {
        return undefined();
    };
    let started = unsafe { crate::object::js_object_get_field(obj, 1) };
    if !started.is_bool() || !started.as_bool() {
        return undefined();
    }
    unsafe {
        crate::object::js_object_set_field(obj, 1, JSValue::bool(false));
    }
    js_v8_gc_profiler_report()
}

/// `(new v8.GCProfiler()).stop()` report object.
#[no_mangle]
pub extern "C" fn js_v8_gc_profiler_report() -> f64 {
    let now = crate::date::js_date_now();
    let statistics = crate::array::js_array_alloc(0);
    let stats_val = f64::from_bits(JSValue::pointer(statistics as *const u8).bits());
    unsafe {
        build_object(&[
            ("version", 1.0),
            ("startTime", now),
            ("statistics", stats_val),
            ("endTime", now),
        ])
    }
}
