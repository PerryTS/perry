//! Promise-owned overflow records retain callback and context children.
use super::super::*;
use super::support::*;
extern "C" fn handler(
    _closure: *const crate::closure::ClosureHeader,
    _this: crate::closure::JsThis,
) -> f64 {
    37.0
}

#[test]
fn overflow_children_follow_the_owner() {
    check_owned_overflow(false);
}
#[test]
#[ignore = "negative control: erase the moved Promise's overflow list"]
fn negative_control_objtables_promise_overflow() {
    check_owned_overflow(true);
}

fn check_owned_overflow(erase: bool) {
    let _guard = CopyingNurseryTestGuard::new(1);
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let owner = crate::promise::js_promise_new();
    let cb = crate::closure::js_closure_alloc(crate::fn_info!(handler, 0), 0);
    let next = crate::promise::js_promise_new();
    let child = crate::object::js_object_alloc(0, 0);
    let context = crate::async_context::test_snapshot_with_store(f64::from_bits(
        crate::value::POINTER_TAG | child as u64,
    ));
    crate::promise::reactions::push_overflow_reaction(owner, cb, std::ptr::null(), next, context);
    js_shadow_slot_set(0, crate::value::POINTER_TAG | owner as u64);
    let _ = gc_collect_minor();
    let moved =
        (js_shadow_slot_get(0) & crate::value::POINTER_MASK) as *mut crate::promise::Promise;
    assert_ne!(moved, owner);
    if erase {
        unsafe {
            (*(*moved).meta).owned_state = 0;
        }
    }
    let (cb_after, context_after) = {
        let records = crate::promise::reactions::promise_take_overflow_reactions(moved);
        assert_eq!(records.len(), 1);
        assert_ne!(records[0].next, next);
        assert!(records[0].on_rejected.is_null());
        (records[0].on_fulfilled, records[0].context.clone())
    };
    assert_ne!(cb_after, cb as *const crate::closure::ClosureHeader);
    let child_after = crate::async_context::test_snapshot_first_store(&context_after).unwrap();
    assert_ne!(
        child_after.to_bits() & crate::value::POINTER_MASK,
        child as u64
    );
    let fresh = crate::promise::js_promise_new();
    assert!(crate::promise::reactions::promise_take_settle_listeners(fresh).is_empty());
    assert!(crate::promise::reactions::promise_take_overflow_reactions(fresh).is_empty());
    assert!(unsafe { (*fresh).meta }.is_null());
}
