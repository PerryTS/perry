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
//! * `v8.GCProfiler` (#3142) — `new v8.GCProfiler()` is represented as the
//!   `"v8.GCProfiler"` native-module namespace; `start()` returns `undefined`
//!   and `stop()` returns a `{ version, startTime, statistics, endTime }`
//!   report object, matching Node's shape.

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
static KEEP_V8_GC_PROFILER_REPORT: extern "C" fn() -> f64 = js_v8_gc_profiler_report;

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
