use super::*;

struct Startup {
    objects: u64,
    called: bool,
}

unsafe extern "C" fn check_startup(data: *mut c_void) {
    let startup = &mut *data.cast::<Startup>();
    assert_eq!(
        crate::gc::heap_stats().object_count,
        startup.objects,
        "bootstrap allocated managed adapter state before the module initializer"
    );
    startup.called = true;
}

#[test]
fn bootstrap_runs_the_initializer_before_allocating_a_managed_frame() {
    let _nursery = crate::gc::CopyingNurseryTestGuard::new(0);
    let _triggers = crate::gc::GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    crate::gc::gc_init();
    let mut startup = Startup {
        objects: crate::gc::heap_stats().object_count,
        called: false,
    };
    unsafe {
        assert!(perry_component_frame_bootstrap(std::ptr::null(), std::ptr::null_mut()).is_null());
        let pointer = perry_component_frame_bootstrap(
            check_startup as *const c_void,
            (&raw mut startup).cast(),
        );
        assert!(startup.called);
        assert!(crate::gc::heap_stats().object_count > startup.objects);
        assert_eq!(perry_component_frame_failed(pointer), 0);
        perry_component_frame_drop(pointer);
    }
}

unsafe extern "C-unwind" fn throw_during_startup(_: *mut c_void) {
    let value = crate::value::js_nanbox_pointer(crate::array::js_array_alloc_with_length(5) as i64);
    crate::exception::js_throw(value);
}

#[test]
fn bootstrap_keeps_an_initializer_exception_alive_through_frame_allocation_and_moving_gc() {
    let _nursery = crate::gc::CopyingNurseryTestGuard::new(0);
    let _triggers = crate::gc::GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let _evacuate = crate::gc::knob_overrides::ForcedEvacuationTestGuard::on();
    crate::gc::gc_init();
    unsafe {
        let pointer = perry_component_frame_bootstrap(
            throw_during_startup as *const c_void,
            std::ptr::null_mut(),
        );
        assert_eq!(perry_component_frame_failed(pointer), 1);
        let before = frame(pointer).error().unwrap().to_bits();
        crate::gc::gc_collect_minor();
        let after = frame(pointer).error().unwrap();
        assert_ne!(
            after.to_bits(),
            before,
            "the initializer exception must move"
        );
        assert_eq!(crate::array::js_array_length(array(after)), 5);
        perry_component_frame_drop(pointer);
    }
}
