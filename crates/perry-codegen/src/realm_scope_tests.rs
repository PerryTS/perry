//! Bounds on emitted image cells use the physical plan, not logical reach.
use crate::codegen::realm_image::plan::ImagePlan;
use perry_hir::realm_manifest::RealmKind;
use perry_hir::realm_reach::{scan_for_source, scan_lowered_for_scope};
use perry_hir::realm_reach_index::RealmIndex;

fn image(source: &str) -> (ImagePlan, perry_hir::realm_reach::RealmReach) {
    let ast = perry_parser::parse_typescript(source, "t.ts").unwrap();
    let hir = perry_hir::lower_module(&ast, "t", "t.ts").unwrap();
    let facts = perry_wpfacts::compute(
        &[("t.ts".to_string(), &hir)],
        &perry_wpfacts::FactsConfig {
            max_work: Some(50_000_000),
            ..Default::default()
        },
    );
    assert!(facts.realm_solve_complete);
    let index = RealmIndex::new(crate::realm_manifest::REALM);
    let mut reach = scan_for_source(&ast, &index, "t.ts");
    scan_lowered_for_scope(&hir, &index, &mut reach);
    let mut conservative = reach.clone();
    conservative.close(&index);
    reach.resolve_observations(&facts.realm_operand_index(), &index);
    reach.close(&index);
    assert!(reach.names.is_subset(&conservative.names));
    let plan = ImagePlan::new(&reach, "x86_64-unknown-linux-gnu").unwrap();
    (plan, reach)
}

fn functions(plan: &ImagePlan) -> usize {
    let index = RealmIndex::new(crate::realm_manifest::REALM);
    plan.realm
        .cells
        .keys()
        .filter_map(|name| index.last(name))
        .filter(|row| matches!(row.kind(), RealmKind::Function | RealmKind::Method))
        .count()
}

#[test]
fn realm_scope_template_emits_at_most_six_function_cells() {
    let (plan, reach) = image("let name=process.argv[2]; console.log(`Hello, ${name}!`)");
    eprintln!(
        "template logical R{} W{}; emitted cells{} functions{}",
        reach.names.len(),
        reach.whole.len(),
        plan.realm.cells.len(),
        functions(&plan)
    );
    assert!(functions(&plan) <= 6);
}

#[test]
fn realm_scope_json_bounds_apply_to_emitted_cells() {
    let (plan, reach) = image("const o=JSON.parse('{\"name\":\"a\"}',(k,v)=>typeof v==='string'?v.toUpperCase():v);console.log(o.name)");
    let whole = plan
        .realm
        .cells
        .keys()
        .filter(|name| reach.whole.contains(*name))
        .count();
    eprintln!(
        "JSON logical R{} W{}; emitted cells{} whole{} functions{}",
        reach.names.len(),
        reach.whole.len(),
        plan.realm.cells.len(),
        whole,
        functions(&plan)
    );
    assert!(plan.realm.cells.len() <= 110);
    assert!(whole <= 50);
    assert!(functions(&plan) <= 60);
}
