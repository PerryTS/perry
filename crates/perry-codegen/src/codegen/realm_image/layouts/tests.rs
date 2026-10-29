use super::*;
use crate::codegen::realm_image::atoms::RealmAtoms;
use perry_abi::{LayoutRow, LayoutWord, ShapeObjectKind};

#[test]
fn descriptor_values_and_direct_aliases_verify_on_each_target() {
    let hir = perry_parser::parse_typescript("console.log('hi');", "realm.cjs").unwrap();
    let reach = perry_hir::realm_reach::scan(&hir, crate::realm_manifest::REALM);
    let plan = crate::realm_plan::RealmPlan::from_reach(&reach).unwrap();
    for target in [
        "x86_64-unknown-linux-gnu",
        "aarch64-apple-darwin",
        "x86_64-pc-windows-msvc",
        "wasm32-wasip2",
    ] {
        let mut module = LlModule::new(target);
        let atoms = RealmAtoms::emit(&mut module, &plan).unwrap();
        module.add_raw_global("@prototype_cell = external global i8".into());
        let (symbol_atom, hash) = atoms.symbol_atom(&mut module, "@@iterator").unwrap();
        let layouts = [
            LayoutFacts {
                symbol: "perry_runtime_layout_test".into(),
                word: LayoutWord::new(LayoutRow(125), 0, 2).unwrap().0,
                family: 0,
                variant: 0,
                kind: ShapeObjectKind::Ordinary,
                integrity: 0,
                slot_count: 1,
                rep: 0,
                prototype: Prototype::Null,
                keys: vec![LayoutKey {
                    atom: symbol_atom,
                    hash,
                    image: ImageKeySlot {
                        value: atoms.symbol("@@iterator").unwrap(),
                        string_hash: None,
                        attrs: 2,
                        slot: 0,
                    },
                }],
                class_id: 0,
                parent_class_id: 0,
            },
            LayoutFacts {
                symbol: "perry_realm_layout_test_function".into(),
                word: LayoutWord::new(LayoutRow(126), 2, 1).unwrap().0,
                family: 0,
                variant: 0,
                kind: ShapeObjectKind::Function,
                integrity: 0,
                slot_count: 0,
                rep: 0,
                prototype: Prototype::Image {
                    serial: 1,
                    cell: "prototype_cell".into(),
                },
                keys: Vec::new(),
                class_id: 0,
                parent_class_id: 0,
            },
        ];
        let tags = emit_layouts(&mut module, 125, &layouts).unwrap();
        let ir = module.to_ir();
        assert!(ir.contains("@perry_lazy_layouts = global [2 x"));
        assert!(ir.contains(
            "{ i64, ptr } { i64 1, ptr getelementptr (i8, ptr @prototype_cell, i64 8) }"
        ));
        assert!(ir.contains("@perry_runtime_layout_test = alias"));
        assert!(ir.contains("@perry_realm_layout_test_function = alias"));
        assert_eq!(tags.len(), if target.contains("apple") { 2 } else { 0 });
        let context = inkwell::context::Context::create();
        crate::inprocess::parse_ir_text(&context, &ir, "lazy_layouts")
            .unwrap()
            .verify()
            .unwrap();
        assert!(emit_layouts(&mut module, 124, &layouts)
            .unwrap_err()
            .contains("assigned row"));
    }
}

#[test]
fn source_runtime_layouts_emit_only_r_with_target_slots_and_shared_key_cells() {
    use perry_hir::realm_manifest::RuntimeLayoutPrototype;
    for source in ["console.log('hi');", "1 + 2;", "with ({}) {}"] {
        let hir = perry_parser::parse_typescript(source, "realm.cjs").unwrap();
        let reach = perry_hir::realm_reach::scan(&hir, crate::realm_manifest::REALM);
        let plan = crate::realm_plan::RealmPlan::from_reach(&reach).unwrap();
        for target in [
            "x86_64-unknown-linux-gnu",
            "aarch64-apple-darwin",
            "x86_64-pc-windows-msvc",
            "wasm32-wasip2",
        ] {
            let mut module = LlModule::new(target);
            let atoms = RealmAtoms::emit(&mut module, &plan).unwrap();
            let mut row = 125;
            let layouts = runtime_layouts(&mut module, &plan, &atoms, |_, variant| {
                let proto = match variant.prototype {
                    RuntimeLayoutPrototype::Object => 0,
                    RuntimeLayoutPrototype::Null => 2,
                    RuntimeLayoutPrototype::Serial { .. } => 1,
                };
                let word = LayoutWord::new(LayoutRow(row), 0, proto).unwrap().0;
                row += 1;
                Ok(word)
            })
            .unwrap();
            assert_eq!(layouts.len(), plan.runtime_layouts.len());
            let fixups = emit_layouts(&mut module, 125, &layouts).unwrap();
            assert_eq!(fixups.is_empty(), !target.contains("apple"));
            let ir = module.to_ir();
            for name in perry_abi::NATIVE_LAYOUT_DESCRIPTORS {
                assert_eq!(
                    ir.contains(&format!("@perry_runtime_layout_{name} = alias")),
                    plan.runtime_layouts
                        .contains_key(&format!("%Layout:{name}%")),
                    "{source}: {name}"
                );
            }
            if reach.is_full() {
                let release = layouts
                    .iter()
                    .find(|l| l.symbol == "perry_runtime_layout_RECORD_Q457_0")
                    .unwrap();
                assert_eq!(
                    release.slot_count,
                    if target.contains("windows") { 4 } else { 3 }
                );
            }
            let context = inkwell::context::Context::create();
            crate::inprocess::parse_ir_text(&context, &ir, "runtime_layouts")
                .unwrap()
                .verify()
                .unwrap();
        }
    }
}
