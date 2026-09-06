//! Receiver-aware `Performance.prototype` method thunks.

use super::*;

fn require_performance_receiver() {
    if !is_performance_namespace_value(crate::object::js_implicit_this_get()) {
        invalid_perf_receiver("Performance");
    }
}

pub(super) extern "C" fn clear_marks(
    _closure: *const crate::closure::ClosureHeader,
    name: f64,
) -> f64 {
    require_performance_receiver();
    js_perf_clear_marks(name)
}

pub(super) extern "C" fn clear_measures(
    _closure: *const crate::closure::ClosureHeader,
    name: f64,
) -> f64 {
    require_performance_receiver();
    js_perf_clear_measures(name)
}

pub(super) extern "C" fn clear_resource_timings(
    _closure: *const crate::closure::ClosureHeader,
) -> f64 {
    require_performance_receiver();
    js_perf_clear_resource_timings()
}

pub(super) extern "C" fn get_entries(_closure: *const crate::closure::ClosureHeader) -> f64 {
    require_performance_receiver();
    js_perf_get_entries()
}

pub(super) extern "C" fn get_entries_by_name(
    _closure: *const crate::closure::ClosureHeader,
    name: f64,
    entry_type: f64,
) -> f64 {
    require_performance_receiver();
    js_perf_get_entries_by_name(name, entry_type)
}

pub(super) extern "C" fn get_entries_by_type(
    _closure: *const crate::closure::ClosureHeader,
    entry_type: f64,
) -> f64 {
    require_performance_receiver();
    js_perf_get_entries_by_type(entry_type)
}

pub(super) extern "C" fn mark(
    _closure: *const crate::closure::ClosureHeader,
    name: f64,
    options: f64,
) -> f64 {
    require_performance_receiver();
    js_perf_mark(name, options)
}

pub(super) extern "C" fn measure(
    _closure: *const crate::closure::ClosureHeader,
    name: f64,
    start_or_options: f64,
    end: f64,
) -> f64 {
    require_performance_receiver();
    js_perf_measure(name, start_or_options, end)
}

pub(super) extern "C" fn now(_closure: *const crate::closure::ClosureHeader) -> f64 {
    require_performance_receiver();
    crate::date::js_performance_now()
}

pub(super) extern "C" fn set_resource_timing_buffer_size(
    _closure: *const crate::closure::ClosureHeader,
    size: f64,
) -> f64 {
    require_performance_receiver();
    js_perf_set_resource_timing_buffer_size(size)
}

pub(super) extern "C" fn to_json(_closure: *const crate::closure::ClosureHeader) -> f64 {
    require_performance_receiver();
    js_perf_to_json()
}

pub(super) extern "C" fn event_loop_utilization(
    _closure: *const crate::closure::ClosureHeader,
    utilization1: f64,
    utilization2: f64,
) -> f64 {
    require_performance_receiver();
    js_perf_event_loop_utilization(utilization1, utilization2)
}

pub(super) extern "C" fn mark_resource_timing(
    _closure: *const crate::closure::ClosureHeader,
    timing_info: f64,
    requested_url: f64,
    initiator_type: f64,
    global: f64,
    cache_mode: f64,
    body_info: f64,
    response_status: f64,
    delivery_type: f64,
) -> f64 {
    require_performance_receiver();
    js_perf_mark_resource_timing(
        timing_info,
        requested_url,
        initiator_type,
        global,
        cache_mode,
        body_info,
        response_status,
        delivery_type,
    )
}

pub(super) extern "C" fn timerify(
    _closure: *const crate::closure::ClosureHeader,
    function: f64,
    options: f64,
) -> f64 {
    require_performance_receiver();
    js_perf_timerify(function, options)
}
