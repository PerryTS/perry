//! Exercise the actual feature-installed namespace and its traced graph.
use super::super::*;
use super::support::*;

#[test]
fn builtin_namespace_survives_copying_and_is_independent_per_thread() {
    // The first realm mutates abs; the second must still start with its own
    // function. Both execute the lazy bootstrap on a fresh runtime thread.
    for _ in 0..2 {
        std::thread::Builder::new()
            .stack_size(16 << 20)
            .spawn(|| {
                let _guard = CopyingNurseryTestGuard::new(0);
                let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
                register_runtime_handle_root_scanner_for_tests();
                let scope = RuntimeHandleScope::new();
                let math = scope.root_nanbox_f64(crate::object::js_get_global_this_builtin_value(
                    b"Math".as_ptr(),
                    4,
                ));
                let get = |name: &str| unsafe {
                    crate::value::js_dynamic_object_get_property(
                        math.get_nanbox_f64(),
                        name.as_ptr().cast(),
                        name.len(),
                    )
                };
                let abs = scope.root_nanbox_f64(get("abs"));
                assert!(crate::JSValue::from_bits(abs.get_nanbox_u64()).is_pointer());
                let pointer = crate::value::js_nanbox_get_pointer(math.get_nanbox_f64());
                assert_eq!(
                    unsafe {
                        crate::object::object_live_slot_count(
                            pointer as *const crate::object::ObjectHeader,
                        )
                    },
                    45,
                    "subject: bulk Math with all members inline"
                );
                let before = abs.get_nanbox_u64();
                let trace = collect_minor_trace(GcTriggerKind::Direct);
                assert!(
                    trace.copying_nursery.copied_objects > 0,
                    "the collector must copy"
                );
                assert_ne!(
                    before,
                    abs.get_nanbox_u64(),
                    "the method itself must relocate"
                );
                assert_eq!(get("abs").to_bits(), abs.get_nanbox_u64());
                let result = unsafe {
                    crate::closure::call_value(
                        abs.get_nanbox_f64(),
                        crate::closure::plain_call_receiver(),
                        &[-7.0],
                    )
                };
                assert_eq!(result, 7.0);
                assert_eq!(get("PI"), std::f64::consts::PI);
                {
                    let _no_move = GcSuppressScope::new();
                    let key = crate::string::canonical_key(b"abs");
                    let ptr = crate::value::js_nanbox_get_pointer(math.get_nanbox_f64());
                    crate::object::js_object_set_field_by_name(ptr as *mut _, key, 19.0);
                }
                assert_eq!(get("abs"), 19.0);
            })
            .expect("spawn realm")
            .join()
            .expect("realm passed");
    }
}
