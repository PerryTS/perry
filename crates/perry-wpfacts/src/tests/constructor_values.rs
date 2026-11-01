use super::{facts, field, tmpl};
use crate::{compute, FactsConfig};
use perry_hir::{Expr, Stmt};

fn capture_facts(unknown_in_constructor_slot: bool, snapshot: bool) -> crate::ProgramFacts {
    let parsed = perry_parser::parse_typescript(
        "class Payload { value = 7; } class Known { constructor(x: any) { this.item = x; } } class Host {}",
        "t.ts",
    ).unwrap();
    let mut hir = perry_hir::lower_module(&parsed, "t", "t.ts").unwrap();
    let unknown = Expr::ExternFuncRef {
        name: "externalCtor".into(),
        param_types: vec![],
        return_type: perry_hir::types::Type::Any,
    };
    hir.init.push(Stmt::Expr(Expr::RegisterClassCaptures {
        class_name: "Host".into(),
        captures: vec![Expr::ClassRef("Known".into()), unknown.clone()],
    }));
    hir.init.push(Stmt::Expr(Expr::ClassEnvSet {
        class_name: "Host".into(),
        index: if unknown_in_constructor_slot { 0 } else { 1 },
        value: Box::new(unknown),
        guarded: true,
        publish: false,
    }));
    let callee = if snapshot {
        Expr::ClassCaptureValue {
            class_name: "Host".into(),
            index: 0,
            fallback: None,
            prefer_fallback: false,
        }
    } else {
        Expr::ClassEnvGet {
            class_name: "Host".into(),
            index: 0,
            guarded: true,
        }
    };
    hir.init.push(Stmt::Expr(Expr::NewDynamic {
        callee: Box::new(callee),
        args: vec![Expr::New {
            class_name: "Payload".into(),
            args: vec![],
            type_args: vec![],
            byte_offset: 0,
            cap_args_appended: 0,
            byte_end: 0,
        }],
        byte_offset: 0,
        byte_end: 0,
    }));
    compute(&[("t.ts".into(), &hir)], &FactsConfig::default())
}

#[test]
fn capture_slots_keep_constructor_identity_separate_from_unknown_captures() {
    for snapshot in [false, true] {
        let pf = capture_facts(false, snapshot);
        assert!(tmpl(&pf, "Payload").prefix_stable, "{}", pf.dump_text());
        assert!(
            field(tmpl(&pf, "Known"), "item")
                .kind
                .desc()
                .contains("obj"),
            "{}",
            pf.dump_text()
        );
    }
}

#[test]
fn unknown_constructor_alternative_in_the_same_capture_slot_still_escapes_arguments() {
    for snapshot in [false, true] {
        let pf = capture_facts(true, snapshot);
        assert!(!tmpl(&pf, "Payload").prefix_stable, "{}", pf.dump_text());
    }
}

#[test]
fn property_and_parameter_constructor_values_preserve_parameters_and_return_substitution() {
    let pf = facts(
        r#"
        class Payload { value = 7; }
        class Replacement { payload: any; constructor(p: any) { this.payload = p; } }
        function Ctor(this: any, p: any) { this.unused = 1; return new Replacement(p); }
        const holder = { klass: Ctor };
        function make(C: any, p: any) { return new C(p); }
        const result = make(holder.klass, new Payload());
        console.log(result.payload.value);
    "#,
    );
    assert!(tmpl(&pf, "Payload").prefix_stable, "{}", pf.dump_text());
    let sites: Vec<_> = pf.sites.iter().filter(|s| s.key == "value").collect();
    assert!(!sites.is_empty());
    assert!(sites.iter().all(|s| !s.top), "{}", pf.dump_text());
}

#[test]
fn builtin_promise_spread_executor_is_connected() {
    let pf = facts(
        r#"
        class Payload { value = 7; }
        const P = globalThis.Promise;
        const args: any[] = [(resolve: any) => resolve(new Payload())];
        new P(...args).then((p: any) => console.log(p.value));
    "#,
    );
    let sites: Vec<_> = pf.sites.iter().filter(|s| s.key == "value").collect();
    assert!(!sites.is_empty());
    assert!(
        sites
            .iter()
            .all(|s| s.receivers.iter().any(|r| r.ends_with(":Payload"))),
        "{}",
        pf.dump_text()
    );
}

#[test]
fn class_expression_constructor_capture_keeps_other_capture_slots_separate() {
    let pf = facts(
        r#"
        class Payload { value = 7; }
        class Box { item: any; constructor(p: any) { this.item = p; } }
        function factory(Ctor: any, unrelated: any) {
            return class {
                create(p: any) { return new Ctor(p); }
                other() { return unrelated; }
            };
        }
        const Maker = factory(Box, externalFactory());
        const maker = new Maker();
        const out = maker.create(new Payload());
        console.log(out.item.value);
    "#,
    );
    assert!(tmpl(&pf, "Payload").prefix_stable, "{}", pf.dump_text());
    let sites: Vec<_> = pf.sites.iter().filter(|s| s.key == "value").collect();
    assert!(!sites.is_empty());
    assert!(sites.iter().all(|s| !s.top), "{}", pf.dump_text());
}
