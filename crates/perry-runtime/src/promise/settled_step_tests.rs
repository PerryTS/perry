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
