//! Conservative eligibility for an outer entry checkpoint with no callbacks.
//! This is separate from event-loop liveness: unref/cleared timer entries still
//! need their normal phase, as do rejection reports without a queued Promise.

use super::*;

pub(super) fn can_skip_callback_phases() -> bool {
    if MICROTASK_RUN_DEPTH.with(|depth| depth.get().pump != 0 || depth.get().jobs != 0)
        || ESM_EVAL_CHECKPOINT_PENDING.with(|pending| pending.get())
        || async_box_execution_ref_depth() != 0
    {
        return false;
    }
    if TASK_QUEUE.with(|queue| !queue.borrow().is_empty())
        || crate::builtins::queued_microtasks_pending()
        || super::super::combinators::SCHEDULED_RESOLVES.with(|queue| !queue.borrow().is_empty())
        || super::super::rejection::checkpoint_work_pending()
        || crate::weakref::pending_finalization_jobs_count() != 0
    {
        return false;
    }
    // The foreign queues are checked under their own locks. Future arrivals
    // retain their existing active-handle/wakeup contracts; a callback arriving
    // after this check is handled by the next pump, just as one arriving after
    // the ordinary drain. Conservatively use the full path for active workers.
    !(super::super::native_async::completion_work_pending()
        || crate::thread::js_thread_has_pending() != 0
        || crate::bun_ffi::callback::threadsafe_callback_work_pending()
        || crate::async_hooks::gc_destroy_work_pending()
        || crate::node_submodules::diagnostics_channel_has_pending_publishes()
        || crate::node_submodules::diagnostics::uncaught_work_pending()
        || crate::os::process_stdin_needs_pump()
        || crate::timer::timer_phase_work_pending())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn isolated(test: impl FnOnce() + Send + 'static) {
        std::thread::spawn(|| {
            crate::gc::js_gc_init();
            test();
        })
        .join()
        .unwrap();
    }

    #[test]
    fn empty_checkpoint_avoids_execution_stack_allocation() {
        isolated(|| {
            assert!(can_skip_callback_phases());
            assert_eq!(js_promise_run_microtasks_event_loop(), 0);
            assert_eq!(js_promise_run_before_exit_checkpoint(), 0);
            assert_eq!(js_promise_run_promise_jobs(), 0);
            // The full runner's push allocates this Vec. Verify the branch
            // actually runs, independently of testing the guard itself.
            ASYNC_BOX_EXECUTION_REF_BASES.with(|bases| assert_eq!(bases.borrow().capacity(), 0));
            assert!(!microtask_drain_active());
            assert!(!microtask_job_drain_active());
        });
    }

    #[test]
    fn esm_evaluation_checkpoint_uses_full_path_once() {
        isolated(|| {
            js_mark_entry_module_esm();
            assert!(!can_skip_callback_phases());
            assert_eq!(js_promise_run_microtasks_event_loop(), 0);
            assert!(can_skip_callback_phases());
        });
    }

    #[test]
    fn queued_microtask_runs_and_restores_quiescence() {
        isolated(|| {
            static CALLED: AtomicU64 = AtomicU64::new(0);
            extern "C" fn callback(_: *const crate::closure::ClosureHeader) -> f64 {
                CALLED.fetch_add(1, Ordering::Relaxed);
                0.0
            }
            crate::closure::js_register_closure_arity(callback as *const u8, 0);
            let callback = crate::closure::js_closure_alloc(callback as *const u8, 0);
            crate::builtins::js_queue_microtask(callback as i64);
            assert!(!can_skip_callback_phases());
            assert!(js_promise_run_microtasks_event_loop() > 0);
            assert_eq!(CALLED.load(Ordering::Relaxed), 1);
            assert!(!microtask_drain_active());
            assert!(!microtask_job_drain_active());
        });
    }

    #[test]
    fn scheduled_resolution_without_a_task_is_not_empty() {
        isolated(|| {
            let promise = crate::promise::js_promise_new();
            let roots = crate::gc::RuntimeHandleScope::new();
            let promise = roots.root_nanbox_f64(crate::value::js_nanbox_pointer(promise as i64));
            crate::promise::js_promise_schedule_resolve(
                crate::value::js_nanbox_get_pointer(promise.get_nanbox_f64()) as *mut Promise,
                13.0,
            );
            assert!(!can_skip_callback_phases());
            assert!(js_promise_run_microtasks_event_loop() > 0);
            let promise =
                crate::value::js_nanbox_get_pointer(promise.get_nanbox_f64()) as *mut Promise;
            assert_eq!(crate::promise::js_promise_state(promise), 1);
            assert_eq!(crate::promise::js_promise_result(promise), 13.0);
        });
    }

    #[test]
    fn rejection_without_a_task_requires_checkpoint() {
        isolated(|| {
            let promise = crate::promise::js_promise_new();
            crate::promise::js_promise_reject(promise, 23.0);
            assert!(TASK_QUEUE.with(|queue| queue.borrow().is_empty()));
            assert!(!can_skip_callback_phases());
            crate::promise::js_promise_mark_internally_handled(promise);
            assert!(!super::super::super::rejection::checkpoint_work_pending());
        });
    }

    #[test]
    fn unref_timer_still_requires_its_phase() {
        isolated(|| {
            crate::timer::js_set_timeout_value_ref(1_000_000.0, 0.0, 0);
            assert_eq!(crate::timer::js_timer_has_pending(), 0);
            assert!(!can_skip_callback_phases());
            crate::timer::purge_agent_timers(crate::agent::current_agent());
            assert!(!crate::timer::timer_phase_work_pending());
        });
    }

    #[test]
    fn gc_destroy_jobs_are_not_skipped() {
        isolated(|| {
            crate::async_hooks::enqueue_gc_destroy(u64::MAX - 1);
            assert!(!can_skip_callback_phases());
            assert!(js_promise_run_microtasks_event_loop() > 0);
            assert!(!crate::async_hooks::gc_destroy_work_pending());
        });
    }

    #[test]
    fn native_completion_arriving_after_empty_checkpoint_is_delivered() {
        isolated(|| {
            use crate::promise::native_async::*;
            let _guard = test_native_async_lock();
            test_reset_native_async_registry();
            assert_eq!(js_promise_run_microtasks_event_loop(), 0);
            let token = js_native_async_completion_new(0);
            let promise = js_native_async_completion_promise(token);
            let token_addr = token as usize;
            std::thread::spawn(move || {
                assert_eq!(
                    js_native_async_completion_resolve_bits(
                        token_addr as *mut NativeAsyncCompletion,
                        37.0f64.to_bits(),
                    ),
                    PERRY_NATIVE_ASYNC_OK,
                );
            })
            .join()
            .unwrap();
            assert!(!can_skip_callback_phases());
            assert_eq!(js_promise_run_before_exit_checkpoint(), 0);
            assert_eq!(crate::promise::js_promise_state(promise), 0);
            assert!(js_promise_run_microtasks_event_loop() > 0);
            assert_eq!(crate::promise::js_promise_state(promise), 1);
            assert_eq!(crate::promise::js_promise_value(promise), 37.0);
            test_reset_native_async_registry();
        });
    }

    #[test]
    fn buffered_stdin_is_delivered_without_any_timer() {
        isolated(|| {
            static CALLED: AtomicU64 = AtomicU64::new(0);
            extern "C" fn callback(_: *const crate::closure::ClosureHeader, _: f64) -> f64 {
                CALLED.fetch_add(1, Ordering::Relaxed);
                0.0
            }
            crate::closure::js_register_closure_arity(callback as *const u8, 1);
            let callback = crate::closure::js_closure_alloc(callback as *const u8, 0);
            crate::os::test_set_stdin_data_listener(Some(callback as i64));
            crate::os::stdin_push_bytes(b"input");
            assert!(!crate::timer::timer_phase_work_pending());
            assert!(!can_skip_callback_phases());
            js_promise_run_microtasks_event_loop();
            assert_eq!(CALLED.load(Ordering::Relaxed), 1);
            assert!(!crate::os::process_stdin_needs_pump());
            crate::os::test_set_stdin_data_listener(None);
        });
    }

    #[test]
    fn before_exit_checkpoint_preserves_timer_for_the_next_turn() {
        isolated(|| {
            let scope = crate::gc::RuntimeHandleScope::new();
            let promise = scope.root_nanbox_f64(crate::value::js_nanbox_pointer(
                crate::timer::js_set_timeout_value_ref(0.0, 47.0, 1) as i64,
            ));
            assert_eq!(js_promise_run_before_exit_checkpoint(), 0);
            assert_eq!(
                crate::promise::js_promise_state(rooted_promise(&promise)),
                0
            );
            assert_eq!(crate::timer::js_timer_has_pending(), 1);
            assert!(js_promise_run_microtasks_event_loop() > 0);
            assert_eq!(
                crate::promise::js_promise_value(rooted_promise(&promise)),
                47.0
            );
        });
    }

    #[test]
    fn exit_promise_checkpoint_leaves_ticks_forbidden() {
        isolated(|| {
            static CALLED: AtomicU64 = AtomicU64::new(0);
            extern "C" fn callback(_: *const crate::closure::ClosureHeader) -> f64 {
                CALLED.fetch_add(1, Ordering::Relaxed);
                0.0
            }
            crate::closure::js_register_closure_arity(callback as *const u8, 0);
            let callback = crate::closure::js_closure_alloc(callback as *const u8, 0);
            crate::builtins::js_queue_next_tick(callback as i64);
            assert_eq!(js_promise_run_promise_jobs(), 0);
            assert_eq!(CALLED.load(Ordering::Relaxed), 0);
            assert!(crate::builtins::queued_microtasks_pending());
            assert!(js_promise_run_before_exit_checkpoint() > 0);
            assert_eq!(CALLED.load(Ordering::Relaxed), 1);
            assert!(!crate::builtins::queued_microtasks_pending());
        });
    }
}
