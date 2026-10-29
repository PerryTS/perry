use super::*;

fn plan(source: &str) -> RealmPlan {
    let module = perry_parser::parse_typescript(source, "realm_atoms.cjs").unwrap();
    RealmPlan::from_reach(&perry_hir::realm_reach::scan(
        &module,
        crate::realm_manifest::REALM,
    ))
    .unwrap()
}

#[test]
fn hello_and_full_realm_atoms_verify_on_each_target() {
    for source in ["console.log('hi');", "with ({}) { console.log('hi'); }"] {
        let plan = plan(source);
        for triple in [
            "x86_64-unknown-linux-gnu",
            "aarch64-apple-darwin",
            "x86_64-pc-windows-msvc",
            "wasm32-wasip2",
        ] {
            let mut module = LlModule::new(triple);
            let atoms = RealmAtoms::emit(&mut module, &plan).unwrap();
            for cell in plan.cells.values() {
                for property in &cell.properties {
                    if property.symbol {
                        atoms.symbol(&property.key).unwrap();
                    } else {
                        atoms.string(&property.key).unwrap();
                    }
                    if let Some(value) = &property.value {
                        if !matches!(value, RealmValue::Dynamic(_)) {
                            atoms.value(value).unwrap();
                        }
                    }
                }
                for value in &cell.captures {
                    atoms.value(value).unwrap();
                }
            }
            let mut expected_symbols = plan.symbols.clone();
            for layout in plan.runtime_layouts.values() {
                for key in &layout.for_target(triple).unwrap().keys {
                    let value = key.atom.realm_value().unwrap();
                    atoms.value(&value).unwrap();
                    if let RealmValue::Symbol(key) = value {
                        expected_symbols.insert(key);
                    }
                }
            }
            let ir = module.to_ir();
            let context = inkwell::context::Context::create();
            crate::inprocess::parse_ir_text(&context, &ir, "realm_atoms")
                .unwrap()
                .verify()
                .unwrap();
            assert_eq!(
                ir.matches("i8 32, i8 5, i16 0, i32 32").count(),
                expected_symbols.len()
            );
        }
    }
}

#[test]
fn image_values_preserve_bits_and_do_not_turn_providers_into_constants() {
    let plan = plan("with ({}) {}");
    let mut module = LlModule::new("x86_64-unknown-linux-gnu");
    let atoms = RealmAtoms::emit(&mut module, &plan).unwrap();
    for (value, expected) in [
        (
            RealmValue::NumberBits("8000000000000000".into()),
            (-0.0f64).to_bits(),
        ),
        (RealmValue::RawWord("0xffffffffffffffff".into()), u64::MAX),
        (
            RealmValue::Int32(-1),
            crate::nanbox::INT32_TAG | u32::MAX as u64,
        ),
        (RealmValue::Undefined, crate::nanbox::TAG_UNDEFINED),
    ] {
        let Slot::Bits(bits) = atoms.value(&value).unwrap() else {
            panic!("primitive became pointer")
        };
        assert_eq!(bits, expected);
    }
    let Slot::Address {
        symbol,
        offset,
        tag,
    } = atoms.value(&RealmValue::Bytes("alloc".into())).unwrap()
    else {
        panic!("byte capture became tagged")
    };
    assert_eq!(symbol, "perry_image_literal_0_616c6c6f63");
    assert_eq!((offset, tag), (28, 0));
    assert!(atoms
        .value(&RealmValue::Dynamic("process.argv".into()))
        .unwrap_err()
        .contains("birth provider"));
    assert!(atoms
        .value(&RealmValue::RawWord("not a word".into()))
        .is_err());
}

#[test]
fn symbol_identities_are_unique_and_disjoint_from_dynamic_births() {
    let identities = symbol_identities().unwrap();
    let ids = identities.values().copied().collect::<BTreeSet<_>>();
    assert_eq!(ids.len(), identities.len());
    // Image ranks are below IMAGE_SYMBOL_ORDINALS; every dynamic birth ranks above.
    assert!(ids
        .iter()
        .all(|&id| perry_abi::symbol_birth_rank(id) < perry_abi::IMAGE_SYMBOL_ORDINALS));
    assert!(
        perry_abi::symbol_birth_rank(perry_abi::symbol_identity(1))
            > perry_abi::IMAGE_SYMBOL_ORDINALS
    );
    for (index, name) in perry_abi::WELL_KNOWN_SYMBOLS.iter().enumerate() {
        let id = identities[&format!("@@{name}")];
        assert_eq!(
            id,
            perry_abi::symbol_identity(perry_abi::IMAGE_SYMBOL_ORDINAL_BASE + index as u64)
        );
        assert_eq!(perry_abi::symbol_birth_rank(id), index as u64);
    }
    assert_eq!(
        symbol_description("@@iterator").unwrap(),
        ("Symbol.iterator".into(), false)
    );
    assert_eq!(
        symbol_description("@for:nodejs.rejection").unwrap(),
        ("nodejs.rejection".into(), true)
    );
    assert_eq!(
        symbol_description("@runtime:kCapture").unwrap(),
        ("kCapture".into(), false)
    );
    assert!(symbol_description("@@unknown").is_err());
    assert!(symbol_description("description only").is_err());
    assert_ne!(symbol_cell_name("@for:x"), symbol_cell_name("@runtime:x"));
}

#[test]
fn function_prototype_state_key_links_to_its_image_string() {
    let plan = plan("console.log('hi');");
    assert!(plan.cells.values().any(|cell| cell
        .header
        .as_ref()
        .is_some_and(|h| h.gc_type == GC_TYPE_CLOSURE
            && super::super::properties::function_has_prototype_state(cell).unwrap())));
    assert!(plan.key_atoms.contains("p"));
    for target in [
        "x86_64-unknown-linux-gnu",
        "aarch64-apple-darwin",
        "x86_64-pc-windows-msvc",
        "wasm32-wasip2",
    ] {
        let mut module = LlModule::new(target);
        let atoms = RealmAtoms::emit(&mut module, &plan).unwrap();
        let Slot::ImageString(symbol) = atoms.string("p").unwrap() else {
            panic!("function state key must be an image string")
        };
        let ir = module.to_ir();
        let key = ir
            .lines()
            .find(|line| line.starts_with("@perry_atom_70 ="))
            .unwrap();
        assert!(
            key.contains(&symbol),
            "key atom must carry the emitted string address"
        );
    }
}

#[test]
fn runtime_layout_facts_preserve_target_variants_and_symbol_registration() {
    let plan = plan("with ({}) {}");
    assert_eq!(
        plan.runtime_layouts.len(),
        perry_abi::NATIVE_LAYOUT_DESCRIPTORS.len()
    );
    let layout = &plan.runtime_layouts["%Layout:RECORD_Q457_0%"];
    let linux = layout.for_target("x86_64-unknown-linux-gnu").unwrap();
    let windows = layout.for_target("x86_64-pc-windows-msvc").unwrap();
    assert_eq!((linux.variant, linux.keys.len()), (0, 3));
    assert_eq!((windows.variant, windows.keys.len()), (1, 4));
    let keys = plan
        .runtime_layouts
        .values()
        .flat_map(|l| &l.for_target("x86_64-unknown-linux-gnu").unwrap().keys)
        .map(|k| k.atom.realm_value().unwrap())
        .collect::<Vec<_>>();
    assert!(keys.contains(&RealmValue::Symbol("@for:nodejs.rejection".into())));
    assert!(keys.contains(&RealmValue::Symbol("@for:Stream.toAsyncStreamable".into())));
    assert!(keys.contains(&RealmValue::Symbol("@runtime:kCapture".into())));
}
