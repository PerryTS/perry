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
fn ids_are_dense_from_the_band_start_in_content_order() {
    // The by-id store allocates a 32-record chunk per 32-id run it touches:
    // ids must fill the band from its start, one per content, no gaps.
    let contents: Vec<BirthShape> = (0..70_000)
        .map(|i| class(&format!("p{i}\0"), 1, 1))
        .collect();
    let ids = assign_static_shape_ids(&contents);
    let distinct: BTreeSet<u32> = ids.values().copied().collect();
    assert_eq!(distinct.len(), 70_000);
    assert_eq!(distinct.first(), Some(&SHAPE_ID_BASE));
    assert_eq!(distinct.last(), Some(&(SHAPE_ID_BASE + 70_000 - 1)));
    let mut sorted: Vec<&BirthShape> = contents.iter().collect();
    sorted.sort();
    for (rank, c) in sorted.into_iter().enumerate() {
        assert_eq!(ids[c], SHAPE_ID_BASE + rank as u32);
    }
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
fn a_structural_birth_never_shares_a_typed_id() {
    let importer = class("next\0value\0", 2, 11);
    let definer = typed("next\0value\0", 2, 11, 0b10, 0b01);
    let ids = assign_static_shape_ids([&importer, &definer]);
    assert_ne!(
        ids[&importer], ids[&definer],
        "one id would name two layouts"
    );
}

fn birth(global: &str, cid: u32, defined: bool, shape: &BirthShape) -> ModuleBirth {
    ModuleBirth {
        keys_global: global.to_string(),
        class_id: cid,
        defined,
        shape: shape.clone(),
    }
}

#[test]
fn a_structural_stub_of_the_definers_facts_resolves_to_the_definers_typed_id() {
    let stub = class("next\0value\0", 2, 21);
    let def = typed("next\0value\0", 2, 21, 0b10, 0b01);
    let births = [
        birth("k_def__C", 21, true, &def),
        birth("k_imp__C", 21, false, &stub),
    ];
    let ids = assign_static_shape_ids(births.iter().map(|b| &b.shape));
    let program = ProgramClassShapeIds::from_births(&births, &ids);
    assert_eq!(
        program.resolved_id("k_imp__C", 21, &stub, ids[&stub]),
        ids[&def]
    );
    assert_eq!(
        program.resolved_id("k_def__C", 21, &def, ids[&def]),
        ids[&def]
    );
    // A stub with other facts (#5094), or a typed stub, keeps its own id.
    let other = class("next\0", 1, 21);
    assert_eq!(program.resolved_id("k_imp__C", 21, &other, 7), 7);
    let typed_stub = typed("next\0value\0", 2, 21, 0, 0b11);
    assert_eq!(program.resolved_id("k_imp__C", 21, &typed_stub, 9), 9);
}

#[test]
fn a_class_id_defined_twice_has_no_program_entry() {
    let a = class("a\0", 1, 31);
    let b = class("b\0", 1, 31);
    let births = [birth("k_1__A", 31, true, &a), birth("k_2__A", 31, true, &b)];
    let ids = assign_static_shape_ids(births.iter().map(|b| &b.shape));
    let program = ProgramClassShapeIds::from_births(&births, &ids);
    assert!(program.0.is_empty());
    assert!(program.restricted_to([31]).0.is_empty());
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
