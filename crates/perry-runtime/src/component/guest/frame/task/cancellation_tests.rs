use super::*;

#[test]
fn c_task_cancel_is_idempotent_and_blocks_all_further_task_execution() {
    crate::gc::gc_init();
    unsafe extern "C" fn count(data: *mut c_void) {
        *data.cast::<u32>() += 1;
    }
    unsafe {
        for operation in 0..4 {
            let task = perry_component_frame_task_new();
            assert_eq!(perry_component_frame_task_cancel(task), 0);
            assert_eq!(perry_component_frame_task_cancel(task), 0);
            assert_eq!(perry_component_frame_failed(task), 0);
            let mut calls = 0u32;
            match operation {
                0 => assert_eq!(perry_component_frame_task_ready(task), -1),
                1 => assert_eq!(perry_component_frame_task_checkpoint(task), -1),
                2 => assert_eq!(
                    perry_component_frame_task_resume(task, Some(count), (&raw mut calls).cast()),
                    -1
                ),
                _ => assert_eq!(perry_component_frame_task_call(task, 0, 0, 0), 0),
            }
            assert_eq!(calls, 0);
            assert_eq!(perry_component_frame_failed(task), 1);
            assert!(job_domain::current().is_none());
            perry_component_frame_drop(task);
        }
        let ordinary = perry_component_frame_new();
        assert_eq!(perry_component_frame_task_cancel(ordinary), -1);
        assert_eq!(perry_component_frame_failed(ordinary), 1);
        perry_component_frame_drop(ordinary);
    }
}

#[test]
fn cancellation_discards_c_frame_jobs_before_drop_and_preserves_the_sibling() {
    let _nursery = crate::gc::CopyingNurseryTestGuard::new(0);
    let _triggers = crate::gc::GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let _evacuate = crate::gc::knob_overrides::ForcedEvacuationTestGuard::on();
    crate::gc::gc_init();
    unsafe {
        let a = perry_component_frame_task_new();
        let b = perry_component_frame_task_new();
        let log = perry_component_frame_array(b, 1);
        let log_a = perry_component_frame_copy(a, b, log);
        frame(a).task_guard(|owner| {
            let scope = RuntimeHandleScope::new();
            let cb = scope.root_raw_mut_ptr(crate::closure::js_closure_alloc(
                crate::fn_info!(unwind super::tests::record_unowned_job, 0),
                1,
            ));
            crate::closure::js_closure_set_capture_f64(cb.get_raw_mut_ptr(), 0, owner.get(log_a));
            crate::builtins::js_queue_microtask(
                cb.get_raw_mut_ptr::<crate::closure::ClosureHeader>() as i64,
            );
        });
        assert_eq!(perry_component_frame_task_ready(a), 1);
        assert_eq!(perry_component_frame_task_ready(b), 0);
        assert_eq!(perry_component_frame_task_cancel(a), 0);
        perry_component_frame_drop(a);
        let before = frame(b).get(log).to_bits();
        crate::gc::gc_collect_minor();
        assert_ne!(before, frame(b).get(log).to_bits());
        assert_eq!(perry_component_frame_task_ready(b), 0);
        assert_eq!(perry_component_frame_task_checkpoint(b), 0);
        assert!(JSValue::from_bits(
            crate::array::js_array_get_f64(array(frame(b).get(log)), 0).to_bits()
        )
        .is_undefined());
        assert_eq!(perry_component_frame_failed(b), 0);
        perry_component_frame_drop(b);
    }
}
