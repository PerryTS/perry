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
fn original_top_level_commonjs_class_is_distinct_from_an_unproven_nested_class() {
    let (mut leaf, layouts, _) = fixture();
    leaf.classes[0].is_nested = true;
    assert!(
        births(&leaf, &layouts).is_empty(),
        "an arbitrary nested class stays refused"
    );
    let marker = module(
        "/marker",
        "import {createRequire as __perry_cjs_create_require} from 'module';",
        0,
    );
    leaf.imports.extend(marker.imports);
    leaf.class_family_syntax = Some(perry_hir::ir::ClassFamilySyntax {
        parents: [("Leaf".into(), ("/base".into(), "Base".into()))].into(),
        ..Default::default()
    });
    assert!(leaf.is_commonjs_wrap());
    assert_eq!(
        births(&leaf, &layouts).len(),
        1,
        "the original top-level declaration is admitted"
    );
    leaf.class_family_syntax.as_mut().unwrap().parents.clear();
    assert!(
        births(&leaf, &layouts).is_empty(),
        "the source proof is necessary"
    );
}
