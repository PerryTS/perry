//! Pending Promise.all inputs own their result machinery through a GC list.
use super::super::*;
use super::support::*;

#[test]
fn promise_all_state_children_follow_the_pending_input() {
    check_owned_all(false);
}

#[test]
#[ignore = "negative control: erase the moved Promise's combinator list"]
fn negative_control_objtables_promise_all() {
    check_owned_all(true);
}

fn check_owned_all(erase: bool) {
    let _guard = CopyingNurseryTestGuard::new(1);
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let owner = crate::promise::js_promise_new();
    let result = crate::promise::js_promise_new();
    let results = crate::array::js_array_alloc_with_length(2);
    let state = crate::array::js_array_alloc_with_length(2);
    unsafe {
        crate::promise::combinators::park_all_state(
            owner,
            crate::promise::combinators::PromiseAllState {
                result_promise: result,
                results_arr: results,
                state_arr: state,
                index: 1,
            },
        );
    }
    js_shadow_slot_set(0, crate::value::POINTER_TAG | owner as u64);
    let _ = gc_collect_minor();
    let moved =
        (js_shadow_slot_get(0) & crate::value::POINTER_MASK) as *mut crate::promise::Promise;
    assert_ne!(owner, moved);
    if erase {
        unsafe {
            (*(*moved).meta).owned_state = 0;
        }
    }
    let records = crate::promise::combinators::promise_all_take_all_handlers(moved);
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].index, 1);
    assert_ne!(records[0].result_promise, result);
    assert_ne!(records[0].results_arr, results);
    assert_ne!(records[0].state_arr, state);
    assert!(crate::promise::combinators::promise_all_take_all_handlers(moved).is_empty());
    let fresh = crate::promise::js_promise_new();
    assert!(crate::promise::combinators::promise_all_take_all_handlers(fresh).is_empty());
    assert!(unsafe { (*fresh).meta }.is_null());
}
