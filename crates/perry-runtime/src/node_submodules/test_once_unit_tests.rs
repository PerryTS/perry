use super::*;

thread_local! {
    static REENTRANT_MOCK: Cell<f64> = const { Cell::new(f64::from_bits(TAG_UNDEFINED)) };
    static REENTRANT_ACTIVE: Cell<bool> = const { Cell::new(false) };
}

extern "C" fn reentrant_implementation(_closure: *const ClosureHeader) -> f64 {
    let recurse = REENTRANT_ACTIVE.with(|active| !active.replace(true));
    if recurse {
        let mock = REENTRANT_MOCK.with(Cell::get);
        js_closure_call0(raw_ptr_from_value(mock) as *const ClosureHeader);
        REENTRANT_ACTIVE.with(|active| active.set(false));
    }
    10.0
}

extern "C" fn return_twenty(_closure: *const ClosureHeader) -> f64 {
    20.0
}

extern "C" fn return_thirty(_closure: *const ClosureHeader) -> f64 {
    30.0
}

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
    MOCK_STATES.with(|states| states.borrow_mut().clear());
    let outer = closure_value(reentrant_implementation as *const u8, 0);
    let explicit_once = closure_value(return_twenty as *const u8, 0);
    let scheduled_inside = closure_value(return_thirty as *const u8, 0);
    let mock = create_mock_function(outer, outer, MockRestoreTarget::None);
    let mock_ptr = raw_ptr_from_value(mock) as *const ClosureHeader;
    let id = closure_id(mock_ptr);
    REENTRANT_MOCK.with(|slot| slot.set(mock));
    REENTRANT_ACTIVE.with(|active| active.set(false));

    // Node records a call only after its implementation completes. The outer
    // and nested dispatches therefore both observe completed-call index 0, so
    // an explicit onCall=1 entry is not consumed by either invocation.
    MOCK_STATES.with(|states| {
        let mut states = states.borrow_mut();
        let state = states.iter_mut().find(|state| state.id == id).unwrap();
        schedule_mock_implementation_once(state, 1, explicit_once);
    });
    assert_eq!(js_closure_call0(mock_ptr), 10.0);

    MOCK_STATES.with(|states| {
        let mut states = states.borrow_mut();
        let state = states.iter_mut().find(|state| state.id == id).unwrap();
        assert_eq!(mock_state_call_count(state), 2);
        assert_eq!(state.once.len(), 1);
        assert_eq!(state.once[0].0, 1);
        assert_eq!(state.once[0].1.to_bits(), explicit_once.to_bits());

        let current = mock_state_call_count(state);
        schedule_mock_implementation_once(state, current, scheduled_inside);
    });
    assert_eq!(js_closure_call0(mock_ptr), 30.0);

    MOCK_STATES.with(|states| {
        let states = states.borrow();
        let state = states.iter().find(|state| state.id == id).unwrap();
        assert_eq!(mock_state_call_count(state), 3);
        assert_eq!(state.once.len(), 1);
        assert_eq!(state.once[0].0, 1);
        assert_eq!(state.once[0].1.to_bits(), explicit_once.to_bits());
    });
    MOCK_STATES.with(|states| states.borrow_mut().clear());
}
