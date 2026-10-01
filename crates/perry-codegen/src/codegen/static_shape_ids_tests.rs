use super::*;

fn class(keys: &str, count: u32, cid: u32) -> BirthShape {
    BirthShape {
        keys: keys.as_bytes().to_vec(),
        key_count: count,
        live: count,
        proto: BirthProto::Class(cid),
        typed: None,
        rep: 0,
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

/// `ids_are_identical_across_builds`: the slots of two fixed contents.
const GOLDEN: (u32, u32) = (308, 723);

fn in_band(id: u32, band: u32) -> bool {
    (SHAPE_ID_BASE..SHAPE_ID_BASE + band).contains(&id)
}

fn many(prefix: &str, n: u32) -> Vec<BirthShape> {
    (0..n)
        .map(|i| class(&format!("{prefix}{i}\0"), 1, 100 + (i % 7)))
        .collect()
}

#[test]
fn the_band_is_the_next_power_of_two_of_four_per_content() {
    assert_eq!(static_band_size(0), MIN_STATIC_BAND);
    assert_eq!(static_band_size(1), 1024);
    assert_eq!(static_band_size(256), 1024);
    assert_eq!(static_band_size(257), 2048);
    assert_eq!(static_band_size(512), 2048);
    assert_eq!(static_band_size(513), 4096);
    assert_eq!(static_band_size(5000), 32768);
    assert_eq!(static_band_size(1 << 18), STATIC_SHAPE_ID_COUNT);
    assert_eq!(static_band_size(usize::MAX), STATIC_SHAPE_ID_COUNT);
}

#[test]
fn every_distinct_content_gets_a_distinct_id_within_the_sized_band() {
    for n in [1u32, 100, 256, 257, 5000] {
        let contents = many("k", n);
        let band = static_band_size(n as usize);
        let ids = assign_static_shape_ids(&contents);
        assert_eq!(ids.len(), contents.len());
        let distinct: BTreeSet<u32> = ids.values().copied().collect();
        assert_eq!(distinct.len(), contents.len(), "two contents shared an id");
        assert!(
            ids.values().all(|&id| in_band(id, band)),
            "an id left the {band}-slot band for {n} contents"
        );
    }
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
fn ids_are_identical_across_builds() {
    // The hash is a fixed FNV-1a, never a per-process seed: a cached object
    // embeds these ids, so a rebuild must reproduce them bit for bit.
    let a = class("x\0y\0", 2, 7);
    let lit = BirthShape {
        proto: BirthProto::Literal,
        ..class("x\0y\0", 2, 0)
    };
    let ids = assign_static_shape_ids([&a, &lit]);
    assert_eq!((ids[&a] - SHAPE_ID_BASE, ids[&lit] - SHAPE_ID_BASE), GOLDEN);
}

/// The occupied run (maximal contiguous occupied slots, wrapping) holding
/// `slot` in a band of `band` slots.
fn run_of(slot: u32, used: &BTreeSet<u32>, band: u32) -> BTreeSet<u32> {
    let mut run = BTreeSet::new();
    let mut s = slot;
    while used.contains(&s) && run.insert(s) {
        s = (s + 1) & (band - 1);
    }
    let mut s = slot.wrapping_sub(1) & (band - 1);
    while used.contains(&s) && run.insert(s) {
        s = s.wrapping_sub(1) & (band - 1);
    }
    run
}

#[test]
fn adding_an_unrelated_content_keeps_the_other_ids() {
    // 300 contents: a 2048-slot band, which one more content does not grow.
    let base = many("s", 300);
    let band = static_band_size(base.len());
    assert_eq!(band, static_band_size(base.len() + 1));
    let before = assign_static_shape_ids(&base);
    let mut untouched = 0;
    for i in 0..64 {
        let extra = class(&format!("unrelated{i}\0"), 1, 900);
        let after = assign_static_shape_ids(base.iter().chain([&extra]));
        let used: BTreeSet<u32> = after.values().map(|id| id - SHAPE_ID_BASE).collect();
        let run = run_of(after[&extra] - SHAPE_ID_BASE, &used, band);
        let moved: Vec<&BirthShape> = base.iter().filter(|c| before[*c] != after[*c]).collect();
        // Only a content in the probe run the new content joined can move.
        for c in &moved {
            assert!(
                run.contains(&(before[*c] - SHAPE_ID_BASE)),
                "{c:?} moved from outside the run the new content joined"
            );
        }
        if moved.is_empty() {
            untouched += 1;
        }
    }
    // At a load of at most 1/4 almost every addition moves nothing.
    assert!(
        untouched >= 56,
        "only {untouched}/64 additions left every id alone"
    );
}

#[test]
fn a_colliding_addition_moves_only_contents_in_its_probe_run() {
    // The counterpart of the test above: a new content whose home slot is an
    // existing id and which sorts before its occupant takes that slot, and
    // the displacement stays inside the run it joined.
    let base = many("s", 300);
    let band = static_band_size(base.len());
    let before = assign_static_shape_ids(&base);
    let taken: BTreeSet<u32> = before.values().map(|id| id - SHAPE_ID_BASE).collect();
    let extra = (0..100_000)
        .map(|i| class(&format!("a{i}\0"), 1, 100))
        .find(|c| taken.contains(&c.home_slot(band)))
        .expect("some content collides in a 2048-slot band");
    let after = assign_static_shape_ids(base.iter().chain([&extra]));
    let used: BTreeSet<u32> = after.values().map(|id| id - SHAPE_ID_BASE).collect();
    let run = run_of(after[&extra] - SHAPE_ID_BASE, &used, band);
    let moved: Vec<&BirthShape> = base.iter().filter(|c| before[*c] != after[*c]).collect();
    assert!(!moved.is_empty(), "the collision displaced nothing");
    for c in moved {
        assert!(run.contains(&(before[c] - SHAPE_ID_BASE)));
        assert!(run.contains(&(after[c] - SHAPE_ID_BASE)));
    }
}

#[test]
fn the_band_doubles_only_at_a_power_of_two_threshold() {
    let contents = many("d", 257);
    let small = assign_static_shape_ids(&contents[..256]);
    let big = assign_static_shape_ids(&contents);
    assert!(small.values().all(|&id| in_band(id, 1024)));
    assert!(big.values().all(|&id| in_band(id, 2048)));
    assert!(big.values().any(|&id| !in_band(id, 1024)));
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

/// Charter step 5, T1 (b): the birth rep is content. A definer born with an
/// `F64` lane and an importer's all-`Any` stub of the same keys are two
/// contents with two ids, and the stub never resolves to the definer's id:
/// its inline allocation fills `undefined` and cannot know the definer's
/// constructor proof, so it keeps its own structural id.
#[test]
fn an_f64_birth_rep_is_content_and_a_stub_never_adopts_it() {
    let rep = 0b01 << 2; // F64 lane at slot 1
    let stub = class("next\0value\0", 2, 22);
    let def = BirthShape {
        rep,
        ..typed("next\0value\0", 2, 22, 0b10, 0b01)
    };
    let def_any = typed("next\0value\0", 2, 22, 0b10, 0b01);
    assert_ne!(def.content_hash(), def_any.content_hash());
    assert_ne!(def.structure(), stub.structure());
    let births = [
        birth("k_def__D", 22, true, &def),
        birth("k_imp__D", 22, false, &stub),
    ];
    let ids = assign_static_shape_ids(births.iter().map(|b| &b.shape));
    assert_ne!(ids[&def], ids[&stub]);
    let program = ProgramClassShapeIds::from_births(&births, &ids);
    assert_eq!(
        program.resolved_id("k_imp__D", 22, &stub, ids[&stub]),
        ids[&stub],
        "the stub keeps its own id"
    );
    // A structural (untyped) definer with an F64 lane: same rule.
    let def_plain = BirthShape {
        rep,
        ..class("next\0value\0", 2, 23)
    };
    let stub23 = class("next\0value\0", 2, 23);
    let births = [
        birth("k_def__E", 23, true, &def_plain),
        birth("k_imp__E", 23, false, &stub23),
    ];
    let ids = assign_static_shape_ids(births.iter().map(|b| &b.shape));
    let program = ProgramClassShapeIds::from_births(&births, &ids);
    assert_eq!(program.resolved_id("k_imp__E", 23, &stub23, 11), 11);
    // A literal with an F64 lane is seeded like an all-`Any` one: the seed
    // carries its rep, so it mints the literal's own facts.
    let lit = BirthShape {
        proto: BirthProto::Literal,
        rep,
        ..class("a\0b\0", 2, 0)
    };
    assert!(lit.is_seedable());
}

/// The seed sidecar carries the birth rep: a warm link replays exactly the
/// facts a cold one seeded. A line without the rep (another format) is
/// malformed, never an all-`Any` seed of the same keys.
#[test]
fn a_seed_line_round_trips_the_birth_rep() {
    for rep in [0u64, 0b0101, 0b01 << 20] {
        let lit = BirthShape {
            proto: BirthProto::Literal,
            rep,
            ..class("lt_u\0lt_v\0", 2, 0)
        };
        let line = encode_static_seed(0x1000_0077, &lit);
        assert_eq!(
            decode_static_seed(&line),
            Some((0x1000_0077, lit)),
            "{line}"
        );
    }
    assert_eq!(decode_static_seed("268435575 2 2 6c745f7500"), None);
    assert_eq!(decode_static_seed("268435575 2 2 6c745f7500 5"), None);
    assert_eq!(decode_static_seed("268435575 2 2 6c745f7500 0x5 x"), None);
}

#[test]
fn class_birth_reads_the_birth_rep_of_its_keys_global() {
    let prefix = "m";
    let class_ids: HashMap<String, u32> = [("Pair".to_string(), 57)].into_iter().collect();
    let images = HashMap::new();
    let pair: ClassKeysInit = (
        "perry_class_keys_m__Pair".into(),
        "a\0b\0".into(),
        2,
        vec![],
        vec![],
    );
    let reps: HashMap<String, u64> = [("perry_class_keys_m__Pair".to_string(), 0b0101u64)]
        .into_iter()
        .collect();
    let b = class_birth(prefix, &pair, &images, &reps, &class_ids);
    assert_eq!(b.shape.unwrap().rep, 0b0101);
    let b = class_birth(prefix, &pair, &images, &HashMap::new(), &class_ids);
    assert_eq!(b.shape.unwrap().rep, 0);
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
    let a = class_birth(prefix, &anon, &images, &HashMap::new(), &class_ids);
    assert_eq!(a.shape.unwrap().proto, BirthProto::Literal);
    let p = class_birth(prefix, &point, &images, &HashMap::new(), &class_ids);
    assert_eq!(p.shape.unwrap().proto, BirthProto::Class(56));
    let o = class_birth(prefix, &orphan, &images, &HashMap::new(), &class_ids);
    assert_eq!(o.class_id, 0);
    assert!(o.shape.is_none());
}
