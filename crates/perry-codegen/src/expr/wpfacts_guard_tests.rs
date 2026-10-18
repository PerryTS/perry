//! Decision 150: class identity alone cannot admit a guarded field read.

use crate::codegen::{ElisionFacts, FactsClass, FactsField, FactsRecv};
use crate::compile_module;
use std::collections::{BTreeMap, HashSet};

fn emit(checked: bool, interp_guard: bool) -> String {
    let ast = perry_parser::parse_typescript(
        "class C { x = 1; } function read(o: any) { return o.x; } console.log(read(new C()));",
        "guard.ts",
    )
    .unwrap();
    let mut module = perry_hir::lower_module(&ast, "guard", "guard.ts").unwrap();
    module.init_kind = perry_hir::ModuleInitKind::Eager;
    let class = module.classes.iter().find(|c| c.name == "C").unwrap();
    let read = module.functions.iter().find(|f| f.name == "read").unwrap();
    let mut opts = super::class_field_barrier_tests::ir_opts();
    opts.program_facts.elision = std::sync::Arc::new(ElisionFacts {
        locals: [(
            read.params[0].id,
            FactsRecv {
                class_ids: vec![class.id],
                checked,
                nullable: false,
            },
        )]
        .into(),
        classes: [(
            class.id,
            FactsClass {
                display: "guard.ts:C".into(),
                fixed_layout: true,
                fields: vec![FactsField {
                    key: "x".into(),
                    slot: 0,
                    readonly: true,
                    kind_mask: 1,
                    ta_stable: false,
                    ta_kind: None,
                }],
            },
        )]
        .into(),
        interp_guard,
        ..Default::default()
    });
    String::from_utf8(compile_module(&module, opts).unwrap()).unwrap()
}

fn variables(line: &str) -> Vec<String> {
    line.split(|c: char| !(c.is_ascii_alphanumeric() || c == '%' || c == '_' || c == '.'))
        .filter(|s| s.starts_with('%'))
        .map(str::to_string)
        .collect()
}

fn depends(defs: &BTreeMap<String, Vec<String>>, from: &str, target: &str) -> bool {
    let mut todo = vec![from.to_string()];
    let mut seen = HashSet::new();
    while let Some(var) = todo.pop() {
        if var == target {
            return true;
        }
        if seen.insert(var.clone()) {
            if let Some(inputs) = defs.get(&var) {
                todo.extend(inputs.iter().cloned());
            }
        }
    }
    false
}

fn check(checked: bool, interp_guard: bool) {
    let ir = emit(checked, interp_guard);
    let lines: Vec<_> = ir.lines().collect();
    let start = lines
        .iter()
        .position(|line| line.contains("wpf.gget.header") && line.trim_end().ends_with(':'))
        .unwrap_or_else(|| panic!("guard must be live: {ir}"));
    let block: Vec<_> = lines[start + 1..]
        .iter()
        .take_while(|line| !line.contains(':'))
        .copied()
        .collect();
    let limit = format!(", {}", crate::runtime_abi::UNSHADOWED_WORD_END as i32);
    let compare = block
        .iter()
        .find(|line| line.contains("icmp ult i32") && line.contains(&limit))
        .unwrap_or_else(|| panic!("guard must test the real layout word: {ir}"));
    let layout_result = variables(compare)[0].clone();
    let branch = block
        .iter()
        .find(|line| line.trim_start().starts_with("br i1 "))
        .unwrap();
    let vars = variables(branch);
    assert!(
        vars[1].contains("wpf.gget.hit"),
        "true edge must load the proven slot: {branch}"
    );
    assert!(
        vars[2].contains("wpf.gget.miss"),
        "false edge must perform the full operation: {branch}"
    );
    let mut defs = BTreeMap::new();
    for line in &block {
        if line.contains(" = ") {
            let mut vars = variables(line).into_iter();
            defs.insert(vars.next().unwrap(), vars.collect());
        }
    }
    assert!(
        depends(&defs, &vars[0], &layout_result),
        "layout check must control the hit edge: {ir}"
    );
    // The compare must use the high word read from the actual object header.
    let compare_input = variables(compare)[1].clone();
    let load = block
        .iter()
        .find(|line| line.contains(" = load i64, ptr "))
        .unwrap();
    let header = variables(load)[0].clone();
    assert!(
        depends(&defs, &compare_input, &header),
        "compare must read the actual object: {ir}"
    );
    assert!(
        block
            .iter()
            .any(|line| line.contains("lshr i64") && line.contains(", 32")),
        "layout is the high header word: {ir}"
    );
    for symbol in ["@js_object_get_field_slow", "@js_object_get_field_nonptr"] {
        assert!(
            ir.lines()
                .any(|line| line.contains("call ") && line.contains(symbol)),
            "generic miss must retain the object and primitive property routes: {ir}"
        );
    }
    if interp_guard {
        assert!(ir.contains("load i32, ptr @PERRY_INTERP_PROTO_WRITTEN"));
    }
}

#[test]
fn checked_receiver_layout_controls_the_guarded_field_hit() {
    check(true, false);
}

#[test]
fn interpreted_reach_layout_controls_the_guarded_field_hit() {
    check(false, true);
}
