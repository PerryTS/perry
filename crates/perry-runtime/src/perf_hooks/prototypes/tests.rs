struct TestScope {
    _lock: std::sync::MutexGuard<'static, ()>,
    _no_move: crate::gc::GcSuppressScope,
}

impl TestScope {
    fn new() -> Self {
        Self {
            _lock: crate::gc::global_side_table_test_lock(),
            _no_move: crate::gc::GcSuppressScope::new(),
        }
    }
}

extern "C" fn timerify_target_9202(
    _closure: *const crate::closure::ClosureHeader,
    _this: crate::closure::JsThis,
) -> f64 {
    42.0
}

unsafe fn call_detached(method: f64, args: &[f64]) -> f64 {
    crate::closure::native_call_value_this(
        method,
        crate::closure::JsThis::from_f64(f64::from_bits(crate::value::TAG_UNDEFINED)),
        args.as_ptr(),
        args.len(),
    )
}

/// Read `performance[name]` through ordinary [[Get]]: `performance` has no own
/// keys, so the value comes from `Performance.prototype`.
fn performance_member(performance: f64, name: &str, what: &str) -> f64 {
    let value = unsafe {
        crate::value::js_get_property(performance, name.as_ptr() as i64, name.len() as i64)
    };
    assert!(
        value.to_bits() != crate::value::TAG_UNDEFINED,
        "{what}: performance.{name} must resolve through Performance.prototype"
    );
    value
}

#[test]
fn timerify_and_event_loop_utilization_share_the_native_export_identity() {
    let _scope = TestScope::new();
    let performance = crate::perf_hooks::performance_namespace();

    for name in ["timerify", "eventLoopUtilization"] {
        let prototype_value = performance_member(performance, name, "Performance prototype method");
        let export_value = crate::object::bound_native_callable_export_value("perf_hooks", name);
        assert_eq!(
            prototype_value.to_bits(),
            export_value.to_bits(),
            "Performance.prototype.{name} and perf_hooks.{name} must share one callable"
        );
    }

    // `now` intentionally remains its receiver-checking prototype method,
    // rather than being replaced with the module-bound callable alias.
    let now = performance_member(performance, "now", "Performance.prototype.now");
    let now_export = crate::object::bound_native_callable_export_value("perf_hooks", "now");
    assert_ne!(now.to_bits(), now_export.to_bits());
}

/// `import * as perf from "node:perf_hooks"; perf.monitorEventLoopDelay(...)`
/// reaches the namespace dispatch, not codegen's static lowering: the bucket
/// must create the histograms (it used to end in "not supported").
#[test]
fn histogram_factories_work_through_the_namespace_dispatch() {
    let _scope = TestScope::new();
    let _ = crate::perf_hooks::performance_namespace();
    crate::perf_hook_slots::install();
    for name in ["monitorEventLoopDelay", "createHistogram"] {
        let export_value = crate::object::bound_native_callable_export_value("perf_hooks", name);
        let histogram = unsafe { call_detached(export_value, &[]) };
        assert!(
            crate::value::JSValue::from_bits(histogram.to_bits()).is_pointer(),
            "perf_hooks.{name}() must return its histogram object"
        );
    }
}

#[test]
fn timerify_and_event_loop_utilization_work_when_detached() {
    let _scope = TestScope::new();
    let performance = crate::perf_hooks::performance_namespace();

    let event_loop_utilization = performance_member(
        performance,
        "eventLoopUtilization",
        "Performance.prototype.eventLoopUtilization",
    );
    let elu_result = unsafe { call_detached(event_loop_utilization, &[]) };
    assert!(
        crate::value::JSValue::from_bits(elu_result.to_bits()).is_pointer(),
        "detached eventLoopUtilization must return its result object"
    );

    let timerify = performance_member(performance, "timerify", "Performance.prototype.timerify");
    let target = crate::closure::js_closure_alloc(
        crate::fn_info!(timerify_target_9202, 0; with_declared(0)),
        0,
    );
    let target = crate::value::js_nanbox_pointer(target as i64);
    let wrapper = unsafe { call_detached(timerify, &[target]) };
    let result = unsafe { call_detached(wrapper, &[]) };
    assert_eq!(
        result, 42.0,
        "detached timerify wrapper preserves the result"
    );
}

fn js_str(s: &str) -> f64 {
    crate::value::js_nanbox_string(
        crate::string::js_string_from_bytes(s.as_ptr(), s.len() as u32) as i64,
    )
}

fn resource_entry_count() -> usize {
    let entries = crate::perf_hooks::js_perf_get_entries_by_type(js_str("resource"));
    let arr = crate::value::js_nanbox_get_pointer(entries) as *const crate::array::ArrayHeader;
    crate::array::js_array_length(arr) as usize
}

/// #13964: undici reads `performance.markResourceTiming` detached at module
/// scope and calls it with an undefined receiver for every fetch. Node does
/// not brand-check `markResourceTiming`, so a detached or foreign-receiver call
/// must record the entry instead of throwing `ERR_INVALID_THIS`.
#[test]
fn mark_resource_timing_ignores_its_receiver() {
    let _scope = TestScope::new();
    let performance = crate::perf_hooks::performance_namespace();
    let mark_resource_timing = performance_member(
        performance,
        "markResourceTiming",
        "Performance.prototype.markResourceTiming",
    );

    crate::perf_hooks::js_perf_clear_resource_timings();
    let timing = crate::object::js_object_alloc(0, 2);
    for (key, value) in [("startTime", 0.0), ("endTime", 1.0)] {
        let key = crate::string::js_string_from_bytes(key.as_ptr(), key.len() as u32);
        crate::object::js_object_set_field_by_name(timing, key, value);
    }
    let undefined = f64::from_bits(crate::value::TAG_UNDEFINED);
    let args = [
        crate::value::js_nanbox_pointer(timing as i64),
        js_str("http://x/"),
        js_str("fetch"),
        undefined,
        js_str(""),
        undefined,
        200.0,
        js_str(""),
    ];

    let detached = unsafe { call_detached(mark_resource_timing, &args) };
    assert!(
        crate::value::JSValue::from_bits(detached.to_bits()).is_pointer(),
        "detached markResourceTiming must return the resource entry"
    );

    let foreign = crate::value::js_nanbox_pointer(crate::object::js_object_alloc(0, 0) as i64);
    let foreign_call = unsafe {
        crate::closure::native_call_value_this(
            mark_resource_timing,
            crate::closure::JsThis::from_f64(foreign),
            args.as_ptr(),
            args.len(),
        )
    };
    assert!(
        crate::value::JSValue::from_bits(foreign_call.to_bits()).is_pointer(),
        "markResourceTiming with a non-Performance receiver must return the resource entry"
    );

    assert_eq!(
        resource_entry_count(),
        2,
        "both calls buffer a resource entry"
    );
    crate::perf_hooks::js_perf_clear_resource_timings();
}
