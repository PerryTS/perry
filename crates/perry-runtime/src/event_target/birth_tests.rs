//! Families born in their final layout (`state::born`): the second birth
//! replays the shape the first one reached, and the state is read and
//! written in the object's own slots.
use super::*;

fn own_symbols(obj: *mut ObjectHeader) -> Vec<u64> {
    let value = boxed_ptr(obj);
    let symbols =
        unsafe { crate::symbol::js_object_get_own_property_symbols(value) } as *const ArrayHeader;
    (0..crate::array::js_array_length(symbols))
        .map(|i| crate::array::js_array_get_f64(symbols, i).to_bits())
        .collect()
}

/// Does `%name.prototype%` record a birth shape for `class_id`?
fn recorded(identity: crate::object::BuiltinConstructorIdentity, class_id: u32) -> bool {
    let proto = crate::array::intrinsic_prototype_addr(identity) as *mut ObjectHeader;
    assert!(!proto.is_null());
    unsafe {
        let meta = crate::meta_read!(crate::meta_obj_runtime_born!(proto));
        !meta.is_null() && (*meta).instance_birth as u32 == class_id
    }
}

#[test]
fn signal_and_target_births_replay_their_recorded_shape() {
    let _gc = crate::gc::GcSuppressScope::new();
    for birth in [&state::ABORT_SIGNAL_BIRTH, &state::EVENT_TARGET_BIRTH] {
        let first = state::born(birth);
        let second = state::born(birth);
        let third = state::born(birth);
        unsafe {
            let a = crate::object::shapes::object_shape_stamp(first);
            assert_eq!(a, crate::object::shapes::object_shape_stamp(second));
            assert_eq!(a, crate::object::shapes::object_shape_stamp(third));
        }
        let keys = own_symbols(first);
        assert_eq!(keys, own_symbols(second));
        assert_eq!(
            keys.len(),
            if std::ptr::eq(birth, &state::ABORT_SIGNAL_BIRTH) {
                7
            } else {
                4
            }
        );
        assert_eq!(
            state::get(second, state::MAX_LISTENERS).to_bits(),
            state::get(first, state::MAX_LISTENERS).to_bits()
        );
        assert!(state::has(third, state::LISTENERS));
        assert!(!state::has(third, state::CONTROLLER_SIGNAL));
    }
    use crate::object::BuiltinConstructorIdentity as Id;
    assert!(recorded(
        Id::AbortSignal,
        crate::native_class_ids::ABORT_SIGNAL
    ));
    assert!(recorded(
        Id::EventTarget,
        crate::native_class_ids::EVENT_TARGET
    ));
}

#[test]
fn signal_state_is_its_own_slots() {
    let _gc = crate::gc::GcSuppressScope::new();
    let _ = state::born(&state::ABORT_SIGNAL_BIRTH);
    let signal = state::born(&state::ABORT_SIGNAL_BIRTH);
    assert!(
        crate::value::JSValue::from_bits(state::get(signal, state::SIGNAL_ABORTED).to_bits())
            .is_bool()
    );
    assert_eq!(abort::js_abort_signal_is_aborted(signal), 0);
    let before = unsafe { crate::object::shapes::object_shape_stamp(signal) };
    state::set(signal, state::SIGNAL_ABORTED, bool_value(true));
    state::set(signal, state::SIGNAL_REASON, 7.0);
    assert_eq!(abort::js_abort_signal_is_aborted(signal), 1);
    assert_eq!(state::get(signal, state::SIGNAL_REASON), 7.0);
    // A value store keeps the layout.
    assert_eq!(before, unsafe {
        crate::object::shapes::object_shape_stamp(signal)
    });
}

#[test]
fn controller_signal_stays_a_private_slot() {
    let _gc = crate::gc::GcSuppressScope::new();
    let _ = crate::url::js_abort_controller_new();
    let controller = crate::url::js_abort_controller_new();
    assert!(own_symbols(controller).is_empty());
    assert!(recorded(
        crate::object::BuiltinConstructorIdentity::AbortController,
        crate::native_class_ids::ABORT_CONTROLLER
    ));
    assert!(state::has(controller, state::CONTROLLER_SIGNAL));
    let signal = abort::js_abort_controller_signal(controller);
    assert_eq!(abort::js_abort_controller_signal(controller), signal);
    abort::js_abort_controller_abort_reason(controller, 3.0);
    assert_eq!(abort::js_abort_signal_is_aborted(signal), 1);
    assert_eq!(state::get(signal, state::SIGNAL_REASON), 3.0);
}
