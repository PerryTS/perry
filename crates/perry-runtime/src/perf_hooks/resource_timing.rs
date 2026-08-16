//! `performance.markResourceTiming()` and the resource-timing buffer controls.
//!
//! Split out of `perf_hooks.rs` to keep that file under the 2k-LOC gate
//! (`scripts/check_file_size.sh`). Everything here reaches the timeline, the
//! entry shapes and the throw helpers through `use super::*`.

use super::*;

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
    cache_mode: f64,
    _body_info: f64,
    response_status: f64,
    delivery_type: f64,
) -> f64 {
    unsafe {
        let Some(timing_obj) = as_object_ptr(timing_info) else {
            throw_type_error_with_code(
                "The \"timingInfo\" argument must be of type object",
                "ERR_INVALID_ARG_TYPE",
            );
        };
        // Node asserts the cache mode is one of the two fetch-spec values
        // before reading anything else, and an assertion failure surfaces as
        // `Error [ERR_INTERNAL_ASSERTION]`, not a TypeError.
        let cache_mode_jv = JSValue::from_bits(cache_mode.to_bits());
        let local_cache = if cache_mode_jv.is_undefined() {
            false
        } else {
            match string_of(cache_mode_jv).as_deref() {
                Some("") => false,
                Some("local") => true,
                _ => crate::fs::validate::throw_error_with_code(
                    "The cacheMode argument must be an empty string or 'local'",
                    "ERR_INTERNAL_ASSERTION",
                ),
            }
        };
        let name = coerce_to_string(requested_url);
        let initiator = coerce_to_string(initiator_type);
        let start_time = option_number(timing_obj, "startTime")
            .or_else(|| option_number(timing_obj, "fetchStart"))
            .unwrap_or(0.0);
        let end_time = option_number(timing_obj, "endTime");
        let duration = end_time.map(|end| end - start_time).unwrap_or(f64::NAN);
        let encoded_body_size = option_value(timing_obj, "encodedBodySize");
        let transfer_size = if local_cache {
            JSValue::number(0.0)
        } else {
            // Node adds the fetch spec's fixed 300-byte header allowance.
            JSValue::number(num_of(encoded_body_size).unwrap_or(f64::NAN) + 300.0)
        };
        let connection = as_object_ptr(f64::from_bits(
            option_value(timing_obj, "finalConnectionTimingInfo").bits(),
        ));
        let connection_field = |key: &str| -> JSValue {
            match connection {
                Some(obj) => option_value(obj, key),
                None => JSValue::undefined(),
            }
        };
        let entry = PerfEntry {
            name: name.clone(),
            entry_type: ENTRY_TYPE_RESOURCE,
            start_time,
            duration,
            detail_bits: JSValue::null().bits(),
            object_bits: 0,
            initiator_type: Some(initiator.clone()),
        };
        let mut entry = entry;
        let obj = {
            let out = js_object_alloc_with_shape(
                RESOURCE_ENTRY_SHAPE,
                RESOURCE_ENTRY_FIELD_COUNT,
                RESOURCE_ENTRY_KEYS.as_ptr(),
                RESOURCE_ENTRY_KEYS.len() as u32,
            );
            RESOURCE_ENTRY_KEYS_ARRAY.with(|c| {
                if c.get() == 0 {
                    c.set((*out).keys_array as usize);
                }
            });
            let fields: [JSValue; RESOURCE_ENTRY_FIELD_COUNT as usize] = [
                str_value(&name),
                str_value(entry_type_name(ENTRY_TYPE_RESOURCE)),
                JSValue::number(start_time),
                JSValue::number(duration),
                str_value(&initiator),
                connection_field("ALPNNegotiatedProtocol"),
                option_value(timing_obj, "finalServiceWorkerStartTime"),
                option_value(timing_obj, "redirectStartTime"),
                option_value(timing_obj, "redirectEndTime"),
                option_value(timing_obj, "postRedirectStartTime"),
                connection_field("domainLookupStartTime"),
                connection_field("domainLookupEndTime"),
                connection_field("connectionStartTime"),
                connection_field("connectionEndTime"),
                connection_field("secureConnectionStartTime"),
                option_value(timing_obj, "finalNetworkRequestStartTime"),
                option_value(timing_obj, "finalNetworkResponseStartTime"),
                option_value(timing_obj, "endTime"),
                transfer_size,
                encoded_body_size,
                option_value(timing_obj, "decodedBodySize"),
                JSValue::from_bits(response_status.to_bits()),
                JSValue::from_bits(delivery_type.to_bits()),
            ];
            for (i, v) in fields.iter().enumerate() {
                js_object_set_field(out, i as u32, *v);
            }
            crate::value::js_nanbox_pointer(out as i64)
        };
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
