use super::*;

fn class(keys: &str, count: u32, cid: u32) -> BirthShape {
    BirthShape {
        keys: keys.as_bytes().to_vec(),
        key_count: count,
        live: count,
        proto: BirthProto::Class(cid),
        typed: None,
        rep: 0,
        constfn: Vec::new(),
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

#[test]
fn constfn_body_symbols_are_seedable_final_static_content() {
    let body = |symbol: &str| BirthShape {
        proto: BirthProto::Literal,
        rep: 0b11,
        constfn: vec![ConstFnBirth {
            slot: 0,
            symbol: symbol.to_string(),
        }],
        ..class("method\0", 1, 0)
    };
    let first = body("perry_closure_m__first$info");
    let second = body("perry_closure_m__second$info");
    assert_ne!(first.content_hash(), second.content_hash());
    assert_ne!(first.structure(), second.structure());
    assert!(
        first.is_seedable(),
        "final literal shapes have a body-aware seed"
    );
    let line = encode_static_seed(0x1000_0099, &first);
    assert_eq!(decode_static_seed(&line), Some((0x1000_0099, first)));
    assert_eq!(decode_static_seed("268435609 1 1 6d6574686f6400 0x3"), None);
    assert_eq!(
        decode_static_seed("268435609 1 1 6d6574686f6400 0x0 0@61"),
        None
    );
    assert_eq!(
        decode_static_seed("268435609 1 1 6d6574686f6400 0x3 0@61,0@62"),
        None
    );
}

#[test]
fn constfn_birth_cannot_publish_a_static_guard_or_seed() {
    let shape = BirthShape {
        proto: BirthProto::Literal,
        rep: 0b11,
        constfn: vec![ConstFnBirth {
            slot: 0,
            symbol: "perry_closure_m__method$info".to_string(),
        }],
        ..class("method\0", 1, 0)
    };
    let key = "perry_class_keys_m__method";
    MODULE_STATIC_IDS.with(|m| {
        m.borrow_mut().insert(key.to_string(), (0x1000_0099, shape));
    });
    MODULE_SEEDS.with(|s| s.borrow_mut().clear());
    assert_eq!(static_shape_id_for_keys_global(key), None);
    assert_eq!(requested_shape_id_for_keys_global(key), None);
    assert_eq!(static_region_slots(key, &["method".into()], 0), None);
    assert!(take_module_static_seeds().is_empty());
    MODULE_STATIC_IDS.with(|m| m.borrow_mut().clear());
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

#[test]
fn compatible_final_guards_preserve_allocation_identity_and_refuse_special_writes() {
    let ordinary = BirthShape {
        rep: 1 << 2,
        ..class("m\0x\0", 2, 71)
    };
    let completed = BirthShape {
        rep: 3 | (1 << 2),
        constfn: vec![ConstFnBirth {
            slot: 0,
            symbol: "guard_body$info".into(),
        }],
        ..ordinary.clone()
    };
    let wrong_number = BirthShape {
        rep: 3,
        ..completed.clone()
    };
    let key = "perry_class_keys_guard__C".to_string();
    MODULE_STATIC_IDS.with(|m| {
        *m.borrow_mut() = [(key.clone(), (SHAPE_ID_BASE + 4, ordinary.clone()))]
            .into_iter()
            .collect();
    });
    MODULE_FINAL_IDS.with(|m| {
        *m.borrow_mut() = [
            (completed, SHAPE_ID_BASE + 5),
            (wrong_number, SHAPE_ID_BASE + 6),
        ]
        .into_iter()
        .collect();
    });
    let (region_id, slots, r_mask) = static_region_slots(&key, &["x".into(), "m".into()], 0)
        .expect("ordinary birth supplies exact numeric slots");
    assert_eq!(region_id, SHAPE_ID_BASE + 4);
    assert_eq!(slots, vec![1, 0]);
    assert_eq!(r_mask, 1, "the method lane never supplies a Number fact");
    assert!(static_region_slots(&key, &["x".into()], 1).is_none());
    assert_eq!(
        requested_shape_id_for_keys_global(&key),
        Some(SHAPE_ID_BASE + 4),
        "allocation supplier cannot request final id"
    );
    assert_eq!(
        compatible_final_shape_ids(&(SHAPE_ID_BASE + 4).to_string(), &[]),
        vec![SHAPE_ID_BASE + 5]
    );
    assert_eq!(
        compatible_final_shape_ids(&(SHAPE_ID_BASE + 4).to_string(), &[1]),
        vec![SHAPE_ID_BASE + 5],
        "numeric stores preserve completed facts"
    );
    assert!(
        compatible_final_shape_ids(&(SHAPE_ID_BASE + 4).to_string(), &[0]).is_empty(),
        "CF stores require checked deprecation before writing"
    );
    assert!(slot_may_be_constfn(&key, 0));
    assert!(!slot_may_be_constfn(&key, 1));
    MODULE_STATIC_IDS.with(|m| m.borrow_mut().clear());
    MODULE_FINAL_IDS.with(|m| m.borrow_mut().clear());
    MODULE_SEEDS.with(|m| m.borrow_mut().clear());
}

#[test]
fn region_static_r_is_the_exact_birth_shapes_f64_key_mask() {
    let shape = BirthShape {
        rep: 0b01 | (0b01 << 4),
        ..class("ra\0rb\0rc\0", 3, 0x517)
    };
    let global = "p7_region_keys".to_string();
    MODULE_STATIC_IDS.with(|m| {
        m.borrow_mut()
            .insert(global.clone(), (SHAPE_ID_BASE + 917, shape));
    });
    let keys = vec!["rc".to_string(), "rb".to_string(), "ra".to_string()];
    let (_, slots, r) = static_region_slots(&global, &keys, 0).expect("birth keys are inline");
    assert_eq!(slots, vec![2, 1, 0]);
    assert_eq!(r, 0b101, "R follows key order, not birth slot order");
    assert!(
        static_region_slots(&global, &keys, 0b001).is_none(),
        "a boxed store to an F64 birth lane is refused"
    );
    MODULE_STATIC_IDS.with(|m| {
        m.borrow_mut().remove(&global);
    });
    take_module_static_seeds();
}

#[test]
fn literal_key_cache_mints_require_a_seed_even_without_a_guard() {
    let literal = BirthShape {
        proto: BirthProto::Literal,
        ..class("x\0m\0", 2, 7)
    };
    let declared = class("x\0m\0", 2, 8);
    let assigned = assign_static_shape_ids([&literal, &declared]);
    let entries = vec![
        (
            "perry_class_keys_probe____AnonShape_a".into(),
            "x\0m\0".into(),
            2,
            vec![],
            vec![],
        ),
        (
            "perry_class_keys_probe__Declared".into(),
            "x\0m\0".into(),
            2,
            vec![],
            vec![],
        ),
    ];
    let classes = HashMap::from([("__AnonShape_a".into(), 7), ("Declared".into(), 8)]);
    set_module_static_ids(
        "probe",
        &entries,
        &HashMap::new(),
        &HashMap::new(),
        &classes,
        &assigned.clone().into_iter().collect::<Vec<_>>(),
        &ProgramClassShapeIds::default(),
    );
    assert_eq!(
        requested_shape_id_for_keys_global(&entries[0].0),
        Some(assigned[&literal])
    );
    assert_eq!(
        requested_shape_id_for_keys_global(&entries[1].0),
        Some(assigned[&declared])
    );
    assert_eq!(
        take_module_static_seeds(),
        vec![(assigned[&literal], literal)]
    );
}

/// A class with plain `this.f = f` fields, as `mint_anon_shape_class`
/// synthesizes for an object literal (or as a declared class would look).
fn field_class(
    id: u32,
    name: &str,
    fields: &[&str],
    ctor_id: u32,
    param_base: u32,
) -> perry_hir::Class {
    use perry_hir::types::Type;
    use perry_hir::{ClassField, Expr, Function, Param, Stmt};
    let params: Vec<Param> = fields
        .iter()
        .enumerate()
        .map(|(i, f)| Param {
            id: param_base + i as u32,
            name: (*f).to_string(),
            ty: Type::Any,
            default: None,
            decorators: Vec::new(),
            is_rest: false,
            arguments_object: None,
        })
        .collect();
    let body = params
        .iter()
        .map(|p| {
            Stmt::Expr(Expr::PropertySet {
                object: Box::new(Expr::This),
                property: p.name.clone(),
                value: Box::new(Expr::LocalGet(p.id)),
            })
        })
        .collect();
    perry_hir::Class {
        id,
        name: name.to_string(),
        type_params: Vec::new(),
        extends: None,
        extends_name: None,
        native_extends: None,
        extends_expr: None,
        heritage_lexically_shadowed: false,
        fields: fields
            .iter()
            .map(|f| ClassField {
                name: (*f).to_string(),
                key_expr: None,
                ty: Type::Any,
                init: None,
                is_private: false,
                is_readonly: false,
                decorators: Vec::new(),
            })
            .collect(),
        constructor: Some(Function {
            id: ctor_id,
            name: "constructor".to_string(),
            type_params: Vec::new(),
            params,
            return_type: Type::Void,
            body,
            is_async: false,
            is_generator: false,
            is_strict: true,
            is_exported: false,
            captures: Vec::new(),
            decorators: Vec::new(),
            was_plain_async: false,
            was_unrolled: false,
        }),
        methods: Vec::new(),
        getters: Vec::new(),
        setters: Vec::new(),
        static_accessor_names: Vec::new(),
        static_accessor_fn_ids: Vec::new(),
        static_fields: Vec::new(),
        static_methods: Vec::new(),
        computed_members: Vec::new(),
        decorators: Vec::new(),
        is_exported: false,
        aliases: Vec::new(),
        is_nested: false,
        alloc_width_hint: 0,
        specialized_from: None,
    }
}

/// `const o = { a: 1 }` (an `__AnonShape_*` birth) beside `new Point(2)`.
fn literal_and_declared_module() -> perry_hir::Module {
    use perry_hir::types::Type;
    use perry_hir::{Expr, Stmt};
    let mut hir = perry_hir::Module::new("literal_birth_mint_test");
    hir.classes.push(field_class(
        1,
        "__AnonShape_000000000000a001",
        &["a"],
        90,
        60,
    ));
    hir.classes.push(field_class(2, "Point", &["x"], 91, 70));
    for (id, class, v) in [
        (50, "__AnonShape_000000000000a001", 1.0),
        (51, "Point", 2.0),
    ] {
        hir.init.push(Stmt::Let {
            id,
            name: format!("v{id}"),
            ty: Type::Any,
            mutable: false,
            init: Some(Expr::New {
                class_name: class.to_string(),
                args: vec![Expr::Number(v)],
                type_args: Vec::new(),
                byte_offset: 0,
                cap_args_appended: 0,
            }),
        });
    }
    hir
}

/// The arguments of every `call ... @callee(...)` in `ir`, as their value text
/// (call instructions only: the module's `declare` line names the callee too).
fn mint_calls(ir: &str, callee: &str) -> Vec<Vec<String>> {
    let needle = format!("@{callee}(");
    ir.lines()
        .filter(|line| line.contains(" call ") && !line.trim_start().starts_with("declare"))
        .filter_map(|line| {
            let start = line.find(&needle)? + needle.len();
            let end = start + line[start..].find(')')?;
            Some(
                line[start..end]
                    .split(", ")
                    .map(|arg| arg.rsplit(' ').next().unwrap_or("").to_string())
                    .collect(),
            )
        })
        .collect()
}

#[test]
fn a_literal_birth_mints_its_shape_with_the_plain_prototype_on_both_routes() {
    // Per-module class ids collide: an `__AnonShape_*` id can also be another
    // module's DECLARED class. Passing it to the shape mint let the runtime
    // derive that class's prototype for a plain literal, so two modules' equal
    // literal contents -- ONE static id -- reached the mint with different
    // facts and the second aborted ("the static ShapeId ... was refused by the
    // shape mint"; OpenCode's TUI: ajv's and json5's `{ x }` literals). A
    // literal birth names the plain prototype, as the startup literal seed
    // (`js_shape_seed_plain`) does: class id 0.
    let hir = literal_and_declared_module();

    // Lazy route (no static id): `(keys, field_count, class_id, rep)`.
    let lazy = String::from_utf8(
        crate::compile_module(
            &hir,
            crate::CompileOptions {
                emit_ir_only: true,
                ..Default::default()
            },
        )
        .expect("module compiles"),
    )
    .expect("UTF-8 IR");
    let calls = mint_calls(&lazy, "js_object_shape_id_for_class_keys");
    assert_eq!(calls.len(), 2, "one lazy mint per birth class:\n{lazy}");
    let class_ids: Vec<&str> = calls.iter().map(|args| args[2].as_str()).collect();
    assert!(
        class_ids.contains(&"0"),
        "the literal's mint must name class id 0: {calls:?}"
    );
    assert!(
        class_ids.iter().any(|cid| *cid != "0"),
        "a declared class keeps its own class id (its prototype is a shape fact): {calls:?}"
    );

    // Static route: the driver assigned the literal's content an id.
    let content = BirthShape {
        keys: b"a\0".to_vec(),
        key_count: 1,
        live: 1,
        proto: BirthProto::Literal,
        typed: None,
        rep: 0,
        constfn: Vec::new(),
    };
    let requested = SHAPE_ID_BASE + 7;
    let stat = String::from_utf8(
        crate::compile_module(
            &hir,
            crate::CompileOptions {
                emit_ir_only: true,
                static_shape_ids: vec![(content, requested)],
                ..Default::default()
            },
        )
        .expect("module compiles"),
    )
    .expect("UTF-8 IR");
    // `(keys, field_count, live, class_id, requested, rep)`
    let calls = mint_calls(&stat, "js_object_shape_id_for_class_keys_static");
    let literal: Vec<&Vec<String>> = calls
        .iter()
        .filter(|args| args[4] == requested.to_string())
        .collect();
    assert_eq!(
        literal.len(),
        1,
        "the driver's id must reach the literal's static mint for this test to mean anything: {calls:?}\n{stat}"
    );
    assert_eq!(
        literal[0][3], "0",
        "the literal's static mint must name class id 0, as its seed does: {calls:?}"
    );
}
