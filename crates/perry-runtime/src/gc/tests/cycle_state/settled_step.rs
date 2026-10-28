use super::*;

extern "C" fn step(
    _: *const crate::closure::ClosureHeader,
    _: crate::closure::JsThis,
    value: f64,
    _: f64,
) -> f64 {
    value
}

#[test]
fn settled_step_queue_publication_after_root_scan_marks_payload() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let _trigger = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let scope = RuntimeHandleScope::new();
    let step = scope.root_raw_mut_ptr(crate::closure::js_closure_alloc(
        crate::fn_info!(step, 2),
        0,
    ));
    let next = scope.root_raw_mut_ptr(crate::promise::js_promise_new());
    let child = gc_malloc(
        std::mem::size_of::<crate::closure::ClosureHeader>(),
        GC_TYPE_CLOSURE,
    );
    unsafe {
        init_test_closure(child);
    }
    let inner = crate::promise::js_promise_new();
    // GC_STORE_AUDIT(INIT): initialize a fresh unexposed Promise before the manual cycle; automatic triggers are suppressed.
    unsafe {
        (*inner).state = crate::promise::PromiseState::Fulfilled;
        (*inner).value = f64::from_bits(ptr_bits(child as usize));
    }
    let previous = crate::promise::INLINE_TRAP.with(|slot| {
        slot.replace(crate::promise::InlineTrap {
            trap_next: next.get_raw_mut_ptr(),
            current_step: step.get_raw_mut_ptr::<crate::closure::ClosureHeader>() as usize,
            box_activation: std::ptr::null_mut(),
        })
    });
    struct Restore(crate::promise::InlineTrap);
    impl Drop for Restore {
        fn drop(&mut self) {
            crate::promise::TASK_QUEUE.with(|queue| queue.borrow_mut().clear());
            crate::promise::INLINE_TRAP.with(|slot| slot.set(self.0));
        }
    }
    let _restore = Restore(previous);
    let mut state = GcCycleState::new_full(trace_snapshot(GcTriggerKind::Manual));
    run_cycle_until_phase(&mut state, GcCyclePhase::BlockPersistence);
    assert!(incremental_mark_barrier_active());
    assert_eq!(
        unsafe { (*header_from_user_ptr(child)).gc_flags & GC_FLAG_MARKED },
        0,
        "payload must start white after the queue root scan"
    );
    let got = crate::promise::js_async_step_chain(
        crate::value::js_nanbox_pointer(inner as i64),
        step.get_raw_mut_ptr(),
    );
    assert_eq!(got, next.get_raw_mut_ptr());
    assert_marked_user_ptr(child as usize, "ordinary async queue publication");
    run_cycle_in_single_unit_steps(&mut state);
    assert!(
        malloc_user_ptr_tracked(child),
        "queued payload must be marked after the queue root scan already passed"
    );
    crate::promise::TASK_QUEUE.with(|queue| {
        assert!(matches!(queue.borrow().front(), Some(crate::promise::Task::AsyncStep(_, value, _, _, _, _, _, _)) if value.to_bits() == ptr_bits(child as usize)));
    });
}
