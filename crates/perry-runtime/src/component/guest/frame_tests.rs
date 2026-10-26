use super::*;

unsafe fn promise_in_frame(owner: *mut c_void, slot: u32) -> *mut crate::promise::Promise {
    JSValue::from_bits(super::frame(owner).get(slot).to_bits())
        .as_pointer::<crate::promise::Promise>()
        .cast_mut()
}

#[test]
fn promise_poll_never_drives_jobs_and_owns_its_result_after_the_promise_is_released() {
    let _nursery = crate::gc::CopyingNurseryTestGuard::new(0);
    let _triggers = crate::gc::GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let _evacuate = crate::gc::knob_overrides::ForcedEvacuationTestGuard::on();
    crate::gc::gc_init();
    let scope = RuntimeHandleScope::new();
    let jobs = scope.root_nanbox_f64(crate::value::js_nanbox_pointer(
        crate::array::js_array_alloc_with_length(1) as i64,
    ));
    crate::array::js_array_set_f64(array(jobs.get_nanbox_f64()), 0, 0.0);
    let callback = scope.root_raw_mut_ptr(crate::closure::js_closure_alloc(
        crate::fn_info!(unwind record_checkpoint_job, 0),
        1,
    ));
    crate::closure::js_closure_set_capture_f64(
        callback.get_raw_mut_ptr(),
        0,
        jobs.get_nanbox_f64(),
    );
    crate::builtins::js_queue_microtask(
        callback.get_raw_mut_ptr::<crate::closure::ClosureHeader>() as i64,
    );
    unsafe {
        let owner = perry_component_frame_new();
        let promise = super::frame(owner).add(crate::value::js_nanbox_pointer(
            crate::promise::js_promise_new() as i64,
        ));
        let mut output = u32::MAX;
        assert_eq!(
            perry_component_frame_promise_poll(owner, promise, &mut output),
            0
        );
        assert_eq!(output, 0);
        let input = perry_component_frame_new();
        let value = perry_component_frame_array(input, 1);
        let text = "Promise result retained across moving GC 🌍";
        let string = perry_component_frame_string(input, text.as_ptr(), text.len());
        perry_component_frame_array_set(input, value, 0, string);
        crate::promise::js_promise_resolve(
            promise_in_frame(owner, promise),
            super::frame(input).get(value),
        );
        perry_component_frame_drop(input);
        let before = super::frame(owner).get(promise).to_bits();
        crate::gc::gc_collect_minor();
        assert_ne!(
            before,
            super::frame(owner).get(promise).to_bits(),
            "the Promise must really move"
        );
        assert_eq!(
            perry_component_frame_promise_poll(owner, promise, &mut output),
            1
        );
        assert_ne!(output, 0);
        assert_eq!(
            crate::array::js_array_get_f64(array(jobs.get_nanbox_f64()), 0),
            0.0,
            "polling must not run another task's jobs"
        );
        // Discard the Promise's only frame edge. Its array result now belongs
        // to the fresh output slot independently of the original input frame.
        let values = super::frame(owner).values();
        crate::array::js_array_set_f64(array(values), promise, undefined());
        crate::promise::microtasks::js_promise_run_microtasks_checkpoint();
        assert_eq!(
            crate::array::js_array_get_f64(array(jobs.get_nanbox_f64()), 0),
            1.0
        );
        let string = perry_component_frame_array_get(owner, output, 0);
        let mut bytes = vec![0; perry_component_frame_string_len(owner, string)];
        perry_component_frame_string_copy(owner, string, bytes.as_mut_ptr(), bytes.len());
        assert_eq!(bytes, text.as_bytes());
        assert_eq!(perry_component_frame_failed(owner), 0);
        perry_component_frame_drop(owner);
    }
}

#[test]
fn promise_poll_handles_pending_rejection_and_preserves_the_original_error_through_gc_and_cleanup()
{
    let _nursery = crate::gc::CopyingNurseryTestGuard::new(0);
    let _triggers = crate::gc::GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let _evacuate = crate::gc::knob_overrides::ForcedEvacuationTestGuard::on();
    crate::gc::gc_init();
    unsafe {
        let owner = perry_component_frame_new();
        let promise = super::frame(owner).add(crate::value::js_nanbox_pointer(
            crate::promise::js_promise_new() as i64,
        ));
        let mut output = u32::MAX;
        assert_eq!(
            perry_component_frame_promise_poll(owner, promise, &mut output),
            0
        );
        let input = perry_component_frame_new();
        let reason = perry_component_frame_object(input);
        let number = perry_component_frame_number(input, 123.0);
        perry_component_frame_set(input, reason, b"marker".as_ptr(), 6, number);
        let unhandled = crate::promise::rejection::unhandled_rejection_count();
        crate::promise::js_promise_reject(
            promise_in_frame(owner, promise),
            super::frame(input).get(reason),
        );
        assert_eq!(
            crate::promise::rejection::unhandled_rejection_count(),
            unhandled,
            "the C observer handles rejection even before it settles"
        );
        perry_component_frame_drop(input);
        crate::gc::gc_collect_minor();
        assert_eq!(
            perry_component_frame_promise_poll(owner, promise, &mut output),
            -1
        );
        assert_eq!(output, 0);
        assert_eq!(
            super::frame(owner).error().unwrap().to_bits(),
            (*promise_in_frame(owner, promise)).reason.to_bits()
        );
        crate::gc::gc_collect_minor();
        // Invalid cleanup must not overwrite the already recorded rejection.
        assert_eq!(
            perry_component_frame_promise_poll(owner, u32::MAX, std::ptr::null_mut()),
            -1
        );
        assert_eq!(
            super::frame(owner).error().unwrap().to_bits(),
            (*promise_in_frame(owner, promise)).reason.to_bits()
        );
        assert_eq!(
            field(super::frame(owner).error().unwrap(), b"marker"),
            123.0
        );
        perry_component_frame_drop(owner);
    }
}

#[test]
fn promise_poll_reports_invalid_c_output_type_and_slot_without_crossing_the_c_error_boundary() {
    crate::gc::gc_init();
    unsafe {
        for kind in 0..3 {
            let owner = perry_component_frame_new();
            let value = perry_component_frame_number(owner, 42.0);
            let mut output = 123;
            let status = match kind {
                0 => perry_component_frame_promise_poll(owner, value, std::ptr::null_mut()),
                1 => perry_component_frame_promise_poll(owner, value, &mut output),
                _ => perry_component_frame_promise_poll(owner, u32::MAX, &mut output),
            };
            assert_eq!(status, -1);
            assert_eq!(perry_component_frame_failed(owner), 1);
            assert_eq!(output, if kind == 0 { 123 } else { 0 });
            let error = super::frame(owner).error().unwrap();
            let message = crate::error::js_error_get_message(
                JSValue::from_bits(error.to_bits())
                    .as_pointer::<crate::error::ErrorHeader>()
                    .cast_mut(),
            );
            let message = string(crate::value::js_nanbox_string(message as i64));
            assert!(
                message.contains(match kind {
                    0 => "Missing component Promise output slot",
                    1 => "Expected a Promise in the component frame",
                    _ => "Invalid component frame slot",
                }),
                "{message}"
            );
            perry_component_frame_drop(owner);
        }
    }
}

extern "C-unwind" fn returned_array(
    closure: *const crate::closure::ClosureHeader,
    _: crate::closure::JsThis,
) -> f64 {
    let scope = RuntimeHandleScope::new();
    let jobs = scope.root_nanbox_f64(crate::closure::js_closure_get_capture_f64(closure, 0));
    let result = scope.root_raw_mut_ptr(crate::array::js_array_alloc_with_length(1));
    // A numeric address is only a movement witness, not another managed root
    // for the returned array. The frame must retain and rewrite the real value.
    crate::array::js_array_set_f64(
        array(jobs.get_nanbox_f64()),
        1,
        result.get_raw_mut_ptr::<crate::array::ArrayHeader>() as usize as f64,
    );
    crate::value::js_nanbox_pointer(result.get_raw_mut_ptr::<crate::array::ArrayHeader>() as i64)
}

#[test]
fn export_checkpoint_drains_jobs_without_timers_and_retains_its_result() {
    let _nursery = crate::gc::CopyingNurseryTestGuard::new(0);
    let _triggers = crate::gc::GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let _evacuate = crate::gc::knob_overrides::ForcedEvacuationTestGuard::on();
    crate::gc::gc_init();
    let scope = RuntimeHandleScope::new();
    let global = scope.root_nanbox_f64(crate::object::js_get_global_this());
    let previous = scope.root_nanbox_f64(super::super::maybe_registry());
    let world = serde_json::json!({"exports":[{"name":"", "functions":[
        {"name":"array", "params":[]}
    ]}]});
    let owner = scope.root_nanbox_f64(context::publish(world, undefined()));
    let exports = scope.root_nanbox_f64(crate::value::js_nanbox_pointer(
        crate::object::js_object_alloc(0, 0) as i64,
    ));
    let jobs = scope.root_nanbox_f64(crate::value::js_nanbox_pointer(
        crate::array::js_array_alloc_with_length(2) as i64,
    ));
    crate::array::js_array_set_f64(array(jobs.get_nanbox_f64()), 0, 0.0);
    let callback = scope.root_raw_mut_ptr(crate::closure::js_closure_alloc(
        crate::fn_info!(unwind returned_array, 0),
        1,
    ));
    crate::closure::js_closure_set_capture_f64(
        callback.get_raw_mut_ptr(),
        0,
        jobs.get_nanbox_f64(),
    );
    set_field(
        exports.get_nanbox_f64(),
        b"array",
        crate::value::js_nanbox_pointer(
            callback.get_raw_mut_ptr::<crate::closure::ClosureHeader>() as i64,
        ),
    );
    let callback = scope.root_raw_mut_ptr(crate::closure::js_closure_alloc(
        crate::fn_info!(unwind record_checkpoint_job, 0),
        1,
    ));
    crate::closure::js_closure_set_capture_f64(
        callback.get_raw_mut_ptr(),
        0,
        jobs.get_nanbox_f64(),
    );
    crate::builtins::js_queue_microtask(
        callback.get_raw_mut_ptr::<crate::closure::ClosureHeader>() as i64,
    );
    crate::timer::js_set_immediate_callback(
        callback.get_raw_mut_ptr::<crate::closure::ClosureHeader>() as i64,
    );
    unsafe {
        use crate::object::owned_state_bag::{self as bag, COMPONENT_EXPORTS, COMPONENT_GUEST};
        bag::store(
            object_pointer(owner.get_nanbox_f64()).unwrap() as usize,
            COMPONENT_EXPORTS,
            exports.get_nanbox_f64(),
        );
        bag::store(
            object_pointer(global.get_nanbox_f64()).unwrap() as usize,
            COMPONENT_GUEST,
            owner.get_nanbox_f64(),
        );
        let frame = perry_component_frame_new();
        let args = perry_component_frame_array(frame, 0);
        let result = perry_component_frame_call(frame, 0, 0, args);
        assert_eq!(perry_component_frame_failed(frame), 0);
        assert_eq!(
            crate::array::js_array_get_f64(array(jobs.get_nanbox_f64()), 0),
            1.0,
            "the export checkpoint must run its microtask but leave the immediate pending"
        );
        assert_ne!(
            array(super::frame(frame).get(result)) as usize as f64,
            crate::array::js_array_get_f64(array(jobs.get_nanbox_f64()), 1),
            "the exported result must move during its checkpoint, after its call scope has ended"
        );
        crate::timer::js_event_loop_check_phase();
        assert_eq!(
            crate::array::js_array_get_f64(array(jobs.get_nanbox_f64()), 0),
            2.0,
            "the immediate must remain available to its event-loop phase"
        );
        crate::gc::gc_collect_minor();
        // Earlier job collections can promote the result. It must remain
        // valid, but a subsequent minor collection need not move it again.
        assert_eq!(perry_component_frame_array_len(frame, result), 1);
        perry_component_frame_drop(frame);
        bag::store(
            object_pointer(global.get_nanbox_f64()).unwrap() as usize,
            COMPONENT_GUEST,
            previous.get_nanbox_f64(),
        );
    }
}

extern "C-unwind" fn record_checkpoint_job(
    closure: *const crate::closure::ClosureHeader,
    _: crate::closure::JsThis,
) -> f64 {
    let scope = RuntimeHandleScope::new();
    let jobs = scope.root_nanbox_f64(crate::closure::js_closure_get_capture_f64(closure, 0));
    crate::gc::gc_collect_minor();
    let count = crate::array::js_array_get_f64(array(jobs.get_nanbox_f64()), 0);
    crate::array::js_array_set_f64(array(jobs.get_nanbox_f64()), 0, count + 1.0);
    undefined()
}

#[test]
fn adapter_slots_are_rewritten_by_a_real_moving_collection() {
    let _nursery = crate::gc::CopyingNurseryTestGuard::new(0);
    let _triggers = crate::gc::GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let _evacuate = crate::gc::knob_overrides::ForcedEvacuationTestGuard::on();
    crate::gc::gc_init();
    unsafe {
        let frame = perry_component_frame_new();
        let text = "A heap string that must survive collection 🌍";
        let string = perry_component_frame_string(frame, text.as_ptr(), text.len());
        let list = perry_component_frame_array(frame, 1);
        perry_component_frame_array_set(frame, list, 0, string);
        let before = super::frame(frame).get(list).to_bits();
        crate::gc::gc_collect_minor();
        assert_ne!(
            super::frame(frame).get(list).to_bits(),
            before,
            "the subject array must move"
        );
        let value = perry_component_frame_array_get(frame, list, 0);
        let mut bytes = vec![0; perry_component_frame_string_len(frame, value)];
        perry_component_frame_string_copy(frame, value, bytes.as_mut_ptr(), bytes.len());
        assert_eq!(std::str::from_utf8(&bytes).unwrap(), text);
        assert_eq!(perry_component_frame_failed(frame), 0);
        perry_component_frame_drop(frame);
    }
}

#[test]
fn canonical_byte_lists_are_owned_and_survive_moving_collection() {
    let _nursery = crate::gc::CopyingNurseryTestGuard::new(0);
    let _triggers = crate::gc::GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let _evacuate = crate::gc::knob_overrides::ForcedEvacuationTestGuard::on();
    crate::gc::gc_init();
    for len in [0, 31, 4097, 3 * 1024 * 1024 + 137] {
        let expected: Vec<u8> = (0..len).map(|i| (i * 31 + 7) as u8).collect();
        let mut input = expected.clone();
        unsafe {
            let frame = perry_component_frame_new();
            let source = if len == 0 {
                std::ptr::null()
            } else {
                input.as_ptr()
            };
            let value = perry_component_frame_bytes(frame, source, len);
            input.fill(0); // Canonical caller storage must not back the JS view.
            let holder = perry_component_frame_array(frame, 1);
            perry_component_frame_array_set(frame, holder, 0, value);
            let before = super::frame(frame).get(holder).to_bits();
            crate::gc::gc_collect_minor();
            // Byte storage may be pinned or large. Its enclosing JS array
            // proves the collection actually moved the rooted object graph.
            assert_ne!(super::frame(frame).get(holder).to_bits(), before);
            let value = perry_component_frame_array_get(frame, holder, 0);
            assert_eq!(perry_component_frame_bytes_len(frame, value), len);
            let mut output = vec![0; len];
            perry_component_frame_bytes_copy(frame, value, output.as_mut_ptr(), len);
            assert_eq!(output, expected);
            assert_eq!(perry_component_frame_failed(frame), 0);
            perry_component_frame_drop(frame);
        }
    }
}

#[test]
fn failed_frame_keeps_the_original_heap_exception_rooted() {
    let _nursery = crate::gc::CopyingNurseryTestGuard::new(0);
    let _triggers = crate::gc::GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let _evacuate = crate::gc::knob_overrides::ForcedEvacuationTestGuard::on();
    crate::gc::gc_init();
    let mut frame = Frame::new();
    frame.guard::<()>(|_| {
        let thrown =
            crate::value::js_nanbox_pointer(crate::array::js_array_alloc_with_length(3) as i64);
        crate::exception::js_throw(thrown);
    });
    let before = frame.error().unwrap().to_bits();
    crate::gc::gc_collect_minor();
    let after = frame.error().unwrap();
    assert_ne!(
        after.to_bits(),
        before,
        "the original heap exception must move"
    );
    assert_eq!(crate::array::js_array_length(array(after)), 3);
}

#[test]
fn scalar_limits_and_errors_never_unwind_through_the_adapter() {
    unsafe {
        let frame = perry_component_frame_new();
        let unsigned = perry_component_frame_u64(frame, u64::MAX);
        let signed = perry_component_frame_s64(frame, i64::MIN);
        assert_eq!(perry_component_frame_get_u64(frame, unsigned), u64::MAX);
        assert_eq!(perry_component_frame_get_s64(frame, signed), i64::MIN);
        assert_eq!(perry_component_frame_failed(frame), 0);
        assert_eq!(perry_component_frame_get_s64(frame, unsigned), 0);
        assert_eq!(perry_component_frame_failed(frame), 1);
        let first_error = super::frame(frame).error().unwrap();
        // A failed frame is inert and retains the first error.
        assert_eq!(perry_component_frame_object(frame), 0);
        assert_eq!(
            super::frame(frame).error().unwrap().to_bits(),
            first_error.to_bits()
        );
        perry_component_frame_drop(frame);

        let frame = perry_component_frame_new();
        let number = perry_component_frame_number(frame, 256.0);
        assert_eq!(
            perry_component_frame_get_integer(frame, number, 0.0, 255.0),
            0.0
        );
        assert_eq!(perry_component_frame_failed(frame), 1);
        perry_component_frame_drop(frame);
    }
}

#[test]
fn adapter_copies_strings_bytes_and_unicode_scalars() {
    unsafe {
        let frame = perry_component_frame_new();
        let data = [0, 127, 255];
        let bytes = perry_component_frame_bytes(frame, data.as_ptr(), data.len());
        let mut out = [0; 3];
        assert_eq!(perry_component_frame_bytes_len(frame, bytes), 3);
        perry_component_frame_bytes_copy(frame, bytes, out.as_mut_ptr(), out.len());
        assert_eq!(out, data);
        let character = perry_component_frame_char(frame, 0x1f30d);
        assert_eq!(perry_component_frame_get_char(frame, character), 0x1f30d);
        assert_eq!(perry_component_frame_failed(frame), 0);
        // Invalid Unicode is an adapter error, not a replacement character.
        perry_component_frame_char(frame, 0xd800);
        assert_eq!(perry_component_frame_failed(frame), 1);
        perry_component_frame_drop(frame);
    }
}

#[test]
fn guest_frames_can_finish_out_of_order_without_truncating_other_roots() {
    let _nursery = crate::gc::CopyingNurseryTestGuard::new(0);
    let _triggers = crate::gc::GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let _evacuate = crate::gc::knob_overrides::ForcedEvacuationTestGuard::on();
    crate::gc::gc_init();
    let older = Frame::new();
    let mut suspended = Frame::new();
    let slot = suspended.add(crate::value::js_nanbox_pointer(
        crate::array::js_array_alloc_with_length(3) as i64,
    ));
    let before = suspended.get(slot).to_bits();
    drop(older);
    crate::gc::gc_collect_minor();
    assert_ne!(suspended.get(slot).to_bits(), before, "the suspended frame's sole retained array must actually move after an older frame finishes");
    assert_eq!(crate::array::js_array_length(array(suspended.get(slot))), 3);
}

#[test]
fn guest_frames_survive_the_native_scope_that_created_them() {
    let _nursery = crate::gc::CopyingNurseryTestGuard::new(0);
    let _triggers = crate::gc::GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let _evacuate = crate::gc::knob_overrides::ForcedEvacuationTestGuard::on();
    crate::gc::gc_init();
    let (frame, slot, before) = {
        let temporary = RuntimeHandleScope::new();
        let _temporary_value = temporary.root_nanbox_f64(undefined());
        let mut frame = Frame::new();
        let slot = frame.add(crate::value::js_nanbox_pointer(
            crate::array::js_array_alloc_with_length(5) as i64,
        ));
        let before = frame.get(slot).to_bits();
        (frame, slot, before)
    };
    crate::gc::gc_collect_minor();
    assert_ne!(
        frame.get(slot).to_bits(),
        before,
        "a canonical frame must retain its values after its submitting native scope returns"
    );
    assert_eq!(crate::array::js_array_length(array(frame.get(slot))), 5);
}

#[test]
fn guest_frame_growth_publishes_live_heads_and_keeps_all_slots_traced() {
    let _nursery = crate::gc::CopyingNurseryTestGuard::new(0);
    let _triggers = crate::gc::GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let _evacuate = crate::gc::knob_overrides::ForcedEvacuationTestGuard::on();
    crate::gc::gc_init();
    let mut frame = Frame::new();
    for n in 0..1025 {
        let value =
            crate::value::js_nanbox_pointer(crate::array::js_array_alloc_with_length(n % 7) as i64);
        assert_eq!(frame.add(value), n + 1);
        // The frame owns this live private array; this only inspects its
        // tracked header to reject a pre-growth forwarding stub.
        let header = unsafe {
            crate::value::addr_class::try_read_tracked_gc_header(array(frame.values()) as usize)
        }
        .unwrap();
        assert_eq!(
            unsafe { header.as_ref().gc_flags } & crate::gc::GC_FLAG_FORWARDED,
            0,
            "an owned frame must publish each grown array head"
        );
        if n % 127 == 0 {
            crate::gc::gc_collect_minor();
        }
    }
    crate::gc::gc_collect_minor();
    for n in 0..1025 {
        assert_eq!(
            crate::array::js_array_length(array(frame.get(n + 1))),
            n % 7
        );
    }
}

#[test]
fn dropping_a_guest_frame_preserves_later_temporary_handle_scopes() {
    let _nursery = crate::gc::CopyingNurseryTestGuard::new(0);
    let _triggers = crate::gc::GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let _evacuate = crate::gc::knob_overrides::ForcedEvacuationTestGuard::on();
    crate::gc::gc_init();
    let frame = Frame::new();
    let temporary = RuntimeHandleScope::new();
    let value = temporary.root_nanbox_f64(crate::value::js_nanbox_pointer(
        crate::array::js_array_alloc_with_length(7) as i64,
    ));
    let before = value.get_nanbox_f64().to_bits();
    drop(frame);
    crate::gc::gc_collect_minor();
    assert_ne!(
        value.get_nanbox_f64().to_bits(),
        before,
        "dropping a persistent frame must not truncate another scope's root"
    );
    assert_eq!(
        crate::array::js_array_length(array(value.get_nanbox_f64())),
        7
    );
}

#[test]
fn guest_frame_drop_after_cell_teardown_never_resolves_its_stale_link() {
    crate::gc::gc_init();
    let domain = crate::async_context::job_domain::JobDomain::new();
    let mut deferred = Box::new(Frame::new());
    let pointer = (&raw mut *deferred).cast();
    assert!(domain.run(|| unsafe { perry_component_frame_import_begin(pointer) }) > 0);
    let frames = [
        Box::new(Frame::new()),
        unsafe { Box::from_raw(perry_component_frame_task_new().cast::<Frame>()) },
        deferred,
    ];
    for mut frame in frames {
        let alive = Arc::downgrade(&frame.alive);
        unsafe {
            crate::native_handle::finalize_native_handle_at_teardown(
                frame.owner.0 as *mut crate::native_handle::NativeHandleHeader,
            );
            assert!(!frame.alive.load(Ordering::Acquire));
            // The real teardown unmaps the cell. Release its test-only pin while
            // storage is still mapped, then poison the inert post-teardown token.
            payload::link_unref(frame.owner);
        }
        frame.owner = OwnerLink(usize::MAX);
        drop(frame);
        assert!(
            alive.upgrade().is_none(),
            "both native anchor references must be released"
        );
    }
}

#[test]
fn copied_frame_slots_survive_source_drop_and_moving_gc_including_self_copy() {
    let _nursery = crate::gc::CopyingNurseryTestGuard::new(0);
    let _triggers = crate::gc::GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let _evacuate = crate::gc::knob_overrides::ForcedEvacuationTestGuard::on();
    crate::gc::gc_init();
    unsafe {
        let source = perry_component_frame_new();
        let destination = perry_component_frame_new();
        let text = "independent frame ownership 🌍";
        let value = perry_component_frame_string(source, text.as_ptr(), text.len());
        let copied = perry_component_frame_copy(destination, source, value);
        perry_component_frame_drop(source);
        let before = super::frame(destination).get(copied).to_bits();
        crate::gc::gc_collect_minor();
        assert_ne!(before, super::frame(destination).get(copied).to_bits());
        let copied = perry_component_frame_copy(destination, destination, copied);
        let mut output = vec![0; text.len()];
        perry_component_frame_string_copy(destination, copied, output.as_mut_ptr(), output.len());
        assert_eq!(output, text.as_bytes());
        assert_eq!(perry_component_frame_failed(destination), 0);
        perry_component_frame_drop(destination);
    }
}

#[test]
fn copying_an_invalid_slot_fails_only_the_destination_and_preserves_its_first_error() {
    crate::gc::gc_init();
    unsafe {
        let source = perry_component_frame_new();
        let destination = perry_component_frame_new();
        assert_eq!(perry_component_frame_copy(destination, source, u32::MAX), 0);
        assert_eq!(perry_component_frame_failed(source), 0);
        assert_eq!(perry_component_frame_failed(destination), 1);
        let error = super::frame(destination).error().unwrap().to_bits();
        assert_eq!(perry_component_frame_copy(destination, source, u32::MAX), 0);
        assert_eq!(super::frame(destination).error().unwrap().to_bits(), error);
        perry_component_frame_drop(destination);
        perry_component_frame_drop(source);
    }
}
