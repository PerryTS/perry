//! A canonical field-result contract must survive arithmetic; checked facts
//! and erased declarations must retain their complete value-dependent exits.
use crate::{
    compile_module, CompileOptions, ElisionFacts, FactsClass, FactsField, FactsRecv,
    ProgramFactsView,
};
use std::sync::Arc;

fn body(with_facts: bool, checked: bool, interp: bool, kind: u16) -> String {
    let source = "class Cell { x: number; constructor(x: number) { this.x = x; } } function run(o: Cell, a: number[]) { a.push((o.x + 1) % 10); return (o.x + 1) % 10; }";
    let mut cache = perry_diagnostics::SourceCache::new();
    let parsed =
        perry_parser::parse_typescript_with_cache(source, "result.ts", &mut cache).unwrap();
    let m = perry_hir::lower_module(&parsed.module, "result", "result.ts").unwrap();
    let c = m.classes.iter().find(|c| c.name == "Cell").unwrap();
    let f = m.functions.iter().find(|f| f.name == "run").unwrap();
    let mut facts = ElisionFacts::default();
    if with_facts {
        facts.locals.insert(
            f.params[0].id,
            FactsRecv {
                class_ids: vec![c.id],
                nullable: false,
                checked,
            },
        );
        facts.classes.insert(
            c.id,
            FactsClass {
                display: c.name.clone(),
                fixed_layout: true,
                fields: vec![FactsField {
                    classes: Vec::new(),
                    nullable: false,
                    key: "x".into(),
                    slot: 0,
                    readonly: false,
                    kind_mask: kind,
                    ta_stable: false,
                    ta_kind: None,
                    declared: false,
                    reserved: false,
                    fns: Vec::new(),
                }],
            },
        );
    }
    facts.interp_guard = interp;
    let ir = String::from_utf8(
        compile_module(
            &m,
            CompileOptions {
                emit_ir_only: true,
                program_facts: ProgramFactsView {
                    elision: Arc::new(facts),
                    ..Default::default()
                },
                ..Default::default()
            },
        )
        .unwrap(),
    )
    .unwrap();
    let start = ir
        .lines()
        .find(|l| l.starts_with("define ") && l.contains("__run("))
        .unwrap();
    let tail = &ir[ir.find(start).unwrap()..];
    tail.split("\n}").next().unwrap().to_string()
}

#[test]
fn canonical_field_result_flows_through_add_remainder_and_push() {
    let ir = body(true, false, false, 1);
    assert!(
        ir.contains("fadd double") && ir.contains("@perry_checked_number_remainder("),
        "live arithmetic subject: {ir}"
    );
    assert!(
        !ir.contains("call double @js_number_coerce("),
        "canonical result must not be recoerced: {ir}"
    );
    assert!(
        !ir.contains("call double @js_dynamic_string_or_number_add("),
        "closed Number field cannot concatenate: {ir}"
    );
    assert!(
        !ir.contains("call void @js_string_addref_if_heap_string("),
        "canonical arithmetic cannot publish a heap string: {ir}"
    );
}

#[test]
fn incomplete_field_contracts_keep_dynamic_value_exits() {
    for args in [
        (false, false, false, 1),
        (true, true, false, 1),
        (true, false, true, 1),
        (true, false, false, 3),
    ] {
        let ir = body(args.0, args.1, args.2, args.3);
        assert!(
            ir.contains("call double @js_dynamic_string_or_number_add("),
            "unproved field may concatenate {args:?}: {ir}"
        );
        assert!(
            ir.contains("call double @js_number_coerce(")
                || ir.contains("call double @js_dynamic_mod("),
            "dynamic add result still needs ToNumber or ToNumeric {args:?}: {ir}"
        );
    }
}
