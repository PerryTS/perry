//! `rest_fields::class_lanes`: a static family layout's slot decides
//! whether a field position can hold an I32 lane (MISC1, three.js
//! `Raycaster.near`).

use super::rest_fields::family_keeps_lane;
use crate::layout_families::{FamilyKey, FamilyLayout};

fn family(keys: &[(&str, u32)]) -> FamilyLayout {
    FamilyLayout {
        class_id: 7,
        parent_class_id: 0,
        keys: keys
            .iter()
            .map(|&(k, s)| (FamilyKey::Name(k.to_string()), s))
            .collect(),
        slot_count: keys.len() as u32,
        own_count: keys.len() as u32,
        init_count: 0,
        discriminant: 0,
        variants: Vec::new(),
    }
}

#[test]
fn a_class_without_a_family_keeps_its_lane() {
    assert!(family_keeps_lane(None, "near", 1));
}

#[test]
fn a_family_slot_equal_to_the_field_position_keeps_the_lane() {
    let fam = family(&[("ray", 0), ("near", 1), ("far", 2)]);
    assert!(family_keeps_lane(Some(&fam), "near", 1));
}

#[test]
fn a_coloured_family_slot_moves_the_key_and_drops_the_lane() {
    // Store order ray, near, far, camera; colouring put camera at slot 0 and
    // ray at slot 1, so near (field position 1) lives at slot 3. A lane at
    // slot 1 would write near's I32 word over ray.
    let fam = family(&[("ray", 1), ("near", 3), ("far", 2), ("camera", 0)]);
    assert!(!family_keeps_lane(Some(&fam), "near", 1));
    assert!(family_keeps_lane(Some(&fam), "far", 2));
}

#[test]
fn a_key_the_family_does_not_hold_has_no_lane() {
    let fam = family(&[("ray", 0)]);
    assert!(!family_keeps_lane(Some(&fam), "near", 1));
}
