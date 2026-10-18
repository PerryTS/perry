use super::*;

#[test]
fn super_heritage_proof_tracks_the_constructor_target() {
    let pf = facts(
        r#"
        class A {} class B extends A {} class C extends A {}
        Object.setPrototypeOf(B, A);
        console.log(new B(), new C());
    "#,
    );
    assert!(!pf
        .super_unrelinked
        .contains(&tmpl(&pf, "B").class_id.unwrap()));
    assert!(pf
        .super_unrelinked
        .contains(&tmpl(&pf, "C").class_id.unwrap()));
}

#[test]
fn super_heritage_proof_declines_unknown_proto_writes() {
    let pf = facts(
        r#"
        class A {} class B extends A {}
        function change(o: any, p: any) { o.__proto__ = p; }
        change(B, A); console.log(new B());
    "#,
    );
    assert!(!pf
        .super_unrelinked
        .contains(&tmpl(&pf, "B").class_id.unwrap()));
}

#[test]
fn super_heritage_relink_revokes_birth_fields_without_harming_unrelated_class() {
    let pf = facts(
        r#"
        class A { a = 1; }
        class B extends A { b = 2; constructor() { super(); } }
        class Other { constructor() { this.other = 3; } }
        class Untouched { z = 4; }
        Object.setPrototypeOf(B, Other);
        new B(); console.log(new Untouched().z);
    "#,
    );
    assert!(!tmpl(&pf, "B").prefix_stable, "{}", pf.dump_text());
    assert!(tmpl(&pf, "Untouched").prefix_stable, "{}", pf.dump_text());
}

#[test]
fn super_heritage_unknown_target_does_not_kill_unreachable_constructors() {
    let pf = facts(
        r#"
        class A {} class Hidden extends A { x = 1; }
        function unknown(target: any, parent: any) { Object.setPrototypeOf(target, parent); }
        unknown(JSON.parse('{}'), {}); new Hidden();
    "#,
    );
    assert!(pf
        .super_unrelinked
        .contains(&tmpl(&pf, "Hidden").class_id.unwrap()));
}

#[test]
fn super_heritage_specialized_layout_uses_source_constructor() {
    let source = r#"
        class A { a = 1; }
        class B<T> extends A { b = 2; }
        class Other { constructor() { this.other = 3; } }
        Object.setPrototypeOf(B, Other);
    "#;
    let parsed = perry_parser::parse_typescript(source, "t.ts").unwrap();
    let mut hir = perry_hir::lower_module(&parsed, "t", "t.ts").unwrap();
    let origin = hir.classes.iter().find(|c| c.name == "B").unwrap();
    let specialized =
        perry_hir::monomorph::specialize_class(origin, &[perry_hir::types::Type::Number], 9001);
    let name = specialized.name.clone();
    hir.classes.push(specialized);
    hir.init.push(perry_hir::Stmt::Expr(perry_hir::Expr::New {
        class_name: name.clone(),
        args: vec![],
        type_args: vec![],
        byte_offset: 0,
        cap_args_appended: 0,
    }));
    let pf = compute(&[("t.ts".into(), &hir)], &FactsConfig::default());
    assert!(!tmpl(&pf, &name).prefix_stable, "{}", pf.dump_text());
    assert!(!pf.super_unrelinked.contains(&9001), "{}", pf.dump_text());
}
