use super::cleanup_tests::{record_drop, TestOps, COLLECTED};
use super::*;
use crate::component::guest::frame as abi;
use std::sync::atomic::Ordering;

unsafe extern "C" fn record_borrow_drop(
    pointer: *mut c_void,
    ty: u32,
    operation: u32,
    handle: u32,
) -> u32 {
    assert_eq!(operation, 2);
    record_drop(pointer, ty, 1, handle)
}

#[test]
fn canonical_return_expires_only_its_borrows_while_task_values_remain_rooted() {
    let _nursery = crate::gc::CopyingNurseryTestGuard::new(0);
    let _triggers = crate::gc::GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let _evacuate = crate::gc::knob_overrides::ForcedEvacuationTestGuard::on();
    crate::gc::gc_init();
    let scope = RuntimeHandleScope::new();
    let _registry = Registry::new(&scope);
    let _ops = TestOps::new(record_borrow_drop);
    unsafe {
        let task = abi::perry_component_frame_task_new();
        let sibling = abi::perry_component_frame_task_new();
        let borrowed = perry_component_frame_resource_lift(task, 4, 1, 0, 0);
        let local = perry_component_frame_resource_lift(task, 9, 0, 23, 0);
        let owner = perry_component_frame_resource_lift(task, 4, 2, 0, 1);
        let other = perry_component_frame_resource_lift(sibling, 4, 3, 0, 0);
        let before = abi::frame(task).get(borrowed).to_bits();
        crate::gc::gc_collect_minor();
        assert_ne!(before, abi::frame(task).get(borrowed).to_bits());
        assert_eq!(abi::perry_component_frame_resource_scope_end(task), 0);
        for slot in [borrowed, local] {
            assert!(crate::exception::catch_js_throw(|| live(abi::frame(task).get(slot))).is_err());
        }
        assert_eq!(get(live(abi::frame(task).get(owner)), STATE), OWNED);
        assert_eq!(get(live(abi::frame(sibling).get(other)), HANDLE), 3.0);
        assert_eq!(COLLECTED.load(Ordering::SeqCst), 1 << 8);
        assert_eq!(abi::perry_component_frame_resource_scope_end(task), 0);
        abi::perry_component_frame_drop(task);
        assert_eq!(COLLECTED.load(Ordering::SeqCst), 1 << 8);
        abi::perry_component_frame_drop(sibling);
        assert_eq!(COLLECTED.load(Ordering::SeqCst), (1 << 8) + (1 << 24));
    }
}

#[test]
fn canonical_borrow_cleanup_preserves_an_earlier_error_and_drops_each_handle_once() {
    crate::test_isolation::in_own_process(|| {
        crate::gc::gc_init();
        let scope = RuntimeHandleScope::new();
        let _registry = Registry::new(&scope);
        let _ops = TestOps::new(record_borrow_drop);
        unsafe {
            let task = abi::perry_component_frame_task_new();
            for _ in 0..2 {
                perry_component_frame_resource_lift(task, 4, 1, 0, 0);
            }
            let error = scope.root_nanbox_f64(crate::value::js_nanbox_string(
                crate::string::js_string_from_bytes(b"original".as_ptr(), 8) as i64,
            ));
            abi::frame(task).guard::<()>(|_| crate::exception::js_throw(error.get_nanbox_f64()));
            assert_eq!(abi::perry_component_frame_resource_scope_end(task), -1);
            assert_eq!(
                abi::frame(task).error().unwrap().to_bits(),
                error.get_nanbox_f64().to_bits()
            );
            abi::perry_component_frame_drop(task);
            assert_eq!(COLLECTED.load(Ordering::SeqCst), 1 << 8);
        }
    });
}
