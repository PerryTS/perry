//! `node:v8` GCProfiler report surface.
//!
//! The bulk of the `node:v8` public API (serialize/deserialize, heap stats,
//! `Serializer`/`Deserializer` classes, `cachedDataVersionTag`) lives in
//! `crate::v8`. This module hosts only the `v8.GCProfiler` report helper
//! (#3142): `new v8.GCProfiler()` is represented as the `"v8.GCProfiler"`
//! native-module namespace; `start()` returns `undefined` and `stop()`
//! returns a `{ version, startTime, statistics, endTime }` report object,
//! matching Node's shape.

use crate::string::js_string_from_bytes;
use crate::value::JSValue;

// Symbol retention: this `#[no_mangle]` entry point is emitted only by
// codegen's `node:v8` dispatch — no Rust caller references it, so the
// auto-optimize whole-program-LLVM build would dead-strip it without an
// anchor (see node_stream_keepalive.rs). Pin it via a `#[used]` static.
#[used]
static KEEP_V8_GC_PROFILER_REPORT: extern "C" fn() -> f64 = js_v8_gc_profiler_report;

/// Build a plain object from `(name, value)` numeric/any pairs.
unsafe fn build_object(pairs: &[(&str, f64)]) -> f64 {
    let obj = crate::object::js_object_alloc(0, pairs.len() as u32);
    for (name, value) in pairs {
        let key = js_string_from_bytes(name.as_ptr(), name.len() as u32);
        crate::object::js_object_set_field_by_name(obj, key, *value);
    }
    f64::from_bits(JSValue::pointer(obj as *const u8).bits())
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
