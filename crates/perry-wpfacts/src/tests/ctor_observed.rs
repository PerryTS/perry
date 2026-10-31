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
