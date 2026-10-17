use super::*;
use crate::object::layout_desc::PerryLayoutDesc;
use crate::object::*;

/// The `(atom, slot)` pairs of a descriptor, in key order.
unsafe fn slots(desc: *const PerryLayoutDesc) -> Vec<(*const StaticAtom, u32)> {
    let d = &*desc;
    (0..d.key_count as usize)
        .map(|i| {
            let key = &*d.keys.add(i);
            (key.atom, key.slot)
        })
        .collect()
}

/// Prefix stability: every key of `base` is a key of `sub` at the same slot,
/// and `sub`'s other keys take the slots after the base's.
unsafe fn extends(base: *const PerryLayoutDesc, sub: *const PerryLayoutDesc) -> Result<(), String> {
    let base = slots(base);
    let sub = slots(sub);
    for &(atom, slot) in &base {
        match sub.iter().find(|&&(a, _)| a == atom) {
            Some(&(_, s)) if s == slot => {}
            Some(&(_, s)) => return Err(format!("base slot {slot} moved to {s}")),
            None => return Err(format!("base slot {slot} missing")),
        }
    }
    for &(atom, slot) in &sub {
        if !base.iter().any(|&(a, _)| a == atom) && (slot as usize) < base.len() {
            return Err(format!("appended key takes base slot {slot}"));
        }
    }
    Ok(())
}

#[test]
fn stream_instance_layouts_are_prefix_stable() {
    crate::gc::initialize_runtime_fixture();
    let emitter = &raw const EVENT_EMITTER;
    unsafe {
        for desc in [
            &raw const EVENT_EMITTER,
            &raw const LEGACY_STREAM,
            &raw const READABLE,
            &raw const WRITABLE,
            &raw const DUPLEX,
            &raw const TRANSFORM,
        ] {
            assert_ne!(layout_id(desc), 0, "the layout registers");
            extends(emitter, desc).unwrap();
        }
        extends(&raw const READABLE, &raw const DUPLEX).unwrap();
        extends(&raw const DUPLEX, &raw const TRANSFORM).unwrap();
    }
}

runtime_atom!(
    PREFIX_STATE,
    "prefixtest_state",
    "707265666978746573745f7374617465"
);
// The emitter's keys with a subclass key enumerated between them, stored by
// position: the base keys move.
runtime_layout!(
    PREFIX_MOVED,
    0x7f0000b2,
    0,
    crate::object::layout_desc::LAYOUT_PROTO_OBJECT,
    [
        NP_ATOM_U_EVENTS,
        PREFIX_STATE,
        NP_ATOM_U_EVENTS_COUNT,
        NP_ATOM_U_MAX_LISTENERS,
    ]
);

#[test]
fn prefix_stability_check_rejects_a_moved_base_slot() {
    unsafe {
        assert_eq!(
            extends(&raw const EVENT_EMITTER, &raw const PREFIX_MOVED),
            Err("base slot 1 moved to 2".to_string())
        );
    }
}

unsafe fn string(value: crate::JSValue) -> String {
    let mut sso = [0; crate::value::SHORT_STRING_MAX_LEN];
    String::from_utf8(
        crate::string::js_string_key_bytes(value, &mut sso)
            .expect("string key")
            .to_vec(),
    )
    .unwrap()
}

unsafe fn own_names(value: f64) -> Vec<String> {
    let own = js_object_get_own_property_names(value);
    let array = (own.to_bits() & crate::value::POINTER_MASK) as *mut crate::ArrayHeader;
    (0..crate::array::js_array_length(array))
        .map(|i| string(crate::array::js_array_get(array, i)))
        .collect()
}

fn undefined() -> f64 {
    f64::from_bits(crate::value::TAG_UNDEFINED)
}

fn object(value: f64) -> *mut ObjectHeader {
    (value.to_bits() & crate::value::POINTER_MASK) as *mut ObjectHeader
}

#[test]
fn direct_births_are_born_on_their_layouts_in_node_order() {
    crate::gc::initialize_runtime_fixture();
    let scope = crate::gc::RuntimeHandleScope::new();
    let births: [(extern "C" fn(f64) -> f64, *const PerryLayoutDesc, &[&str]); 5] = [
        (
            crate::node_stream::js_event_emitter_object_new,
            &raw const EVENT_EMITTER,
            &["_events", "_eventsCount", "_maxListeners"],
        ),
        (
            crate::node_stream::js_node_stream_readable_new,
            &raw const READABLE,
            &["_events", "_readableState", "_maxListeners"],
        ),
        (
            crate::node_stream::js_node_stream_writable_new,
            &raw const WRITABLE,
            &["_events", "_writableState", "_maxListeners"],
        ),
        (
            crate::node_stream::js_node_stream_duplex_new,
            &raw const DUPLEX,
            &[
                "_events",
                "_readableState",
                "_writableState",
                "allowHalfOpen",
                "_maxListeners",
            ],
        ),
        (
            crate::node_stream::js_node_stream_transform_new,
            &raw const TRANSFORM,
            &[
                "_events",
                "_readableState",
                "_writableState",
                "allowHalfOpen",
                "_maxListeners",
                "_eventsCount",
            ],
        ),
    ];
    for (new, desc, names) in births {
        let value = scope.root_nanbox_f64(new(undefined()));
        unsafe {
            let obj = object(value.get_nanbox_f64());
            assert_eq!(
                (*obj).parent_class_id,
                layout_id(desc),
                "born on its layout"
            );
            assert!(!crate::object::overflow::has_overflow(obj));
            assert_eq!(own_names(value.get_nanbox_f64()), names);
        }
    }
}

/// A fill of an optional member appends, as node's `[[Set]]` does: in place
/// when it is the last key, else through the ordered overflow.
#[test]
fn filled_optional_members_enumerate_last() {
    crate::gc::initialize_runtime_fixture();
    let scope = crate::gc::RuntimeHandleScope::new();
    unsafe {
        let r = scope.root_nanbox_f64(crate::node_stream::js_node_stream_readable_new(undefined()));
        let count = crate::string::intern_ascii_literal(b"_eventsCount");
        js_object_set_field_by_name(object(r.get_nanbox_f64()), count, 1.0);
        assert_eq!(
            own_names(r.get_nanbox_f64()),
            ["_events", "_readableState", "_maxListeners", "_eventsCount"]
        );
        assert!(!crate::object::overflow::has_overflow(object(
            r.get_nanbox_f64()
        )));
        let read = crate::string::intern_ascii_literal(b"_read");
        js_object_set_field_by_name(object(r.get_nanbox_f64()), read, 2.0);
        assert_eq!(
            own_names(r.get_nanbox_f64()),
            [
                "_events",
                "_readableState",
                "_maxListeners",
                "_eventsCount",
                "_read"
            ]
        );
        assert_eq!(
            js_object_get_field_by_name_f64(object(r.get_nanbox_f64()), read),
            2.0
        );
    }
}

#[test]
fn legacy_stream_takes_the_emitter_birth() {
    crate::gc::initialize_runtime_fixture();
    let scope = crate::gc::RuntimeHandleScope::new();
    let ctor = scope.root_nanbox_f64(crate::object::bound_native_callable_export_value(
        "stream", "Stream",
    ));
    let proto = crate::closure::closure_get_dynamic_prop(
        (ctor.get_nanbox_u64() & crate::value::POINTER_MASK) as usize,
        "prototype",
    );
    let stream = scope.root_nanbox_f64(crate::node_stream::emitter_new(
        proto,
        undefined(),
        &raw const LEGACY_STREAM,
    ));
    unsafe {
        assert_eq!(
            own_names(stream.get_nanbox_f64()),
            ["_events", "_eventsCount", "_maxListeners"]
        );
        assert_eq!(
            (*object(stream.get_nanbox_f64())).parent_class_id,
            layout_id(&raw const LEGACY_STREAM)
        );
        assert_eq!(
            js_object_get_prototype_of(stream.get_nanbox_f64()).to_bits(),
            proto.to_bits()
        );
    }
}

/// A prototype derived from `EventEmitter.prototype` is not the sealed one:
/// the birth takes the ordinary `init` on `Object.create(proto)`.
#[test]
fn emitter_birth_declines_a_derived_prototype() {
    crate::gc::initialize_runtime_fixture();
    let scope = crate::gc::RuntimeHandleScope::new();
    let base = crate::node_stream::event_emitter_prototype_value("EventEmitter");
    let derived = scope.root_nanbox_f64(crate::object::js_object_create(base));
    let emitter = scope.root_nanbox_f64(crate::node_stream::emitter_new(
        derived.get_nanbox_f64(),
        undefined(),
        &raw const EVENT_EMITTER,
    ));
    unsafe {
        assert_ne!(
            (*object(emitter.get_nanbox_f64())).parent_class_id,
            layout_id(&raw const EVENT_EMITTER)
        );
        assert_eq!(
            js_object_get_prototype_of(emitter.get_nanbox_f64()).to_bits(),
            derived.get_nanbox_u64()
        );
        assert_eq!(
            own_names(emitter.get_nanbox_f64()),
            ["_events", "_eventsCount", "_maxListeners"]
        );
    }
}

/// A write to a sealed prototype of the chain (its shadow bit) sends the
/// birth down the ordinary construction, with the same own keys.
#[test]
fn readable_birth_declines_a_written_chain() {
    // A fresh thread: the write stays on its own agent prototypes.
    std::thread::Builder::new()
        .stack_size(16 << 20)
        .spawn(readable_birth_declines_a_written_chain_body)
        .unwrap()
        .join()
        .unwrap();
}

fn readable_birth_declines_a_written_chain_body() {
    crate::gc::initialize_runtime_fixture();
    let scope = crate::gc::RuntimeHandleScope::new();
    unsafe {
        let ee = scope.root_nanbox_f64(crate::node_stream::event_emitter_prototype_value(
            "EventEmitter",
        ));
        let key = crate::string::intern_ascii_literal(b"layouttest_written");
        js_object_set_field_by_name(object(ee.get_nanbox_f64()), key, 1.0);
        let r = scope.root_nanbox_f64(crate::node_stream::js_node_stream_readable_new(undefined()));
        assert_ne!(
            (*object(r.get_nanbox_f64())).parent_class_id,
            layout_id(&raw const READABLE)
        );
        assert_eq!(
            own_names(r.get_nanbox_f64()),
            ["_events", "_readableState", "_maxListeners"]
        );
    }
}
