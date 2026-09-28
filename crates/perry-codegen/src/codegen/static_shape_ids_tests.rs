use super::*;

fn class(keys: &str, count: u32, cid: u32) -> BirthShape {
    BirthShape {
        keys: keys.as_bytes().to_vec(),
        key_count: count,
        live: count,
        proto: BirthProto::Class(cid),
        typed: None,
    }
}

fn typed(keys: &str, count: u32, cid: u32, raw: u64, ptr: u64) -> BirthShape {
    BirthShape {
        typed: Some(TypedMasks {
            raw_f64_words: vec![raw],
            pointer_words: vec![ptr],
        }),
        ..class(keys, count, cid)
    }
}

fn in_band(id: u32) -> bool {
    (SHAPE_ID_BASE..SHAPE_ID_BASE + STATIC_SHAPE_ID_COUNT).contains(&id)
}

#[test]
fn every_distinct_content_gets_a_distinct_id_in_the_band() {
    let contents: Vec<BirthShape> = (0..5000)
        .map(|i| class(&format!("k{i}\0"), 1, 100 + (i % 7)))
        .collect();
    let ids = assign_static_shape_ids(&contents);
    assert_eq!(ids.len(), contents.len());
    let distinct: BTreeSet<u32> = ids.values().copied().collect();
    assert_eq!(distinct.len(), contents.len(), "two contents shared an id");
    assert!(ids.values().all(|&id| in_band(id)));
}

#[test]
fn ids_depend_on_the_content_set_not_its_order_or_duplicates() {
    let a = class("x\0y\0", 2, 7);
    let b = class("x\0y\0", 2, 8);
    let c = typed("x\0y\0", 2, 9, 1, 2);
    let first = assign_static_shape_ids([&a, &b, &c]);
    let second = assign_static_shape_ids([&c, &a, &b, &a, &c]);
    assert_eq!(first, second);
}

#[test]
fn a_hash_collision_probes_to_the_next_free_id() {
    // Force many contents into a tiny region of the band: identical hashes are
    // impossible to construct cheaply, so check the probe invariant instead —
    // no id repeats even when the band is densely filled near one slot.
    let contents: Vec<BirthShape> = (0..70_000)
        .map(|i| class(&format!("p{i}\0"), 1, 1))
        .collect();
    let ids = assign_static_shape_ids(&contents);
    let distinct: BTreeSet<u32> = ids.values().copied().collect();
    assert_eq!(distinct.len(), 70_000);
}

#[test]
fn masks_and_prototype_are_part_of_the_content() {
    let plain = class("a\0", 1, 3);
    let t1 = typed("a\0", 1, 4, 1, 2);
    let t2 = typed("a\0", 1, 4, 2, 1);
    let lit = BirthShape {
        proto: BirthProto::Literal,
        ..class("a\0", 1, 3)
    };
    let ids = assign_static_shape_ids([&plain, &t1, &t2, &lit]);
    let distinct: BTreeSet<u32> = ids.values().copied().collect();
    assert_eq!(distinct.len(), 4);
}

#[test]
fn a_structural_birth_takes_its_one_typed_layouts_id() {
    let importer = class("next\0value\0", 2, 11);
    let definer = typed("next\0value\0", 2, 11, 0b10, 0b01);
    let ids = assign_static_shape_ids([&importer, &definer]);
    assert_eq!(ids[&importer], ids[&definer]);
}

#[test]
fn a_structural_birth_with_two_typed_matches_keeps_its_own_id() {
    let importer = class("next\0value\0", 2, 12);
    let t1 = typed("next\0value\0", 2, 12, 0b10, 0b01);
    let t2 = typed("next\0value\0", 2, 12, 0, 0b11);
    let ids = assign_static_shape_ids([&importer, &t1, &t2]);
    assert_ne!(ids[&importer], ids[&t1]);
    assert_ne!(ids[&importer], ids[&t2]);
    assert_ne!(ids[&t1], ids[&t2]);
}

#[test]
fn a_wide_birth_never_takes_a_typed_id() {
    let wide = BirthShape {
        live: 4,
        ..class("next\0value\0", 2, 13)
    };
    let definer = typed("next\0value\0", 2, 13, 0b10, 0b01);
    let ids = assign_static_shape_ids([&wide, &definer]);
    assert_ne!(ids[&wide], ids[&definer]);
}

#[test]
fn class_birth_names_anon_shapes_as_literals_and_skips_class_zero() {
    let prefix = "m";
    let mut class_ids = HashMap::new();
    class_ids.insert("__AnonShape_3".to_string(), 55u32);
    class_ids.insert("Point".to_string(), 56u32);
    let images = HashMap::new();
    let anon: ClassKeysInit = (
        "perry_class_keys_m____AnonShape_3".into(),
        "a\0b\0".into(),
        2,
        vec![],
        vec![],
    );
    let point: ClassKeysInit = (
        "perry_class_keys_m__Point".into(),
        "x\0y\0".into(),
        2,
        vec![],
        vec![],
    );
    let orphan: ClassKeysInit = (
        "perry_class_keys_m__Gone".into(),
        "z\0".into(),
        1,
        vec![],
        vec![],
    );
    let a = class_birth(prefix, &anon, &images, &class_ids);
    assert_eq!(a.shape.unwrap().proto, BirthProto::Literal);
    let p = class_birth(prefix, &point, &images, &class_ids);
    assert_eq!(p.shape.unwrap().proto, BirthProto::Class(56));
    let o = class_birth(prefix, &orphan, &images, &class_ids);
    assert_eq!(o.class_id, 0);
    assert!(o.shape.is_none());
}
