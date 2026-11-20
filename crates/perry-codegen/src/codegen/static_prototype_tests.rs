//! Admission must be live for evaluated heritage, while the family and
//! original CommonJS syntax remain separate necessary proofs.
use super::*;
use crate::layout_families::{compute, ProgramLayouts};
use perry_hir::Module;
use std::collections::BTreeMap;

fn module(path: &str, source: &str, offset: u32) -> Module {
    let ast = perry_parser::parse_typescript(source, path).unwrap();
    let mut hir = perry_hir::lower_module(&ast, path, path).unwrap();
    for c in &mut hir.classes {
        c.id += offset;
    }
    for import in &mut hir.imports {
        import.resolved_path = Some(import.source.clone());
    }
    hir
}
fn fixture() -> (Module, ProgramLayouts, u32) {
    let base = module(
        "/base",
        "export class Base { constructor() { this.base = 1; } }",
        100,
    );
    let leaf = module("/leaf", "import {Base} from '/base'; class Leaf extends Base { constructor() { super(); this.leaf = 2; } own() { return this.leaf; } }", 200);
    let layouts = compute(
        [("/base", &base), ("/leaf", &leaf)].into_iter(),
        true,
        &BTreeMap::new(),
    );
    (leaf, layouts, base.classes[0].id)
}
fn births(hir: &Module, layouts: &ProgramLayouts) -> Vec<ModuleBirth> {
    let ids = hir.classes.iter().map(|c| (c.name.clone(), c.id)).collect();
    collect(hir, "leaf", &ids, layouts)
}
#[test]
fn evaluated_parent_is_a_candidate_only_with_its_admitted_family() {
    let (leaf, layouts, parent) = fixture();
    assert!(
        leaf.classes[0].extends_expr.is_some(),
        "fixture must exercise evaluated heritage"
    );
    let result = births(&leaf, &layouts);
    assert_eq!(result.len(), 1);
    assert_eq!(
        result[0].shape.proto,
        BirthProto::Prototype(leaf.classes[0].id, parent)
    );
    assert_eq!(
        result[0].shape.constfn.len(),
        1,
        "the own body must carry a ConstFn lane"
    );
    assert!(births(&leaf, &ProgramLayouts::default()).is_empty());
}
#[test]
fn nested_evaluation_needs_the_structural_run_once_program_proof() {
    let (mut leaf, mut layouts, _) = fixture();
    leaf.classes[0].is_nested = true;
    assert!(births(&leaf, &layouts).is_empty());
    // This fact is computed from original-source structure and the program
    // capability census, independently of runtime observations.
    layouts.run_once_evaluated.insert(leaf.classes[0].id);
    assert_eq!(births(&leaf, &layouts).len(), 1);
    layouts.run_once_evaluated.clear();
    assert!(births(&leaf, &layouts).is_empty());
}
#[test]
fn run_once_program_proof_is_revoked_by_loader_escape_or_repeated_structure() {
    // Deferred dynamic-code capability state belongs to a whole compilation
    // and is shared with parallel lowering. Run this whole-program proof in
    // a fresh process so unrelated compilation fixtures cannot change it.
    // The child is started the way libtest's runner starts a per-test process
    // under the workspace's `panic-abort-tests` (`__RUST_TEST_INVOKE=<libtest
    // name>`, no arguments; exit 50, libtest's `TR_OK`, is a pass). The marker
    // variable is what makes it the child: libtest runs every test in a
    // re-executed process, so arguments or the test name are the same in every
    // generation and cannot tell the child from its parent.
    const CHILD: &str = "PERRY_TEST_RUN_ONCE_PROOF_CHILD";
    if std::env::var_os(CHILD).is_none() {
        let module = module_path!()
            .split_once("::")
            .map_or("", |(_crate, module)| module);
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .env(
                "__RUST_TEST_INVOKE",
                format!(
                    "{module}::run_once_program_proof_is_revoked_by_loader_escape_or_repeated_structure"
                ),
            )
            .env(CHILD, "1")
            .output()
            .unwrap();
        assert_eq!(
            output.status.code(),
            Some(50),
            "run-once program proof child failed ({}): {}\n{}",
            output.status,
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        return;
    }
    assert!(!perry_hir::has_deferred_dynamic_code_sites());
    let mut base = module(
        "/base",
        "export class Base { constructor() { this.base = 1; } }",
        100,
    );
    let mut leaf = module("/leaf", "import {Base} from '/base'; class Leaf extends Base { constructor() { super(); this.leaf = 2; } own() { return this.leaf; } }", 200);
    base.class_family_syntax = Some(perry_hir::ir::ClassFamilySyntax::default());
    leaf.class_family_syntax = Some(perry_hir::ir::ClassFamilySyntax {
        run_once_classes: ["Leaf".into()].into(),
        ..Default::default()
    });
    let proof = |base: &Module, leaf: &Module| {
        compute([("/base", base), ("/leaf", leaf)], true, &BTreeMap::new())
    };
    let cid = leaf.classes[0].id;
    assert!(proof(&base, &leaf).run_once_evaluated.contains(&cid));
    base.class_family_syntax
        .as_mut()
        .unwrap()
        .module_capability_uses = 1;
    assert!(!proof(&base, &leaf).run_once_evaluated.contains(&cid));
    base.class_family_syntax
        .as_mut()
        .unwrap()
        .module_capability_uses = 0;
    leaf.class_family_syntax
        .as_mut()
        .unwrap()
        .run_once_classes
        .clear();
    assert!(!proof(&base, &leaf).run_once_evaluated.contains(&cid));
}

#[test]
fn local_evaluated_heritage_keeps_its_ordinary_birth_path() {
    let mut hir = module("/same", "class Base { constructor() { this.base = 1; } } class Leaf extends Base { constructor() { super(); this.leaf = 2; } own() { return this.leaf; } }", 300);
    let leaf = hir.classes.iter_mut().find(|c| c.name == "Leaf").unwrap();
    let cid = leaf.id;
    leaf.extends_expr = Some(Box::new(perry_hir::Expr::ClassRef("Base".into())));
    hir.class_family_syntax = Some(perry_hir::ir::ClassFamilySyntax {
        run_once_classes: ["Leaf".to_string()].into(),
        ..Default::default()
    });
    let layouts = compute([("/same", &hir)], true, &BTreeMap::new());
    assert!(layouts.families.contains_key(&cid));
    assert!(!layouts.run_once_evaluated.contains(&cid));
    assert!(!births(&hir, &layouts)
        .iter()
        .any(|b| matches!(b.shape.proto, BirthProto::Prototype(id, _) if id == cid)));
}
#[test]
fn known_symbol_members_are_birth_keys_after_the_strings() {
    // A getter-only `[Symbol.toStringTag]` is an ordinary member too (it was
    // lifted to a class hook and never reached the prototype).
    let hir = module(
        "/sym",
        "class C { a() { return 1; } *[Symbol.iterator]() { yield 1; } get [Symbol.toStringTag]() { return 'C'; } [Symbol.for('t')]() { return 2; } b() { return 3; } }",
        300,
    );
    let ids = hir.classes.iter().map(|c| (c.name.clone(), c.id)).collect();
    let result = collect(&hir, "sym", &ids, &ProgramLayouts::default());
    assert_eq!(
        result.len(),
        1,
        "a known-symbol member must not refuse the class"
    );
    let shape = &result[0].shape;
    let keys: Vec<&[u8]> = shape
        .keys
        .split(|b| *b == 0)
        .filter(|k| !k.is_empty())
        .collect();
    let wk = |name: &str| {
        let i = crate::runtime_abi::WELL_KNOWN_SYMBOLS
            .iter()
            .position(|w| *w == name)
            .unwrap();
        format!("\u{1}@@wk:{i}")
    };
    let expected = [
        "constructor".to_string(),
        "a".to_string(),
        "b".to_string(),
        wk("iterator"),
        wk("toStringTag"),
        "\u{1}@@for:t".to_string(),
    ];
    assert_eq!(
        keys,
        expected.iter().map(|k| k.as_bytes()).collect::<Vec<_>>(),
        "strings by definition, then symbols by definition"
    );
    assert_eq!(shape.key_count, 6);
    // Method slots carry a ConstFn lane; the accessor slot (4) does not.
    let mut lanes: Vec<u32> = shape.constfn.iter().map(|c| c.slot).collect();
    lanes.sort();
    assert_eq!(lanes, vec![1, 2, 3, 5]);
}
#[test]
fn known_symbol_member_past_slot_31_gets_a_constfn_birth() {
    let methods: String = (0..40)
        .map(|i| format!("m{i}() {{ return {i}; }} "))
        .collect();
    let hir = module(
        "/sym40",
        &format!("class C {{ {methods} *[Symbol.iterator]() {{ yield 1; }} }}"),
        310,
    );
    let ids = hir.classes.iter().map(|c| (c.name.clone(), c.id)).collect();
    let result = collect(&hir, "sym40", &ids, &ProgramLayouts::default());
    assert_eq!(result.len(), 1);
    let shape = &result[0].shape;
    // Slot 0 `constructor`, 1..=40 the string methods, 41 `[Symbol.iterator]`.
    let slots: Vec<u32> = shape.constfn.iter().map(|e| e.slot).collect();
    assert_eq!(slots, (1..=41).collect::<Vec<u32>>());
    let special: u64 = (1..32).fold(0, |rep, slot| rep | (3u64 << (2 * slot)));
    assert_eq!(shape.rep, special);
}
#[test]
fn methods_past_slot_31_get_constfn_births() {
    let methods: String = (0..100)
        .map(|i| format!("m{i}() {{ return {i}; }} "))
        .collect();
    let base = module(
        "/base",
        "export class Base { constructor() { this.base = 1; } }",
        100,
    );
    let leaf = module(
        "/leaf",
        &format!("import {{Base}} from '/base'; class Leaf extends Base {{ constructor() {{ super(); this.leaf = 2; }} {methods} }}"),
        200,
    );
    let layouts = compute(
        [("/base", &base), ("/leaf", &leaf)].into_iter(),
        true,
        &BTreeMap::new(),
    );
    let result = births(&leaf, &layouts);
    assert_eq!(result.len(), 1);
    let shape = &result[0].shape;
    // Slot 0 is `constructor`, slots 1..=100 the methods: every one has a body.
    let slots: Vec<u32> = shape.constfn.iter().map(|e| e.slot).collect();
    assert_eq!(slots, (1..=100).collect::<Vec<u32>>());
    // Word 0 carries the lanes 1..32 and nothing past it.
    let special: u64 = (1..32).fold(0, |rep, slot| rep | (3u64 << (2 * slot)));
    assert_eq!(shape.rep, special);
}
