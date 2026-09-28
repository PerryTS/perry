//! Design step 4 §7.5: a static ShapeId means the same facts in every agent,
//! and each agent mints its own record under it. The serializer carries keys
//! and values, never a ShapeId, so a replayed object is minted BY FACTS in the
//! receiving agent on first sight. Both orders are pinned: a receiving agent
//! that ran the seed resolves the replayed object to the static id; one that
//! did not mints a counter id and never holds the static id under any facts.
use super::*;
use crate::object::shapes::{
    is_shape_id, is_static_shape_id, object_shape_stamp, shape_descriptor_by_id, SHAPE_ID_BASE,
};

const NAMES: [&str; 2] = ["lt4w_left", "lt4w_right"];
const REQUESTED: u32 = SHAPE_ID_BASE + 0x4567;

fn seed_here() -> u32 {
    let packed: Vec<u8> = NAMES
        .iter()
        .flat_map(|n| n.bytes().chain(std::iter::once(0)))
        .collect();
    crate::object::static_shapes::js_shape_seed_plain(
        REQUESTED,
        packed.as_ptr(),
        packed.len() as u32,
        2,
    )
}

/// An object carrying the seeded shape in THIS agent, serialized for a worker.
fn replayable() -> SerializedValue {
    assert_eq!(seed_here(), REQUESTED, "sending agent: seed did not adopt");
    unsafe {
        let obj = crate::object::js_object_alloc_with_parent(0, 0, 2);
        let keys = crate::object::static_shapes::canonical_keys_for_names(&[
            NAMES[0].as_bytes(),
            NAMES[1].as_bytes(),
        ]);
        crate::object::js_object_set_keys(obj, keys.arr() as *mut _);
        assert_eq!(
            object_shape_stamp(obj),
            REQUESTED,
            "fixture is vacuous: the sent object does not carry the static id"
        );
        let bits = crate::value::js_nanbox_pointer(obj as i64).to_bits();
        let wire = serialize_nanbox_for_thread(bits);
        if let SerializedValue::Object {
            parent_class_id, ..
        } = &wire
        {
            assert!(
                !is_shape_id(*parent_class_id),
                "a ShapeId reached the wire; the receiver would replay it verbatim"
            );
        } else {
            panic!("expected an object on the wire");
        }
        wire
    }
}

/// Runs in a fresh agent (a new thread has its own runtime state).
fn receive(wire: SerializedValue, seed_first: bool) -> (u32, bool, bool) {
    std::thread::spawn(move || {
        if seed_first {
            assert_eq!(
                seed_here(),
                REQUESTED,
                "receiving agent: seed did not adopt"
            );
        }
        let bits = unsafe { deserialize_nanbox_on_current_thread(&wire) };
        let obj =
            crate::value::JSValue::from_bits(bits).as_pointer::<crate::object::ObjectHeader>();
        let stamp = unsafe { object_shape_stamp(obj) };
        let static_present = shape_descriptor_by_id(REQUESTED).is_some();
        // Same facts = the stamp names this agent's canonical node for NAMES.
        let canonical = unsafe {
            crate::object::static_shapes::canonical_keys_for_names(&[
                NAMES[0].as_bytes(),
                NAMES[1].as_bytes(),
            ])
        };
        let same_keys = shape_descriptor_by_id(stamp)
            .is_some_and(|d| d.keys == canonical.arr() as usize as u64);
        (stamp, static_present, same_keys)
    })
    .join()
    .expect("receiving agent panicked")
}

#[test]
fn a_seeded_worker_resolves_a_replayed_object_to_the_static_id() {
    let (stamp, present, keys) = receive(replayable(), true);
    assert!(
        keys,
        "the replayed object is not stamped with the canonical key list"
    );
    assert_eq!(
        stamp, REQUESTED,
        "the receiving agent ran the seed, so minting the replayed object's facts must find it"
    );
    assert!(present);
}

#[test]
fn an_unseeded_worker_mints_a_replayed_object_by_facts_and_never_aliases_the_static_id() {
    let (stamp, present, keys) = receive(replayable(), false);
    assert!(
        keys,
        "the replayed object is not stamped with the canonical key list"
    );
    assert!(
        is_shape_id(stamp) && !is_static_shape_id(stamp),
        "an unseeded agent must mint a counter id by facts, got {stamp:#x}"
    );
    assert!(
        !present,
        "the static id exists in an agent that never seeded it — a compare could hit wrong facts"
    );
}
