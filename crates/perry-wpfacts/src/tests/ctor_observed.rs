//! A key the constructor assigns after code that runs with `this` (a method
//! call on `this`, a `super` method or accessor, a read of a key that is not
//! own yet) can be observed unset: its kind holds `undefined`, and a key
//! the constructor stores is reserved (known slot, presence not known).
//! Per unit agrees with the whole program.

use super::*;

fn units(src: &str) -> ProgramFacts {
    let module = perry_parser::parse_typescript(src, "t.ts").expect("parses");
    let hir = perry_hir::lower_module(&module, "t", "t.ts").expect("lowers");
    crate::compute_units(&[("t.ts".to_string(), &hir)], &FactsConfig::default())
}

/// In both modes: class `A`'s field `p` holds `undefined` exactly when the
/// constructor can be observed before it assigns `p`.
fn check(src: &str, observed: bool) {
    for pf in [facts(src), units(src)] {
        let p = field(tmpl(&pf, "A"), "p");
        let undef = p.kind.prims & crate::model::prim::UNDEF != 0;
        assert_eq!(undef, observed, "{}", pf.dump_text());
    }
}

#[test]
fn assigned_before_any_observation_is_definite() {
    check(
        r#"
        class P { v = 5; }
        class A { p: P; s: string; constructor() { this.p = new P(); this.s = this.peek(); }
          peek(): string { return typeof this.p; } }
        console.log(new A().s);
        "#,
        false,
    );
}

#[test]
fn a_method_call_on_this_observes_the_unassigned_key() {
    check(
        r#"
        class P { v = 5; }
        class A { p: P; s: string; constructor(e: boolean) { this.s = e ? this.peek() : "-"; this.p = new P(); }
          peek(): string { return typeof this.p; } }
        console.log(new A(true).s);
        "#,
        true,
    );
}

#[test]
fn a_getter_read_on_this_observes_the_unassigned_key() {
    check(
        r#"
        class P { v = 5; }
        class A { p: P; s: string; constructor() { this.s = this.peek; this.p = new P(); }
          get peek(): string { return typeof this.p; } }
        console.log(new A().s);
        "#,
        true,
    );
}

#[test]
fn a_super_method_call_observes_the_unassigned_key() {
    check(
        r#"
        class P { v = 5; }
        class B { peek(): string { return typeof (this as any).p; } }
        class A extends B { p: P; s: string; constructor() { super(); this.s = super.peek(); this.p = new P(); } }
        console.log(new A().s);
        "#,
        true,
    );
}

#[test]
fn an_undeclared_key_assigned_after_a_method_call_holds_undefined() {
    check(
        r#"
        class P { v = 5; }
        class A { constructor(e) { this.s = e ? this.peek() : "-"; this.p = new P(); }
          peek() { return typeof this.p; } }
        console.log(new A(true).s);
        "#,
        true,
    );
}

/// A key the constructor stores (a reservation, `ClassFieldOrigin::
/// ConstructorStore`) keeps its slot either way; it is present from birth
/// on only when no code can observe the instance before the store. Else it
/// is reserved: known slot, unknown presence (the slot holds the hole), and
/// the layout is not fixed (an observer may see the hole).
fn check_presence(src: &str, observed: bool) {
    for pf in [facts(src), units(src)] {
        let t = tmpl(&pf, "A");
        let p = field(t, "p");
        assert_eq!(p.slot, Some(1), "{}", pf.dump_text());
        assert_eq!(p.definite, !observed, "{}", pf.dump_text());
        assert_eq!(p.reserved, observed, "{}", pf.dump_text());
        assert_eq!(t.fixed_layout, !observed, "{}", pf.dump_text());
    }
}

#[test]
fn a_constructor_store_after_a_method_call_is_reserved() {
    check_presence(
        r#"
        class A { constructor(e) { this.s = e ? this.peek() : "-"; this.p = 1; }
          peek() { return typeof this.p; } }
        console.log(new A(true).s);
        "#,
        true,
    );
}

#[test]
fn a_constructor_store_before_any_observation_is_definite() {
    check_presence(
        r#"
        class A { constructor(e) { this.s = "-"; this.p = 1; }
          peek() { return typeof this.p; } }
        console.log(new A(true).peek());
        "#,
        false,
    );
}

/// SHC6: a field DEFINITION is present from birth on only when no value
/// evaluated before it (an earlier initializer, its own, a parameter default,
/// a parent constructor) can observe the instance. Else it is reserved, its
/// kind holds `undefined`, and the layout is not fixed: `a = this.f()` runs
/// `f` with `p` not yet defined (node reads `undefined`).
fn check_field_presence(src: &str, observed: bool) {
    for pf in [facts(src), units(src)] {
        let t = tmpl(&pf, "A");
        let p = field(t, "p");
        assert_eq!(p.slot, Some(1), "{}", pf.dump_text());
        assert_eq!(p.definite, !observed, "{}", pf.dump_text());
        assert_eq!(p.reserved, observed, "{}", pf.dump_text());
        let undef = p.kind.prims & crate::model::prim::UNDEF != 0;
        assert_eq!(undef, observed, "{}", pf.dump_text());
        assert_eq!(t.fixed_layout, !observed, "{}", pf.dump_text());
    }
}

#[test]
fn a_field_defined_after_an_initializer_calls_a_method_is_reserved() {
    check_field_presence(
        r#"
        class A { a: any = this.f(); p: any = 5;
          f(): any { return this.p; } }
        console.log(String(new A().a));
        "#,
        true,
    );
}

#[test]
fn a_field_defined_after_receiver_free_initializers_is_definite() {
    check_field_presence(
        r#"
        class A { a: number[] = new Array<number>(8).fill(0); p: any = 5;
          f(): any { return this.p; } }
        console.log(new A().f());
        "#,
        false,
    );
}

#[test]
fn a_field_of_a_class_whose_parent_constructor_leaks_is_reserved() {
    let src = r#"
        class B { constructor() { this.init(); } init(): void {} }
        class A extends B { q: any = 4; p: any = 5; seen: any;
          init(): void { this.seen = this.p; } }
        console.log(String(new A().seen));
        "#;
    for pf in [facts(src), units(src)] {
        let t = tmpl(&pf, "A");
        let p = field(t, "p");
        assert!(!p.definite && p.reserved, "{}", pf.dump_text());
        assert!(p.kind.prims & crate::model::prim::UNDEF != 0, "{}", pf.dump_text());
        assert!(!t.fixed_layout, "{}", pf.dump_text());
    }
}

/// A base constructor defines its fields before its parameter defaults run
/// (node: field, then param): a default that calls a method on the instance
/// sees every field, so the fields stay present from birth on.
#[test]
fn a_parameter_default_of_a_base_constructor_runs_after_the_fields() {
    let src = r#"
        class A { a: any = 1; p: any = 5;
          constructor(x: any = this.f()) { console.log(String(x)); }
          f(): any { return this.p; } }
        new A();
        "#;
    for pf in [facts(src), units(src)] {
        let t = tmpl(&pf, "A");
        let p = field(t, "p");
        assert!(p.definite && !p.reserved, "{}", pf.dump_text());
        assert!(p.kind.prims & crate::model::prim::UNDEF == 0, "{}", pf.dump_text());
    }
}
