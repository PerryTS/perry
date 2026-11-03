use super::*;
use crate::closure::{ClosureHeader, JsThis};

extern "C" fn resume(_: *const ClosureHeader, _: JsThis, value: f64, _: f64) -> f64 {
    value
}

fn boxed<T>(p: *const T) -> f64 {
    f64::from_bits(owner_bits(p))
}

fn fixture(f: impl FnOnce(ClosurePtr, *mut Promise, *mut Promise) + Send + 'static) {
    std::thread::spawn(move || {
        crate::gc::gc_init();
        let _no_move = crate::gc::GcSuppressScope::new();
        let scope = crate::gc::RuntimeHandleScope::new();
        let step = scope.root_raw_mut_ptr(crate::closure::js_closure_alloc(
            crate::fn_info!(resume, 2),
            0,
        ));
        let next = scope.root_raw_mut_ptr(js_promise_new());
        let inner = scope.root_raw_mut_ptr(js_promise_resolved(19.0));
        let previous = INLINE_TRAP.with(|slot| {
            slot.replace(InlineTrap {
                trap_next: next.get_raw_mut_ptr(),
                current_step: step.get_raw_mut_ptr::<crate::closure::ClosureHeader>() as usize,
                box_activation: std::ptr::null_mut(),
            })
        });
        struct Reset(InlineTrap);
        impl Drop for Reset {
            fn drop(&mut self) {
                TASK_QUEUE.with(|queue| queue.borrow_mut().clear());
                INLINE_TRAP.with(|slot| slot.set(self.0));
            }
        }
        let _reset = Reset(previous);
        f(
            step.get_raw_mut_ptr(),
            next.get_raw_mut_ptr(),
            inner.get_raw_mut_ptr(),
        );
    })
    .join()
    .unwrap();
}

#[test]
fn settled_step_publishes_pointer_payload_after_existing_fifo_job() {
    fixture(|step, next, _| {
        let storage = crate::async_context::AsyncLocalStoragePayload::default();
        crate::async_context::set_store(storage.token(), 33.0);
        let object = crate::object::js_object_alloc(0, 0);
        let value = boxed(object);
        let inner = js_promise_resolved(value);
        unsafe {
            (*next).async_id = 41;
            (*next).trigger_async_id = 7;
        }
        TASK_QUEUE.with(|queue| {
            queue.borrow_mut().push_back(Task::Inline(
                std::ptr::null(),
                -1.0,
                next,
                true,
                capture_context(),
            ))
        });
        assert_eq!(try_enqueue(boxed(inner), step), Some(next));
        TASK_QUEUE.with(|queue| {
            let mut queue = queue.borrow_mut();
            assert!(matches!(
                queue.pop_front(),
                Some(Task::Inline(_, -1.0, _, _, _))
            ));
            let Some(Task::AsyncStep(owner, payload, result, error, context, _, id, trigger)) =
                queue.pop_front()
            else {
                panic!("missing ordinary async FIFO job")
            };
            assert_eq!(owner, step);
            assert_eq!(payload.to_bits(), value.to_bits());
            assert_eq!(result, next);
            assert!(!error);
            assert_eq!((id, trigger), (41, 7));
            let previous = crate::async_context::enter_owned_context(context);
            assert_eq!(crate::async_context::get_store(storage.token()), Some(33.0));
            crate::async_context::restore_context(previous);
            assert!(queue.is_empty());
        });
        crate::async_context::clear_store(storage.token());
    });
}

#[test]
fn settled_step_refuses_first_call_and_different_activation_owner() {
    fixture(|step, next, inner| {
        INLINE_TRAP.with(|slot| {
            let mut trap = slot.get();
            trap.trap_next = std::ptr::null_mut();
            slot.set(trap);
        });
        assert!(
            try_enqueue(boxed(inner), step).is_none(),
            "first call needs its own result promise"
        );
        let other = crate::closure::js_closure_alloc(crate::fn_info!(resume, 2), 0);
        INLINE_TRAP.with(|slot| {
            let mut trap = slot.get();
            trap.trap_next = next;
            trap.current_step = other as usize;
            slot.set(trap);
        });
        assert!(
            try_enqueue(boxed(inner), step).is_none(),
            "same code does not prove same activation"
        );
    });
}

#[test]
fn settled_step_requires_real_promise_brand() {
    fixture(|step, _, inner| unsafe {
        let header = inner
            .cast::<u8>()
            .sub(crate::gc::GC_HEADER_SIZE)
            .cast::<crate::gc::GcHeader>();
        struct Restore(*mut crate::gc::GcHeader, u8);
        impl Drop for Restore {
            fn drop(&mut self) {
                unsafe {
                    (*self.0).obj_type = self.1;
                }
            }
        }
        let _restore = Restore(header, (*header).obj_type);
        // The fixture suppresses collection while this otherwise valid Promise
        // payload has a non-Promise brand; teardown restores the real header.
        (*header).obj_type = crate::gc::GC_TYPE_OBJECT;
        assert!(try_enqueue(boxed(inner), step).is_none());
    });
}

#[test]
fn settled_step_requires_fulfilled_state() {
    fixture(|step, _, inner| unsafe {
        for state in [PromiseState::Pending, PromiseState::Rejected] {
            (*inner).state = state;
            assert!(try_enqueue(boxed(inner), step).is_none());
        }
    });
}

#[test]
fn settled_step_declines_recorded_prototype_metadata() {
    fixture(|step, _, inner| {
        crate::object::prototype_chain::object_set_static_prototype(
            inner as usize,
            crate::value::TAG_NULL,
        );
        assert!(unsafe { !(*inner).meta.is_null() });
        assert!(try_enqueue(boxed(inner), step).is_none());
    });
}

extern "C" fn init(_: *const ClosureHeader, _: JsThis, _: f64, _: f64, _: f64, _: f64) -> f64 {
    f64::from_bits(crate::value::TAG_UNDEFINED)
}

#[test]
fn settled_step_observed_hooks_keep_collecting_route() {
    fixture(|step, _, inner| {
        crate::async_hook_slots::install();
        let options = crate::object::js_object_alloc(0, 0);
        let callback = crate::closure::js_closure_alloc(crate::fn_info!(init, 4), 0);
        let key = crate::string::js_string_from_bytes(b"init".as_ptr(), 4);
        crate::object::js_object_set_field_by_name(options, key, boxed(callback));
        let hook = crate::async_hooks::js_async_hooks_create_hook(boxed(options));
        struct Disable(f64);
        impl Drop for Disable {
            fn drop(&mut self) {
                crate::async_hooks::js_async_hook_disable(crate::value::js_nanbox_get_pointer(
                    self.0,
                ));
            }
        }
        let _disable = Disable(hook);
        crate::async_hooks::js_async_hook_enable(crate::value::js_nanbox_get_pointer(hook));
        assert!(crate::async_hook_slots::promise_hooks_active());
        assert!(try_enqueue(boxed(inner), step).is_none());
    });
}

#[test]
fn settled_step_notifies_producers_outside_the_job_drain() {
    fixture(|step, next, inner| {
        use std::sync::atomic::{AtomicUsize, Ordering};
        unsafe extern "C" fn wake(context: *mut std::ffi::c_void) {
            (*(context as *const AtomicUsize)).fetch_add(1, Ordering::SeqCst);
        }
        struct Reset;
        impl Drop for Reset {
            fn drop(&mut self) {
                unsafe { crate::event_pump::perry_set_wake_callback(None, std::ptr::null_mut()) };
            }
        }
        let count = AtomicUsize::new(0);
        let _reset = Reset;
        unsafe {
            crate::event_pump::perry_set_wake_callback(
                Some(wake),
                (&count as *const AtomicUsize).cast_mut().cast(),
            );
        }
        assert!(!super::super::microtasks::microtask_job_drain_active());
        assert_eq!(try_enqueue(boxed(inner), step), Some(next));
        assert_eq!(
            count.load(Ordering::SeqCst),
            1,
            "published work wakes the host"
        );
    });
}

// ---- SNC (settled self-next continuation): one decline test per predicate ----
// Each test starts from the same admitted state and breaks exactly one
// condition. A declined probe must change nothing observable.

const TAG_TDZ_BITS: u64 = 0x7FFC_0000_0000_0011;

fn snc_arm(step: ClosurePtr) {
    super::microtasks::CURRENT_MICROTASK_CALLBACK.with(|c| c.set(step));
}

struct SncReset;
impl Drop for SncReset {
    fn drop(&mut self) {
        super::microtasks::CURRENT_MICROTASK_CALLBACK.with(|c| c.set(std::ptr::null()));
    }
}

fn admitted(value: f64, step: ClosurePtr) -> bool {
    js_async_step_resume_now(value, step).to_bits() != TAG_TDZ_BITS
}

/// The payload an admitted probe hands back (the decline sentinel otherwise).
fn payload(value: f64, step: ClosurePtr) -> u64 {
    js_async_step_resume_now(value, step).to_bits()
}

fn cell_value() -> u64 {
    js_iter_result_get_value().to_bits()
}

#[test]
fn snc_admits_primitive_and_native_fulfilled_and_writes_payload() {
    fixture(|step, _, inner| {
        let _reset = SncReset;
        snc_arm(step);
        js_iter_result_set(-1.0, 1);
        assert_eq!(f64::from_bits(payload(5.0, step)), 5.0);
        assert_eq!(f64::from_bits(payload(boxed(inner), step)), 19.0);
        assert_eq!(
            f64::from_bits(cell_value()),
            -1.0,
            "the payload travels by value, not through the iter-result cell"
        );
        assert!(
            TASK_QUEUE.with(|q| q.borrow().is_empty()),
            "SNC never publishes"
        );
    });
}

#[test]
fn snc_p1_requires_own_step_job() {
    fixture(|step, _, _| {
        let _reset = SncReset;
        js_iter_result_set(-1.0, 0);
        // not inside any microtask job (first call / thunk)
        assert!(!admitted(5.0, step));
        // a different callback owns the current job
        let other = crate::closure::js_closure_alloc(crate::fn_info!(resume, 2), 0);
        snc_arm(other);
        assert!(!admitted(5.0, step));
        assert!(!admitted(5.0, std::ptr::null()));
        assert_eq!(f64::from_bits(cell_value()), -1.0);
    });
}

#[test]
fn snc_p2_requires_reusable_result_promise() {
    fixture(|step, next, _| {
        let _reset = SncReset;
        snc_arm(step);
        js_iter_result_set(-1.0, 0);
        INLINE_TRAP.with(|slot| {
            let mut trap = slot.get();
            trap.trap_next = std::ptr::null_mut();
            slot.set(trap);
        });
        assert!(!admitted(5.0, step), "first call has no reusable result");
        let other = crate::closure::js_closure_alloc(crate::fn_info!(resume, 2), 0);
        INLINE_TRAP.with(|slot| {
            let mut trap = slot.get();
            trap.trap_next = next;
            trap.current_step = other as usize;
            slot.set(trap);
        });
        assert!(!admitted(5.0, step), "another activation owns the trap");
        assert_eq!(f64::from_bits(cell_value()), -1.0);
    });
}

#[test]
fn snc_p3_declines_while_async_hooks_are_active() {
    fixture(|step, _, _| {
        let _reset = SncReset;
        snc_arm(step);
        crate::async_hook_slots::install();
        let options = crate::object::js_object_alloc(0, 0);
        let callback = crate::closure::js_closure_alloc(crate::fn_info!(init, 4), 0);
        let key = crate::string::js_string_from_bytes(b"init".as_ptr(), 4);
        crate::object::js_object_set_field_by_name(options, key, boxed(callback));
        let hook = crate::async_hooks::js_async_hooks_create_hook(boxed(options));
        struct Disable(f64);
        impl Drop for Disable {
            fn drop(&mut self) {
                crate::async_hooks::js_async_hook_disable(crate::value::js_nanbox_get_pointer(
                    self.0,
                ));
            }
        }
        let _disable = Disable(hook);
        crate::async_hooks::js_async_hook_enable(crate::value::js_nanbox_get_pointer(hook));
        js_iter_result_set(-1.0, 0);
        assert!(!admitted(5.0, step));
        assert_eq!(f64::from_bits(cell_value()), -1.0);
    });
}

#[test]
fn snc_p4_declines_non_settled_or_observable_values() {
    fixture(|step, _, inner| unsafe {
        let _reset = SncReset;
        snc_arm(step);
        js_iter_result_set(-1.0, 0);
        for state in [PromiseState::Pending, PromiseState::Rejected] {
            (*inner).state = state;
            assert!(!admitted(boxed(inner), step));
        }
        (*inner).state = PromiseState::Fulfilled;
        crate::object::prototype_chain::object_set_static_prototype(
            inner as usize,
            crate::value::TAG_NULL,
        );
        assert!(!(*inner).meta.is_null());
        assert!(!admitted(boxed(inner), step), "patched prototype");
        let object = crate::object::js_object_alloc(0, 0);
        assert!(
            !admitted(boxed(object), step),
            "thenable/object is not a native promise"
        );
        assert_eq!(f64::from_bits(cell_value()), -1.0);
    });
}

#[test]
fn snc_p5_requires_an_empty_microtask_queue() {
    fixture(|step, next, _| {
        let _reset = SncReset;
        snc_arm(step);
        js_iter_result_set(-1.0, 0);
        TASK_QUEUE.with(|queue| {
            queue.borrow_mut().push_back(Task::Inline(
                std::ptr::null(),
                0.0,
                next,
                true,
                capture_context(),
            ))
        });
        assert!(
            !admitted(5.0, step),
            "a queued job runs before the continuation"
        );
        assert_eq!(f64::from_bits(cell_value()), -1.0);
        TASK_QUEUE.with(|queue| queue.borrow_mut().clear());
        assert!(admitted(5.0, step));
    });
}

#[test]
fn snc_p6_declines_a_cancelled_job_domain_and_marks_ready_otherwise() {
    fixture(|step, _, _| {
        let _reset = SncReset;
        snc_arm(step);
        let domain = crate::async_context::job_domain::JobDomain::new();
        domain.run(|| {
            js_iter_result_set(-1.0, 0);
            assert!(admitted(5.0, step), "live current domain is eligible");
            assert!(domain.take_ready(), "the same ready mark as enqueue");
            domain.cancel();
            js_iter_result_set(-1.0, 0);
            assert!(!admitted(5.0, step), "cancelled domain drops the job");
            assert_eq!(f64::from_bits(cell_value()), -1.0);
        });
    });
}
