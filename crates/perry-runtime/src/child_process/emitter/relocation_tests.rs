use super::*;

/// Simulate the runtime-root rewrite performed by a moving collection inside
/// the first listener, without requiring a native stack map in a Rust test.
extern "C" fn relocate(closure: *const ClosureHeader, _arg: f64) -> f64 {
    for pair in 0..2 {
        let source = js_closure_get_capture_ptr(closure, pair * 2) as *mut u8;
        let destination = js_closure_get_capture_ptr(closure, pair * 2 + 1) as *mut u8;
        unsafe {
            // Both addresses originate from js_object_alloc in this fixture.
            let header = crate::gc::header_from_trusted_user_ptr(source).cast_mut();
            crate::gc::set_forwarding_address(header, destination);
        }
    }
    crate::gc::test_rewrite_runtime_handles_for_forwarded_objects();
    cp_undefined()
}

extern "C" fn observe(_closure: *const ClosureHeader, arg: f64) -> f64 {
    let target = crate::object::js_implicit_this_get();
    cp_set_field(target, b"seen", arg);
    cp_undefined()
}

struct RestoreForwarding([(*mut u8, usize); 2]);

impl Drop for RestoreForwarding {
    fn drop(&mut self) {
        for (source, first_word) in self.0 {
            unsafe {
                // GC_STORE_AUDIT(POINTER_FREE): restore the original object
                // header word after this synthetic forwarding-only test.
                source.cast::<usize>().write(first_word);
                // Restore the same fixture-owned js_object_alloc allocation.
                let header = crate::gc::header_from_trusted_user_ptr(source).cast_mut();
                (*header).gc_flags &= !crate::gc::GC_FLAG_FORWARDED;
            }
        }
    }
}

#[test]
fn child_dispatch_reloads_receiver_and_arguments_after_listener_relocation() {
    cp_register_arities();
    js_register_closure_arity(relocate as *const u8, 1);
    js_register_closure_arity(observe as *const u8, 1);
    let scope = crate::gc::RuntimeHandleScope::new();
    let source = scope.root_nanbox_f64(cp_box_ptr(crate::object::js_object_alloc(0, 0).cast()));
    let destination =
        scope.root_nanbox_f64(cp_box_ptr(crate::object::js_object_alloc(0, 0).cast()));
    let argument = scope.root_nanbox_f64(cp_box_ptr(crate::object::js_object_alloc(0, 0).cast()));
    let moved_argument =
        scope.root_nanbox_f64(cp_box_ptr(crate::object::js_object_alloc(0, 0).cast()));
    let first = scope.root_nanbox_f64(cp_box_ptr(
        js_closure_alloc(relocate as *const u8, 4).cast(),
    ));
    let second =
        scope.root_nanbox_f64(cp_box_ptr(js_closure_alloc(observe as *const u8, 0).cast()));
    let event = scope.root_nanbox_f64(cp_box_string("end"));
    for target in [&source, &destination] {
        cp_register(
            target.get_nanbox_f64(),
            event.get_nanbox_f64(),
            first.get_nanbox_f64(),
        );
        cp_register(
            target.get_nanbox_f64(),
            event.get_nanbox_f64(),
            second.get_nanbox_f64(),
        );
    }
    let sources = [source.get_nanbox_f64(), argument.get_nanbox_f64()]
        .map(|v| crate::value::js_nanbox_get_pointer(v) as *mut u8);
    let destinations = [
        destination.get_nanbox_f64(),
        moved_argument.get_nanbox_f64(),
    ]
    .map(|v| crate::value::js_nanbox_get_pointer(v) as *mut u8);
    let _restore = RestoreForwarding(sources.map(|p| (p, unsafe { p.cast::<usize>().read() })));
    let callback =
        crate::value::js_nanbox_get_pointer(first.get_nanbox_f64()) as *mut ClosureHeader;
    for pair in 0..2 {
        js_closure_set_capture_ptr(callback, pair * 2, sources[pair as usize] as i64);
        js_closure_set_capture_ptr(callback, pair * 2 + 1, destinations[pair as usize] as i64);
    }
    assert!(cp_emit(
        source.get_nanbox_f64(),
        "end",
        &[argument.get_nanbox_f64()]
    ));
    assert_eq!(
        source.get_nanbox_f64().to_bits(),
        destination.get_nanbox_f64().to_bits()
    );
    assert_eq!(
        cp_get_field(destination.get_nanbox_f64(), b"seen").to_bits(),
        moved_argument.get_nanbox_f64().to_bits()
    );
}
