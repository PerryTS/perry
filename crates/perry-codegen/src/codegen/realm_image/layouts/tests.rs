use super::*;
use crate::codegen::realm_image::atoms::RealmAtoms;
use perry_abi::{LayoutRow, LayoutWord, ShapeObjectKind};

#[test]
fn descriptor_values_and_direct_aliases_verify_on_each_target() {
    let hir = perry_parser::parse_typescript("[Object,Function,Array,Error,TypeError,RangeError,SyntaxError,ReferenceError,EvalError,URIError,AggregateError,SuppressedError]; console.log('hi');", "realm.cjs").unwrap();
    let reach = perry_hir::realm_reach::scan(
        &hir,
        &perry_hir::realm_reach_index::RealmIndex::new(crate::realm_manifest::REALM),
    );
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

/// Top-level fields of an LLVM aggregate initializer `{ a, b, ... }` or `[ ... ]`.
fn fields(init: &str) -> Vec<String> {
    let inner = init.trim();
    let inner = &inner[1..inner.len() - 1];
    let (mut out, mut depth, mut field) = (Vec::new(), 0i32, String::new());
    for c in inner.chars() {
        match c {
            '{' | '[' | '(' => depth += 1,
            '}' | ']' | ')' => depth -= 1,
            ',' if depth == 0 => {
                out.push(field.trim().to_string());
                field.clear();
                continue;
            }
            _ => {}
        }
        field.push(c);
    }
    if !field.trim().is_empty() {
        out.push(field.trim().to_string());
    }
    out
}

/// The balanced `{ .. }` / `[ .. ]` group starting at the first opener of `text`,
/// and the rest after it.
fn group(text: &str) -> (&str, &str) {
    let open = text.find(['{', '[']).unwrap();
    let mut depth = 0;
    for (i, c) in text[open..].char_indices() {
        match c {
            '{' | '[' => depth += 1,
            '}' | ']' => {
                depth -= 1;
                if depth == 0 {
                    return (&text[open..open + i + 1], &text[open + i + 1..]);
                }
            }
            _ => {}
        }
    }
    panic!("unbalanced {text}")
}

/// The initializer of global `@name` (after its type).
fn initializer<'a>(ir: &'a str, name: &str, linkage: &str) -> &'a str {
    let head = format!("@{name} = {linkage} ");
    let line = ir
        .lines()
        .find(|l| l.starts_with(&head))
        .unwrap_or_else(|| panic!("no {head}"));
    let (_ty, rest) = group(&line[head.len()..]);
    group(rest).0
}

/// DESIGN §3.2 items 9 and 10: the production emission (`emit_program_runtime_layouts`)
/// defines exactly R's runtime layouts, each a standalone `global` under its direct
/// symbol, and every descriptor cell and key-list entry equals the declared facts.
#[test]
fn program_runtime_layouts_are_exactly_r_and_equal_the_declared_facts() {
    use perry_hir::realm_manifest::RuntimeLayoutPrototype;
    let mut symbol_keyed = 0;
    let mut keyless = 0;
    let mut associated = 0;
    for source in ["[Object,Function,Array,Error,TypeError,RangeError,SyntaxError,ReferenceError,EvalError,URIError,AggregateError,SuppressedError]; console.log('hi');", "1 + 2;", "with ({}) {}"] {
        let hir = perry_parser::parse_typescript(source, "realm.cjs").unwrap();
        let reach = perry_hir::realm_reach::scan(
            &hir,
            &perry_hir::realm_reach_index::RealmIndex::new(crate::realm_manifest::REALM),
        );
        let plan = crate::realm_plan::RealmPlan::from_reach(&reach).unwrap();
        assert_eq!(
            reach.is_full(),
            plan.runtime_layouts.len()
                == perry_abi::NATIVE_LAYOUT_DESCRIPTORS.len()
                    - perry_abi::NATIVE_LAYOUT_RESERVED.len(),
            "{source}: a full reach carries every runtime layout, a partial one fewer"
        );
        for target in [
            "x86_64-unknown-linux-gnu",
            "aarch64-apple-darwin",
            "x86_64-pc-windows-msvc",
            "wasm32-wasip2",
        ] {
            let mut module = LlModule::new(target);
            let fixups = emit_program_runtime_layouts(&mut module, &reach).unwrap();
            let ir = module.to_ir();
            assert!(!ir.contains("@perry_lazy_layouts"), "{source} {target}");
            for name in perry_abi::NATIVE_LAYOUT_DESCRIPTORS {
                assert_eq!(
                    ir.contains(&format!("@perry_runtime_layout_{name} = global")),
                    plan.runtime_layouts
                        .contains_key(&format!("%Layout:{name}%")),
                    "{source} {target}: {name}"
                );
                assert!(!ir.contains(&format!("@perry_runtime_layout_{name} = alias")));
            }
            let mut lists = 0;
            for (name, layout) in &plan.runtime_layouts {
                let facts = layout.for_target(target).unwrap();
                let short = &name["%Layout:".len()..name.len() - 1];
                let symbol = &layout.symbol;
                assert_eq!(symbol, &format!("perry_runtime_layout_{short}"));
                let cells = fields(initializer(&ir, symbol, "global"));
                assert_eq!(cells.len(), 20, "{symbol}: {cells:?}");
                let slots = facts.keys.len() as u32;
                let proto_code = match facts.prototype {
                    RuntimeLayoutPrototype::Object => 0,
                    RuntimeLayoutPrototype::Serial { .. } => 1,
                    RuntimeLayoutPrototype::Null => 2,
                };
                let word = perry_abi::native_layout_word(short, proto_code);
                assert!(perry_abi::LayoutWord(word).row().0 < perry_abi::NATIVE_LAYOUT_ROWS);
                let capacity = (facts.keys.len() * 2).next_power_of_two();
                let symbolic = facts
                    .keys
                    .iter()
                    .any(|k| k.atom.kind != perry_abi::ATOM_KIND_STRING);
                // The emitter's rule (`emit_image_key_list`): a layout with no
                // keys has no list to carry, so its cell is null.
                let listed = !facts.keys.is_empty();
                let prototype = match facts.prototype {
                    RuntimeLayoutPrototype::Object => "ptr null".to_string(),
                    RuntimeLayoutPrototype::Null => "ptr inttoptr (i64 1 to ptr)".to_string(),
                    RuntimeLayoutPrototype::Serial { serial } => {
                        assert_eq!(
                            fields(initializer(
                                &ir,
                                &format!("{symbol}_prototype"),
                                "private constant"
                            )),
                            [
                                format!("i64 {}", crate::nanbox::i64_literal(serial)),
                                "ptr null".into()
                            ]
                        );
                        format!("ptr @{symbol}_prototype")
                    }
                };
                let expected = [
                    format!("i32 {}", facts.family),
                    format!("i16 {}", facts.variant),
                    format!("i8 {}", facts.object_kind),
                    format!("i8 {}", facts.integrity),
                    format!("i32 {slots}"),
                    format!("i32 {}", word as i32),
                    format!("i64 {}", crate::nanbox::i64_literal(facts.rep)),
                    prototype,
                    format!("ptr @{symbol}_keys"),
                    format!("i32 {slots}"),
                    format!("i32 {}", capacity - 1),
                    format!("ptr @{symbol}_index"),
                    format!("i32 {}", facts.class_id),
                    format!("i32 {}", facts.parent_class_id),
                    "ptr null".into(),
                    "i32 0".into(),
                    format!("i32 {}", perry_abi::layout_gc_plan(slots, facts.rep) as i32),
                    if !listed {
                        "ptr null".into()
                    } else {
                        format!(
                            "ptr getelementptr (i8, ptr @{symbol}_list, i64 {})",
                            crate::target_layout::GC_HEADER_SIZE_BYTES
                        )
                    },
                ];
                assert_eq!(&cells[..18], &expected[..], "{source} {target}: {symbol}");
                // Every keyed layout carries its link-time list, symbol-keyed
                // ones too: their symbol slots hold `undefined` until the first
                // install stores the runtime's symbol (`image_keys::prepare`).
                assert_eq!(
                    ir.contains(&format!("@{symbol}_list = ")),
                    listed,
                    "{symbol}"
                );
                // On ELF the list (and its attribute cell) lives and dies with
                // the descriptor in the link; the realm section is retained.
                let elf = crate::fn_record::RecordFormat::of(target)
                    == crate::fn_record::RecordFormat::Elf;
                let defined_at = ir
                    .lines()
                    .position(|l| l.starts_with(&format!("@{symbol} = global ")))
                    .unwrap();
                for cell in [format!("{symbol}_list"), format!("{symbol}_list_attrs")] {
                    if let Some(at) = ir
                        .lines()
                        .position(|l| l.starts_with(&format!("@{cell} = ")))
                    {
                        let line = ir.lines().nth(at).unwrap();
                        // The assembler opens the list's linked-to section
                        // only after the descriptor's own.
                        assert!(defined_at < at, "{target}: {cell} precedes @{symbol}");
                        assert_eq!(
                            line.ends_with(&format!(", !associated !{{ptr @{symbol}}}")),
                            elf,
                            "{target}: {cell}"
                        );
                        associated += usize::from(elf);
                    }
                }
                lists += usize::from(listed);
                symbol_keyed += usize::from(symbolic);
                keyless += usize::from(facts.keys.is_empty());
                let keys = fields(initializer(
                    &ir,
                    &format!("{symbol}_keys"),
                    "private constant",
                ));
                assert_eq!(keys.len(), facts.keys.len(), "{symbol}");
                for (entry, key) in keys.iter().zip(&facts.keys) {
                    let entry = fields(group(&entry[crate::layout_desc::KEY_TY.len()..]).0);
                    let atom = match key.atom.kind {
                        perry_abi::ATOM_KIND_STRING => {
                            let text = key.atom.text.as_deref().unwrap();
                            let spelling =
                                text.bytes().map(|b| format!("{b:02x}")).collect::<String>();
                            // The runtime's atom and image string (its
                            // `runtime_atom!`), never a second program cell.
                            for name in [
                                format!("perry_atom_{spelling}"),
                                format!("perry_image_literal_0_{spelling}"),
                            ] {
                                assert!(
                                    ir.contains(&format!("@{name} = external global")),
                                    "{symbol}: {name} is not the runtime's"
                                );
                                assert_eq!(
                                    ir.matches(&format!("@{name} = ")).count(),
                                    1,
                                    "{symbol}: {name}"
                                );
                            }
                            format!("perry_atom_{spelling}")
                        }
                        perry_abi::ATOM_KIND_WELL_KNOWN_SYMBOL => format!(
                            "perry_atom_well_known_{}",
                            perry_abi::WELL_KNOWN_SYMBOLS[key.atom.index as usize]
                        ),
                        perry_abi::ATOM_KIND_PROGRAM_SYMBOL => {
                            let atom = format!("perry_runtime_symbol_atom_{}", key.atom.index);
                            assert!(ir.contains(&format!("@{atom} = external global")), "{atom}");
                            atom
                        }
                        kind => panic!("{symbol}: atom kind {kind}"),
                    };
                    assert!(
                        ir.contains(&format!("@{atom} = ")),
                        "{symbol}: {atom} undefined"
                    );
                    assert_eq!(
                        &entry[..4],
                        &[
                            format!("ptr @{atom}"),
                            format!("i32 {}", key.slot),
                            format!("i8 {}", key.attributes as i8),
                            "i8 0".to_string(),
                        ],
                        "{symbol}"
                    );
                }
            }
            // Apple rewrites the image lists' tagged words at startup; nothing else does.
            assert_eq!(
                fixups.is_empty(),
                !target.contains("apple") || lists == 0,
                "{target}"
            );
            if reach.is_full() {
                let release = plan.runtime_layouts["%Layout:RECORD_Q457_0%"]
                    .for_target(target)
                    .unwrap();
                assert_eq!(
                    release.keys.len(),
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
    assert!(
        symbol_keyed > 0,
        "the symbol-keyed list path was never checked"
    );
    assert!(
        keyless > 0,
        "the keyless (null list) path was never checked"
    );
    assert!(
        associated > 0,
        "no ELF list was checked for its descriptor association"
    );
}

/// On ELF every emitted key-list cell of a runtime descriptor (and its
/// attribute cell) is assembled into a link-order section whose linked-to
/// symbol is its OWN descriptor, and that descriptor is defined earlier in the
/// assembly: the link then drops the list exactly when it drops the
/// descriptor, although `perry_realm` itself is retained by its bounds. A list
/// emitted without the tie fails here, not only when a descriptor dies.
#[cfg(feature = "llvm-inprocess")]
#[test]
fn every_runtime_key_list_section_is_linked_to_its_own_descriptor_on_elf() {
    let target = "x86_64-unknown-linux-gnu";
    let hir = perry_parser::parse_typescript("1 + 2;", "realm.cjs").unwrap();
    let reach = perry_hir::realm_reach::scan(
        &hir,
        &perry_hir::realm_reach_index::RealmIndex::new(crate::realm_manifest::REALM),
    );
    let mut module = LlModule::new(target);
    emit_program_runtime_layouts(&mut module, &reach).unwrap();
    let ir = module.to_ir();
    let cells = ir
        .lines()
        .filter(|l| {
            l.starts_with("@perry_runtime_layout_")
                && l.split(" = ")
                    .next()
                    .is_some_and(|n| n.ends_with("_list") || n.ends_with("_list_attrs"))
        })
        .count();
    assert!(cells > 0, "no runtime key list was emitted");
    let asm = crate::native_root_coverage::assembly_for(&ir, target);
    let lines = asm.lines().map(str::trim).collect::<Vec<_>>();
    let mut tied = 0;
    for (at, line) in lines.iter().enumerate() {
        let Some(rest) = line.strip_prefix(".section\tperry_realm,") else {
            continue;
        };
        let fields = rest.split(',').collect::<Vec<_>>();
        assert_eq!(
            fields.first(),
            Some(&"\"awo\""),
            "realm section without link order: {line}"
        );
        let owner = fields[2];
        let label = lines[at + 1..]
            .iter()
            .find_map(|l| {
                l.strip_suffix(':')
                    .filter(|n| !n.contains(char::is_whitespace))
            })
            .unwrap();
        let cell = label.trim_start_matches(".L");
        assert!(
            cell == format!("{owner}_list") || cell == format!("{owner}_list_attrs"),
            "{cell} is linked to {owner}, not to its own descriptor"
        );
        let defined = lines
            .iter()
            .position(|l| *l == format!("{owner}:"))
            .unwrap();
        assert!(
            defined < at,
            "{cell}: its descriptor {owner} is defined after the list's section"
        );
        tied += 1;
    }
    assert_eq!(tied, cells, "every list cell has exactly one tied section");
}
