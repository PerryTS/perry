use super::*;

fn mock_state_with_calls(call_count: usize, implementation: f64) -> MockState {
    let mut calls = crate::array::js_array_alloc(call_count as u32);
    for _ in 0..call_count {
        calls = crate::array::js_array_push_f64(calls, undefined_value());
    }
    MockState {
        id: 1,
        original: implementation,
        implementation,
        once: Vec::new(),
        calls: boxed_ptr(calls),
        context: undefined_value(),
        function: undefined_value(),
        restore: MockRestoreTarget::None,
    }
}

fn set_call_count(state: &mut MockState, call_count: usize) {
    let mut calls = crate::array::js_array_alloc(call_count as u32);
    for _ in 0..call_count {
        calls = crate::array::js_array_push_f64(calls, undefined_value());
    }
    state.calls = boxed_ptr(calls);
}

#[test]
fn default_once_scheduling_overwrites_the_current_call_index() {
    let mut state = mock_state_with_calls(0, 10.0);
    let call = mock_state_call_count(&state);
    schedule_mock_implementation_once(&mut state, call, 20.0);
    schedule_mock_implementation_once(&mut state, call, 30.0);

    assert_eq!(take_mock_implementation(&mut state), 30.0);
    assert_eq!(take_mock_implementation(&mut state), 10.0);
}

#[test]
fn indexed_once_scheduling_uses_absolute_call_indices() {
    let mut state = mock_state_with_calls(1, 10.0);
    schedule_mock_implementation_once(&mut state, 2, 20.0);
    schedule_mock_implementation_once(&mut state, 4, 30.0);

    assert_eq!(take_mock_implementation(&mut state), 10.0);
    set_call_count(&mut state, 2);
    assert_eq!(take_mock_implementation(&mut state), 20.0);
    set_call_count(&mut state, 3);
    assert_eq!(take_mock_implementation(&mut state), 10.0);
    set_call_count(&mut state, 4);
    assert_eq!(take_mock_implementation(&mut state), 30.0);
    set_call_count(&mut state, 5);
    assert_eq!(take_mock_implementation(&mut state), 10.0);
}

#[test]
fn restoring_a_mock_preserves_calls_and_scheduled_implementations() {
    let mut state = mock_state_with_calls(2, 10.0);
    state.implementation = 20.0;
    schedule_mock_implementation_once(&mut state, 4, 30.0);

    let _ = prepare_mock_state_restore(&mut state);

    assert_eq!(state.implementation, 10.0);
    assert_eq!(mock_state_call_count(&state), 2);
    assert_eq!(state.once, vec![(4, 30.0)]);
}

#[test]
fn reentrant_dispatch_uses_completed_call_indices_like_node() {
    let mut explicit = mock_state_with_calls(0, 10.0);
    schedule_mock_implementation_once(&mut explicit, 1, 20.0);

    // Node records a call only after its implementation completes. The outer
    // and nested dispatches therefore both observe completed-call index 0, so
    // an explicit onCall=1 entry is not consumed by either invocation.
    assert_eq!(take_mock_implementation(&mut explicit), 10.0);
    assert_eq!(take_mock_implementation(&mut explicit), 10.0);
    set_call_count(&mut explicit, 2);
    assert_eq!(take_mock_implementation(&mut explicit), 10.0);
    assert_eq!(explicit.once, vec![(1, 20.0)]);

    let mut scheduled_inside = mock_state_with_calls(0, 10.0);
    assert_eq!(take_mock_implementation(&mut scheduled_inside), 10.0);
    let current = mock_state_call_count(&scheduled_inside);
    schedule_mock_implementation_once(&mut scheduled_inside, current, 30.0);
    assert_eq!(take_mock_implementation(&mut scheduled_inside), 30.0);
}
