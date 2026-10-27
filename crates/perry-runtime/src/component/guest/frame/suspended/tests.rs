use super::*;

struct Begin {
    frame: *mut c_void,
    storage: i64,
    marker: f64,
    promise: u32,
}
unsafe extern "C" fn begin(data: *mut c_void) {
    let data = &mut *data.cast::<Begin>();
    let store = crate::array::js_array_alloc_with_length(1);
    crate::array::js_array_set_f64(store, 0, data.marker);
    als::enter_with(data.storage, crate::value::js_nanbox_pointer(store as i64));
    data.promise = perry_component_frame_import_begin(data.frame);
    // The task's later context differs from the import's completion context.
    let store = crate::array::js_array_alloc_with_length(1);
    crate::array::js_array_set_f64(store, 0, data.marker + 100.0);
    als::enter_with(data.storage, crate::value::js_nanbox_pointer(store as i64));
}
struct Complete {
    frame: *mut c_void,
    value: u32,
    reject: bool,
    status: i32,
}
unsafe extern "C" fn complete(data: *mut c_void) {
    let data = &mut *data.cast::<Complete>();
    data.status = if data.reject {
        perry_component_frame_import_reject(data.frame, data.value)
    } else {
        perry_component_frame_import_resolve(data.frame, data.value)
    };
}
fn promise(value: f64) -> *mut crate::promise::Promise {
    JSValue::from_bits(value.to_bits())
        .as_pointer::<crate::promise::Promise>()
        .cast_mut()
}
fn array(value: f64) -> *mut crate::array::ArrayHeader {
    JSValue::from_bits(value.to_bits())
        .as_pointer::<crate::array::ArrayHeader>()
        .cast_mut()
}
extern "C-unwind" fn read_import_context(
    closure: *const crate::closure::ClosureHeader,
    _: crate::closure::JsThis,
) -> f64 {
    let scope = RuntimeHandleScope::new();
    let log = scope.root_nanbox_f64(crate::closure::js_closure_get_capture_f64(closure, 0));
    let token = crate::closure::js_closure_get_capture_f64(closure, 1) as i64;
    let store = scope.root_nanbox_f64(als::get_store(token).expect("captured import ALS"));
    crate::gc::gc_collect_minor();
    let marker = crate::array::js_array_get_f64(array(store.get_nanbox_f64()), 0);
    let index = crate::array::js_array_length(array(log.get_nanbox_f64()));
    crate::array::js_array_set_f64_extend(array(log.get_nanbox_f64()), index, marker);
    undefined()
}
fn thenable(log: f64, token: i64) -> f64 {
    let scope = RuntimeHandleScope::new();
    let log = scope.root_nanbox_f64(log);
    let object = scope.root_nanbox_f64(crate::value::js_nanbox_pointer(
        crate::object::js_object_alloc(0, 0) as i64,
    ));
    let closure = scope.root_raw_mut_ptr(crate::closure::js_closure_alloc(
        crate::fn_info!(unwind read_import_context, 0),
        2,
    ));
    crate::closure::js_closure_set_capture_f64(closure.get_raw_mut_ptr(), 0, log.get_nanbox_f64());
    crate::closure::js_closure_set_capture_f64(closure.get_raw_mut_ptr(), 1, token as f64);
    let key = scope.root_string_ptr(crate::string::js_string_from_bytes(b"then".as_ptr(), 4));
    crate::object::js_object_define_getter(
        object.get_nanbox_f64(),
        crate::value::js_nanbox_string(key.get_raw_mut_ptr::<crate::string::StringHeader>() as i64),
        crate::value::js_nanbox_pointer(
            closure.get_raw_mut_ptr::<crate::closure::ClosureHeader>() as i64
        ),
    );
    object.get_nanbox_f64()
}

#[test]
fn pending_imports_keep_heap_frames_and_als_through_moving_gc_and_reverse_c_resume() {
    let _nursery = crate::gc::CopyingNurseryTestGuard::new(0);
    let _triggers = crate::gc::GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let _evacuate = crate::gc::knob_overrides::ForcedEvacuationTestGuard::on();
    crate::gc::gc_init();
    let scope = RuntimeHandleScope::new();
    let mut previous = als::capture_context();
    let previous_roots = als::root_snapshot(&scope, &previous);
    let storage = als::AsyncLocalStoragePayload::default();
    als::enter_with(storage.token(), 9.0);
    let log = scope.root_nanbox_f64(crate::value::js_nanbox_pointer(
        crate::array::js_array_alloc(4) as i64,
    ));
    let depth = als::context_guard_depth();
    unsafe {
        let mut entries = Vec::new();
        for marker in [11.0, 22.0] {
            let parent = perry_component_frame_task_new();
            let mut owner = Box::new(Frame::new());
            let pointer = (&raw mut *owner).cast();
            let mut data = Begin {
                frame: pointer,
                storage: storage.token(),
                marker,
                promise: 0,
            };
            assert_eq!(
                perry_component_frame_task_resume(parent, Some(begin), (&raw mut data).cast()),
                0
            );
            assert!(data.promise > 0);
            let value = owner.add(thenable(log.get_nanbox_f64(), storage.token()));
            assert_eq!(
                (*promise(owner.get(data.promise))).state,
                crate::promise::PromiseState::Pending
            );
            owner.release_import(); // Exactly the runtime's post-import handoff.
            entries.push((parent, pointer, data.promise, value));
            assert_eq!(als::get_store(storage.token()), Some(9.0));
            assert!(job_domain::current().is_none());
            assert_eq!(als::context_guard_depth(), depth);
        }
        let before = frame(entries[0].1).get(entries[0].3).to_bits();
        crate::gc::gc_collect_minor();
        assert_ne!(
            before,
            frame(entries[0].1).get(entries[0].3).to_bits(),
            "the retained input must actually move"
        );
        for index in [1, 0] {
            let (parent, pointer, slot, value) = entries[index];
            let mut data = Complete {
                frame: pointer,
                value,
                reject: false,
                status: 99,
            };
            assert_eq!(
                perry_component_frame_task_resume(parent, Some(complete), (&raw mut data).cast()),
                0
            );
            assert_eq!(data.status, 0);
            assert_eq!(
                (*promise(frame(pointer).get(slot))).state,
                crate::promise::PromiseState::Fulfilled
            );
            assert_eq!(
                (*promise(frame(pointer).get(slot))).value.to_bits(),
                frame(pointer).get(value).to_bits()
            );
            assert_eq!(als::get_store(storage.token()), Some(9.0));
            assert!(job_domain::current().is_none());
            assert_eq!(als::context_guard_depth(), depth);
            if index == 1 {
                assert_eq!(
                    (*promise(frame(entries[0].1).get(entries[0].2))).state,
                    crate::promise::PromiseState::Pending
                );
            }
        }
        assert_eq!(
            crate::array::js_array_length(array(log.get_nanbox_f64())),
            2
        );
        assert_eq!(
            crate::array::js_array_get_f64(array(log.get_nanbox_f64()), 0),
            22.0
        );
        assert_eq!(
            crate::array::js_array_get_f64(array(log.get_nanbox_f64()), 1),
            11.0
        );
        let public = scope.root_nanbox_f64(frame(entries[0].1).get(entries[0].2));
        for (parent, pointer, _, _) in entries {
            perry_component_frame_drop(pointer);
            perry_component_frame_drop(parent);
        }
        crate::gc::gc_collect_minor();
        assert_eq!(
            (*promise(public.get_nanbox_f64())).state,
            crate::promise::PromiseState::Fulfilled
        );
    }
    als::refresh_snapshot_from_roots(&mut previous, &previous_roots);
    als::restore_context(previous);
}

#[test]
fn immediate_import_completion_waits_for_the_runtime_handoff_before_c_drops_the_frame() {
    crate::gc::gc_init();
    let domain = job_domain::JobDomain::new();
    let scope = RuntimeHandleScope::new();
    unsafe {
        let mut owner = Box::new(Frame::new());
        let pointer = (&raw mut *owner).cast();
        let slot = domain.run(|| perry_component_frame_import_begin(pointer));
        let value = perry_component_frame_number(pointer, 42.0);
        assert_eq!(
            domain.run(|| perry_component_frame_import_resolve(pointer, value)),
            0
        );
        let public = scope.root_nanbox_f64(owner.get(slot));
        let alive = Arc::downgrade(&owner.alive);
        owner.release_import();
        assert!(alive.upgrade().is_some());
        assert_eq!((*promise(frame(pointer).get(slot))).value, 42.0);
        perry_component_frame_drop(pointer);
        assert!(alive.upgrade().is_none());
        crate::gc::gc_collect_minor();
        assert_eq!((*promise(public.get_nanbox_f64())).value, 42.0);
        let ordinary = Box::new(Frame::new());
        let alive = Arc::downgrade(&ordinary.alive);
        ordinary.release_import();
        assert!(
            alive.upgrade().is_none(),
            "synchronous imports must still release immediately"
        );
    }
}

#[test]
fn import_begin_rejects_ambient_and_task_frames_without_transferring_them() {
    crate::gc::gc_init();
    unsafe {
        let ordinary = perry_component_frame_new();
        assert_eq!(perry_component_frame_import_begin(ordinary), 0);
        assert_eq!(perry_component_frame_failed(ordinary), 1);
        assert!(frame(ordinary).suspended.is_none());
        perry_component_frame_drop(ordinary);
        let task = perry_component_frame_task_new();
        assert_eq!(perry_component_frame_import_begin(task), 0);
        assert_eq!(perry_component_frame_failed(task), 1);
        assert!(frame(task).suspended.is_none());
        perry_component_frame_drop(task);
        let domain = job_domain::JobDomain::new();
        let duplicate = perry_component_frame_new();
        let promise_slot = domain.run(|| perry_component_frame_import_begin(duplicate));
        assert!(promise_slot > 0);
        assert_eq!(
            domain.run(|| perry_component_frame_import_begin(duplicate)),
            0
        );
        assert!(frame(duplicate).suspended.is_some());
        assert_eq!(
            (*promise(frame(duplicate).get(promise_slot))).state,
            crate::promise::PromiseState::Pending
        );
        perry_component_frame_drop(duplicate);
    }
}

#[test]
fn import_rejection_and_duplicate_completion_preserve_the_first_promise_outcome() {
    let _nursery = crate::gc::CopyingNurseryTestGuard::new(0);
    let _triggers = crate::gc::GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let _evacuate = crate::gc::knob_overrides::ForcedEvacuationTestGuard::on();
    crate::gc::gc_init();
    let domain = job_domain::JobDomain::new();
    unsafe {
        let pointer = perry_component_frame_new();
        let slot = domain.run(|| perry_component_frame_import_begin(pointer));
        let error = perry_component_frame_array(pointer, 1);
        let number = perry_component_frame_number(pointer, 77.0);
        perry_component_frame_array_set(pointer, error, 0, number);
        assert_eq!(
            domain.run(|| perry_component_frame_import_reject(pointer, error)),
            0
        );
        crate::gc::gc_collect_minor();
        assert_eq!(
            (*promise(frame(pointer).get(slot))).state,
            crate::promise::PromiseState::Rejected
        );
        assert_eq!(
            (*promise(frame(pointer).get(slot))).reason.to_bits(),
            frame(pointer).get(error).to_bits()
        );
        assert_eq!(
            domain.run(|| perry_component_frame_import_resolve(pointer, number)),
            -1
        );
        let scope = RuntimeHandleScope::new();
        let first_error = scope.root_nanbox_f64(frame(pointer).error().unwrap());
        assert_eq!(
            domain.run(|| perry_component_frame_import_reject(pointer, error)),
            -1
        );
        crate::gc::gc_collect_minor();
        assert!(frame(pointer).failed);
        assert_eq!(
            (*promise(frame(pointer).get(slot))).reason.to_bits(),
            frame(pointer).get(error).to_bits()
        );
        // The frame's misuse error also remains owned; repeated calls are inert.
        assert_eq!(
            frame(pointer).error().unwrap().to_bits(),
            first_error.get_nanbox_f64().to_bits()
        );
        perry_component_frame_drop(pointer);
    }
}

#[test]
fn import_completion_rejects_foreign_and_finished_tasks() {
    crate::gc::gc_init();
    unsafe {
        for finished in [false, true] {
            let owner = job_domain::JobDomain::new();
            let other = job_domain::JobDomain::new();
            let pointer = perry_component_frame_new();
            let slot = owner.run(|| perry_component_frame_import_begin(pointer));
            let result = perry_component_frame_number(pointer, 1.0);
            let token = owner.token();
            let status = if finished {
                drop(owner);
                job_domain::run(token, || {
                    perry_component_frame_import_resolve(pointer, result)
                })
            } else {
                other.run(|| perry_component_frame_import_resolve(pointer, result))
            };
            assert_eq!(status, -1);
            assert_eq!(
                (*promise(frame(pointer).get(slot))).state,
                crate::promise::PromiseState::Pending
            );
            assert!(job_domain::current().is_none());
            perry_component_frame_drop(pointer);
        }
    }
}

#[test]
fn task_resume_rejects_missing_callbacks_and_ordinary_frames_inside_c_boundary() {
    unsafe extern "C" fn increment(data: *mut c_void) {
        *data.cast::<u32>() += 1;
    }
    crate::gc::gc_init();
    unsafe {
        let depth = als::context_guard_depth();
        let ordinary = perry_component_frame_new();
        let mut count = 0u32;
        assert_eq!(
            perry_component_frame_task_resume(ordinary, Some(increment), (&raw mut count).cast()),
            -1
        );
        assert_eq!(count, 0);
        perry_component_frame_drop(ordinary);
        let task = perry_component_frame_task_new();
        assert_eq!(
            perry_component_frame_task_resume(task, None, std::ptr::null_mut()),
            -1
        );
        assert!(job_domain::current().is_none());
        assert_eq!(als::context_guard_depth(), depth);
        perry_component_frame_drop(task);
    }
}

#[test]
fn invalid_import_result_slot_is_caught_and_keeps_the_frame_inert() {
    crate::gc::gc_init();
    let owner = job_domain::JobDomain::new();
    unsafe {
        let pointer = perry_component_frame_new();
        let slot = owner.run(|| perry_component_frame_import_begin(pointer));
        assert_eq!(
            owner.run(|| perry_component_frame_import_resolve(pointer, u32::MAX)),
            -1
        );
        let first_error = frame(pointer).error().unwrap().to_bits();
        assert_eq!(
            owner.run(|| perry_component_frame_import_resolve(pointer, 0)),
            -1
        );
        assert_eq!(frame(pointer).error().unwrap().to_bits(), first_error);
        assert_eq!(
            (*promise(frame(pointer).get(slot))).state,
            crate::promise::PromiseState::Pending
        );
        perry_component_frame_drop(pointer);
    }
}

#[test]
fn import_resolve_adopts_a_promise_without_driving_its_jobs_or_a_siblings_checkpoint() {
    crate::gc::gc_init();
    let owner = job_domain::JobDomain::new();
    let sibling = job_domain::JobDomain::new();
    unsafe {
        let pointer = perry_component_frame_new();
        let outer = owner.run(|| perry_component_frame_import_begin(pointer));
        let inner = owner.run(|| {
            frame(pointer).add(crate::value::js_nanbox_pointer(
                crate::promise::js_promise_new() as i64,
            ))
        });
        assert_eq!(
            owner.run(|| perry_component_frame_import_resolve(pointer, inner)),
            0
        );
        assert_eq!(
            (*promise(frame(pointer).get(outer))).state,
            crate::promise::PromiseState::Pending
        );
        owner.run(|| crate::promise::js_promise_resolve(promise(frame(pointer).get(inner)), 42.0));
        sibling.run(crate::promise::microtasks::js_promise_run_microtasks_checkpoint);
        assert_eq!(
            (*promise(frame(pointer).get(outer))).state,
            crate::promise::PromiseState::Pending
        );
        assert!(owner.run(crate::promise::microtasks::js_promise_run_microtasks_checkpoint) > 0);
        assert_eq!(
            (*promise(frame(pointer).get(outer))).state,
            crate::promise::PromiseState::Fulfilled
        );
        assert_eq!((*promise(frame(pointer).get(outer))).value, 42.0);
        perry_component_frame_drop(pointer);
    }
}
