//! `bun:jsc.heapStats` reports Perry's own per-thread heap, without forcing GC.
//!
//! Sizes include arena allocations and tracked malloc GC cells, with headers.
//! Type names come from Perry's GC registry. "Protected" counts approximate
//! native protection using pinned cells; protected globals and external bytes
//! are not separately tracked and report zero. One global context is reported
//! for the calling thread. The compatibility `mimalloc` object contains Perry
//! arena/malloc counters, not measurements from a JavaScriptCore allocator.
use crate::gc::{RuntimeHandle, RuntimeHandleScope};
use crate::object::{js_object_alloc, js_object_set_field_by_name, ObjectHeader};
use crate::string::js_string_from_bytes;
use crate::value::JSValue;

#[cfg(feature = "keepalive-anchors")]
#[used]
static KEEP_BUN_JSC_HEAP_STATS: extern "C" fn(f64) -> f64 = js_bun_jsc_heap_stats;

fn object(scope: &RuntimeHandleScope, capacity: usize) -> RuntimeHandle<'_> {
    scope.root_raw_mut_ptr(js_object_alloc(0, capacity as u32))
}

fn number(target: &RuntimeHandle<'_>, name: &str, value: u64) {
    let key = js_string_from_bytes(name.as_ptr(), name.len() as u32);
    js_object_set_field_by_name(target.get_raw_mut_ptr(), key, value as f64);
}

fn nested(target: &RuntimeHandle<'_>, name: &str, value: &RuntimeHandle<'_>) {
    let key = js_string_from_bytes(name.as_ptr(), name.len() as u32);
    let value = JSValue::pointer(value.get_raw_mut_ptr::<ObjectHeader>().cast());
    js_object_set_field_by_name(target.get_raw_mut_ptr(), key, f64::from_bits(value.bits()));
}

/// Bun's optional compatibility argument is accepted and ignored. Taking a
/// snapshot does not collect; users can call `Bun.gc(true)` before comparing.
#[no_mangle]
pub extern "C" fn js_bun_jsc_heap_stats(_compatibility: f64) -> f64 {
    let stats = crate::gc::heap_stats();
    let scope = RuntimeHandleScope::new();
    let report = object(&scope, 10);
    let types = object(&scope, stats.types.len());
    let protected_types = object(&scope, stats.types.len());
    let allocator = object(&scope, 4);
    for (name, count, pinned) in stats.types {
        number(&types, name, count);
        if pinned != 0 {
            number(&protected_types, name, pinned);
        }
    }
    let used = stats.arena_used.saturating_add(stats.malloc_bytes);
    let capacity = stats
        .arena_reserved
        .saturating_add(stats.malloc_bytes)
        .max(used);
    number(&report, "heapSize", used);
    number(&report, "heapCapacity", capacity);
    number(&report, "extraMemorySize", 0);
    number(&report, "objectCount", stats.object_count);
    number(&report, "protectedObjectCount", stats.pinned_count);
    number(&report, "globalObjectCount", 1);
    number(&report, "protectedGlobalObjectCount", 0);
    nested(&report, "objectTypeCounts", &types);
    nested(&report, "protectedObjectTypeCounts", &protected_types);
    number(&allocator, "arenaUsed", stats.arena_used);
    number(&allocator, "arenaReserved", stats.arena_reserved);
    number(&allocator, "gcMallocBytes", stats.malloc_bytes);
    number(&allocator, "gcMallocObjectCount", stats.malloc_count);
    nested(&report, "mimalloc", &allocator);
    f64::from_bits(JSValue::pointer(report.get_raw_mut_ptr::<ObjectHeader>().cast()).bits())
}
