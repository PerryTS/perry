//! Families born on their declared layouts (`state::born`): every birth,
//! the first included, is on the layout the family declared with its
//! prototype, and the state is read and written in the object's own slots.
use super::*;

fn own_symbols(obj: *mut ObjectHeader) -> Vec<u64> {
    let value = boxed_ptr(obj);
    let symbols =
        unsafe { crate::symbol::js_object_get_own_property_symbols(value) } as *const ArrayHeader;
    (0..crate::array::js_array_length(symbols))
        .map(|i| crate::array::js_array_get_f64(symbols, i).to_bits())
        .collect()
}

type Family = (
    &'static state::Birth,
    &'static str,
    crate::object::BuiltinConstructorIdentity,
);

/// Every family with its constructor name and identity.
fn families() -> [Family; 5] {
    use crate::object::BuiltinConstructorIdentity as Id;
    [
        (&state::EVENT_TARGET_BIRTH, "EventTarget", Id::EventTarget),
        (&state::ABORT_SIGNAL_BIRTH, "AbortSignal", Id::AbortSignal),
        (
            &state::ABORT_CONTROLLER_BIRTH,
            "AbortController",
            Id::AbortController,
        ),
        (&state::EVENT_BIRTH, "Event", Id::Event),
        (&state::CUSTOM_EVENT_BIRTH, "CustomEvent", Id::CustomEvent),
    ]
}

#[test]
fn every_birth_is_on_the_declared_layout_the_first_included() {
    let _gc = crate::gc::GcSuppressScope::new();
    for (birth, name, identity) in families() {
        let first = state::born(birth);
        let declared = crate::array::realm_instance_layout(identity);
        assert_ne!(declared, 0, "{name} declares its layout with its prototype");
        let second = state::born(birth);
        unsafe {
            assert_eq!(crate::object::shapes::object_shape_stamp(first).0, declared);
            assert_eq!(
                crate::object::shapes::object_shape_stamp(second).0,
                declared
            );
        }
        assert_eq!(own_symbols(first), own_symbols(second));
        // These families do not use the prototype's birth record.
        let proto = crate::array::intrinsic_prototype_addr(identity) as *mut ObjectHeader;
        unsafe {
            let meta = crate::meta_read!(crate::meta_obj_runtime_born!(proto));
            assert!(meta.is_null() || (*meta).instance_birth == 0, "{name}");
        }
    }
    assert_eq!(
        own_symbols(state::born(&state::ABORT_SIGNAL_BIRTH)).len(),
        7
    );
    assert_eq!(
        own_symbols(state::born(&state::EVENT_TARGET_BIRTH)).len(),
        4
    );
    assert!(own_symbols(state::born(&state::ABORT_CONTROLLER_BIRTH)).is_empty());
}

/// The declared layout names the same object the generic construction
/// reaches (alloc, prototype link, one key definition per state key in
/// order): the same key list with its attributes, prototype identity, kind,
/// attribute summary, field representation, generation and integrity. The
/// one fact that differs is the carrier's inline bound: the generic sequence
/// starts from an object allocated with no slots, so its later keys live out
/// of line, while a declared birth holds every key inline.
#[test]
fn the_declared_layout_is_the_generic_construction_shape() {
    use crate::object::shapes::shape_descriptor_for_word;
    let _gc = crate::gc::GcSuppressScope::new();
    for (birth, name, identity) in families() {
        let born = state::born(birth);
        let declared =
            crate::codegen_abi::LayoutWord(crate::array::realm_instance_layout(identity));
        let generic = crate::object::js_object_alloc(unsafe { (*born).class_id }, 0);
        state::link(generic, name);
        for &(index, bits) in state::birth_slots(birth) {
            state::set(generic, index, f64::from_bits(bits));
        }
        let reached = unsafe { crate::object::shapes::object_shape_stamp(generic) };
        let d = shape_descriptor_for_word(declared).expect(name);
        let g = shape_descriptor_for_word(reached).expect(name);
        let n = state::birth_slots(birth).len() as u32;
        assert_eq!(d.keys, g.keys, "{name}: key list and attributes");
        assert_eq!((d.logical_key_count, g.logical_key_count), (n, n), "{name}");
        assert_eq!(d.proto_id, g.proto_id, "{name}: prototype identity");
        assert_eq!(d.object_kind, g.object_kind, "{name}");
        assert_eq!(d.summary, g.summary, "{name}");
        assert_eq!(
            crate::object::field_rep::identity_with_special(d.rep),
            crate::object::field_rep::identity_with_special(g.rep),
            "{name}"
        );
        assert_eq!(
            (d.semantic_generation, d.integrity_flags, d.hole_count),
            (g.semantic_generation, g.integrity_flags, g.hole_count),
            "{name}"
        );
        assert_eq!(d.live_inline_slot_count, n, "{name}: every key inline");
        assert_eq!(own_symbols(generic), own_symbols(born), "{name}");
    }
}

#[test]
fn an_event_is_born_with_its_type_and_options() {
    let _gc = crate::gc::GcSuppressScope::new();
    let kind = crate::string::js_string_from_bytes(b"ping".as_ptr(), 4);
    let event = js_event_new(
        crate::value::js_nanbox_string(kind as i64),
        f64::from_bits(crate::value::TAG_UNDEFINED),
        1,
    );
    unsafe {
        assert_eq!(
            crate::object::shapes::object_shape_stamp(event).0,
            crate::array::realm_instance_layout(crate::object::BuiltinConstructorIdentity::Event)
        );
    }
    let ty = state::get(event, state::TYPE);
    assert!(crate::value::JSValue::from_bits(ty.to_bits()).is_string());
    assert_eq!(
        state::get(event, state::BUBBLES).to_bits(),
        crate::value::TAG_FALSE
    );
    assert_eq!(
        state::get(event, state::TARGET).to_bits(),
        crate::value::TAG_NULL
    );
    assert!(!state::has(event, state::DETAIL));
}

#[test]
fn signal_state_is_its_own_slots() {
    let _gc = crate::gc::GcSuppressScope::new();
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
    let controller = crate::url::js_abort_controller_new();
    assert!(own_symbols(controller).is_empty());
    assert!(state::has(controller, state::CONTROLLER_SIGNAL));
    let signal = abort::js_abort_controller_signal(controller);
    assert_eq!(abort::js_abort_controller_signal(controller), signal);
    abort::js_abort_controller_abort_reason(controller, 3.0);
    assert_eq!(abort::js_abort_signal_is_aborted(signal), 1);
    assert_eq!(state::get(signal, state::SIGNAL_REASON), 3.0);
}
