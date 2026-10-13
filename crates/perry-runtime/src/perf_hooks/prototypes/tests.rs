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

#[test]
fn timerify_and_event_loop_utilization_share_the_native_export_identity() {
    let _scope = TestScope::new();
    let performance = crate::perf_hooks::performance_namespace();

    for name in ["timerify", "eventLoopUtilization"] {
        let prototype_value = crate::object::native_module::performance_namespace_method(
            "perf_hooks",
            name,
            performance,
        )
        .expect("Performance prototype method");
        let export_value = crate::object::bound_native_callable_export_value("perf_hooks", name);
        assert_eq!(
            prototype_value.to_bits(),
            export_value.to_bits(),
            "Performance.prototype.{name} and perf_hooks.{name} must share one callable"
        );
    }

    // `now` intentionally remains its receiver-checking prototype method,
    // rather than being replaced with the module-bound callable alias.
    let now = crate::object::native_module::performance_namespace_method(
        "perf_hooks",
        "now",
        performance,
    )
    .expect("Performance.prototype.now");
    let now_export = crate::object::bound_native_callable_export_value("perf_hooks", "now");
    assert_ne!(now.to_bits(), now_export.to_bits());
}

#[test]
fn timerify_and_event_loop_utilization_work_when_detached() {
    let _scope = TestScope::new();
    let performance = crate::perf_hooks::performance_namespace();

    let event_loop_utilization = crate::object::native_module::performance_namespace_method(
        "perf_hooks",
        "eventLoopUtilization",
        performance,
    )
    .expect("Performance.prototype.eventLoopUtilization");
    let elu_result = unsafe { call_detached(event_loop_utilization, &[]) };
    assert!(
        crate::value::JSValue::from_bits(elu_result.to_bits()).is_pointer(),
        "detached eventLoopUtilization must return its result object"
    );

    let timerify = crate::object::native_module::performance_namespace_method(
        "perf_hooks",
        "timerify",
        performance,
    )
    .expect("Performance.prototype.timerify");
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
