//! The static-id seeds (design step 4, §7.3): the mint adopts a requested
//! static id only on a by-facts miss, never twice, never outside its band.
use super::*;
use crate::object::shapes::{is_shape_id, is_static_shape_id, SHAPE_ID_BASE, STATIC_SHAPE_ID_END};

fn seed(requested: u32, names: &[&str]) -> u32 {
    let packed: Vec<u8> = names
        .iter()
        .flat_map(|n| n.bytes().chain(std::iter::once(0)))
        .collect();
    js_shape_seed_plain(
        requested,
        packed.as_ptr(),
        packed.len() as u32,
        names.len() as u32,
    )
}

fn is_carrier(id: u32) -> bool {
    shapes::test_shape_record_is_carrier(id)
}

#[test]
fn a_seed_mints_the_requested_id_and_every_later_mint_of_those_facts_resolves_to_it() {
    let requested = SHAPE_ID_BASE + 0x1234;
    let id = seed(requested, &["lt4s_a", "lt4s_b"]);
    assert_eq!(id, requested, "the seed must mint the requested static id");
    assert!(
        is_carrier(id),
        "a seeded record must carry RECORD_FLAG_EXTERNAL_CARRIER (never pruned)"
    );
    // The ordinary literal birth of the same key list (the canonical node,
    // count = live bound, proto 0) reaches the seeded record.
    let keys = unsafe { canonical_keys_for_names(&[b"lt4s_a", b"lt4s_b"]) };
    assert_eq!(shapes::shape_id_for_keys_ensure(keys.arr(), 2), requested);
    // A second seed of the same facts under another id finds the first.
    assert_eq!(
        seed(SHAPE_ID_BASE + 0x1235, &["lt4s_a", "lt4s_b"]),
        requested
    );
    assert!(
        shapes::shape_descriptor_by_id(SHAPE_ID_BASE + 0x1235).is_none(),
        "a by-facts hit must not mint the second requested id"
    );
}

#[test]
fn a_static_id_already_naming_other_facts_is_declined() {
    let requested = SHAPE_ID_BASE + 0x2345;
    assert_eq!(seed(requested, &["lt4d_x"]), requested);
    let other = seed(requested, &["lt4d_y"]);
    assert_ne!(other, requested, "one static id named two sets of facts");
    assert!(is_shape_id(other) && !is_static_shape_id(other));
}

#[test]
fn a_requested_id_outside_the_static_band_is_declined() {
    for bad in [0x7000_0000u32, STATIC_SHAPE_ID_END, STATIC_SHAPE_ID_END + 5] {
        let id = seed(bad, &["lt4b_q", &format!("lt4b_{bad:x}")]);
        assert!(is_shape_id(id), "no shape minted for {bad:#x}");
        assert!(
            !is_static_shape_id(id),
            "a counter mint landed in the static band"
        );
        assert!(
            bad == STATIC_SHAPE_ID_END || id != bad,
            "an out-of-band request {bad:#x} was adopted"
        );
    }
}

#[test]
fn the_counter_never_mints_into_the_static_band() {
    let keys = unsafe { canonical_keys_for_names(&[b"lt4c_only"]) };
    let id = shapes::shape_id_for_keys_ensure(keys.arr(), 1);
    assert!(is_shape_id(id) && id >= STATIC_SHAPE_ID_END, "{id:#x}");
}

#[test]
fn a_class_seed_takes_the_class_prototype_identity() {
    const CLASS_ID: u32 = 0x0074_1c11;
    let packed = b"lt4k_a\0lt4k_b\0";
    let keys = crate::object::js_build_class_keys_array(CLASS_ID, 2, packed.as_ptr(), 14) as u64;
    let requested = SHAPE_ID_BASE + 0x3456;
    let id = js_object_shape_id_for_class_keys_static(keys, 2, 2, CLASS_ID, requested);
    let record = shapes::shape_descriptor_by_id(id).expect("seeded record");
    assert_eq!(record.proto_id, shapes::class_proto_id(CLASS_ID));
    // Registration is idempotent: a second registration (another module
    // importing the class) resolves to the same id.
    assert_eq!(
        js_object_shape_id_for_class_keys_static(keys, 2, 2, CLASS_ID, requested),
        id
    );
    assert!(is_carrier(id));
}
