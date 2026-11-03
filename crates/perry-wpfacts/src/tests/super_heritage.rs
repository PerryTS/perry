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
        byte_end: 0,
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

/// A derived class without a constructor gets `constructor(...a) {
/// super(...a) }`; a class that captures locals gets it with capture slots
/// after the rest parameter. The parent's parameters take the `new` call's
/// arguments position by position, not every argument each (DECISIONS 49).
#[test]
fn default_derived_ctor_forwards_by_position() {
    let pf = facts(
        r#"
        function make() {
            const k = { z: 1 };
            class P { x: number; constructor() { this.x = 1; } }
            class Q { y: number; constructor() { this.y = 2; } }
            class Base { a: any; b: any; constructor(a: any, b: any) { this.a = a; this.b = b; } }
            class D extends Base { kz() { return k.z; } }
            return new D(new P(), new Q());
        }
        const d = make();
        let t = 0; for (let i = 0; i < 3; i++) t += d.a.x + d.b.y + d.kz();
        console.log(t);
    "#,
    );
    for k in ["x", "y"] {
        assert_eq!(
            site(&pf, k).verdict,
            Verdict::T0Mono,
            "{k}\n{}",
            pf.dump_text()
        );
    }
}

/// The capture slots after the rest parameter take no user argument: the
/// rest takes them all, so a captured value keeps its own shape.
#[test]
fn capture_slot_after_rest_takes_no_user_argument() {
    let pf = facts(
        r#"
        function make() {
            const k = { z: 1 };
            class P { x: number; constructor() { this.x = 1; } }
            class Base { a: any; constructor(a: any) { this.a = a; } }
            class D extends Base { kz() { return k.z; } }
            return new D(new P());
        }
        const d = make();
        let t = 0; for (let i = 0; i < 3; i++) t += d.kz();
        console.log(t);
    "#,
    );
    assert_eq!(
        site(&pf, "z").verdict,
        Verdict::T0Mono,
        "{}",
        pf.dump_text()
    );
}

/// A class declared in a function whose members name it: the HIR makes its
/// self binding a one-element cell (`owner = [undefined]`, read as
/// `owner[0]`) and runs its static initializers where it is evaluated. The
/// class goes into the cell's element, and the declaration's unrewritten
/// copy of the initializers is not run (DECISIONS 50).
const NESTED_SELF: &str = r#"
    function make() {
        class S {
            v: number;
            constructor(v: number) { this.v = v; }
            static mk() { return new S(2); }
            static one = S.mk();
            get() { return S.one.v; }
        }
        return S;
    }
    const A = make(); make();
    let t = 0; for (let i = 0; i < 3; i++) t += A.one.v + A.one.get();
    console.log(t);
"#;

#[test]
fn nested_class_static_initializer_runs_at_its_evaluation() {
    let pf = facts(NESTED_SELF);
    assert_eq!(tmpl(&pf, "S").escape, None, "{}", pf.dump_text());
    let v: Vec<_> = pf.sites.iter().filter(|s| s.key == "v").collect();
    assert!(
        !v.is_empty() && v.iter().all(|s| s.verdict == Verdict::T0Mono),
        "{}",
        pf.dump_text()
    );
}

#[test]
fn class_self_cell_holds_the_class_in_its_element() {
    let pf = facts(NESTED_SELF);
    let mk: Vec<_> = pf.sites.iter().filter(|s| s.key == "mk").collect();
    assert!(!mk.is_empty(), "{}", pf.dump_text());
    for s in mk {
        assert_eq!(
            s.receivers,
            vec!["class:t.ts:S".to_string()],
            "{}",
            pf.dump_text()
        );
    }
}
