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

/// h4: the parent constructor calls an override that stores `seen` before
/// `x` and `y` are defined. The runtime then appends `x` and `y` after
/// `seen` (enumeration order), so their birth slots stay the hole: they
/// have no slot. `seen`, the last key, fills its slot in place.
#[test]
fn a_key_added_after_a_later_key_has_no_birth_slot() {
    let src = r#"
        class A { constructor() { this.init(); } init(): void {} }
        class B extends A { x: any = 5; y: any = 6; seen: any;
          init(): void { this.seen = this.x; }
          get(): any { return this.x + this.y; } }
        const b = new B();
        console.log(String(b.seen), b.get());
        "#;
    for pf in [facts(src), units(src)] {
        let t = tmpl(&pf, "B");
        for k in ["x", "y"] {
            let f = field(t, k);
            assert!(!f.definite && f.slot.is_none(), "{k}: {}", pf.dump_text());
        }
        assert_eq!(field(t, "seen").slot, Some(2), "{}", pf.dump_text());
        assert!(!t.fixed_layout, "{}", pf.dump_text());
    }
}

/// Same without a parent: `reg(this)` stores `b` before the constructor
/// stores `a`.
#[test]
fn a_constructor_store_after_an_observer_writes_a_later_key_has_no_birth_slot() {
    let src = r#"
        let keep: any = null;
        function reg(o: any): void { keep = o; o.b = 9; }
        class A { declare a: any; declare b: any;
          constructor() { reg(this); this.a = 1; this.b = 2; }
          get(): any { return this.a; } }
        console.log(new A().get(), keep !== null);
        "#;
    for pf in [facts(src), units(src)] {
        let t = tmpl(&pf, "A");
        assert!(field(t, "a").slot.is_none(), "{}", pf.dump_text());
        assert_eq!(field(t, "b").slot, Some(1), "{}", pf.dump_text());
    }
}

/// A defined key under a leaking parent keeps its slot when every later key
/// is written only by construction: definitions run in order.
#[test]
fn a_defined_key_before_keys_only_construction_writes_keeps_its_slot() {
    let src = r#"
        class A { constructor() { this.init(); } init(): void {} }
        class B extends A { x: any = 5; y: any = 6;
          get(): any { return this.x + this.y; } }
        console.log(new B().get());
        "#;
    for pf in [facts(src), units(src)] {
        let t = tmpl(&pf, "B");
        let x = field(t, "x");
        assert!(x.reserved && x.slot == Some(0), "{}", pf.dump_text());
        assert_eq!(field(t, "y").slot, Some(1), "{}", pf.dump_text());
    }
}

/// The control: the child calls the override from its own constructor,
/// after its initializers ran: every key is present from birth with its
/// slot.
#[test]
fn an_override_called_after_the_child_initializers_keeps_the_slots() {
    let src = r#"
        class A { init(): void {} }
        class B extends A { x: any = 5; y: any = 6; seen: any;
          constructor() { super(); this.init(); }
          init(): void { this.seen = this.x; }
          get(): any { return this.x + this.y; } }
        const b = new B();
        console.log(String(b.seen), b.get());
        "#;
    for pf in [facts(src), units(src)] {
        let t = tmpl(&pf, "B");
        for (k, s) in [("x", 0), ("y", 1)] {
            let f = field(t, k);
            assert!(f.definite && f.slot == Some(s), "{k}: {}", pf.dump_text());
        }
    }
}

/// The override the parent constructor calls stores the parent's `p` (7)
/// under `seen`. Per unit the store reaches `B`'s instances through the
/// method's receiver marker: what it stores is the caller's value, which no
/// caller running the method unseen puts beside it, so `seen` holds a
/// number (or ⊤), never `undefined` alone, and is written after
/// construction.
#[test]
fn a_store_through_the_receiver_of_an_unseen_override_reaches_the_instances() {
    let src = r#"
        class A { p: any; constructor() { this.p = 7; this.init(); } init(): void {} }
        class B extends A { x: any = 5; seen: any;
          init(): void { this.seen = this.p; }
          get(): any { return this.x; } }
        const b = new B();
        console.log(String(b.seen), b.get());
        "#;
    for pf in [facts(src), units(src)] {
        let f = field(tmpl(&pf, "B"), "seen");
        let num = f.kind.top || f.kind.prims & crate::model::prim::NUM != 0;
        assert!(num && !f.readonly && f.writers_post > 0, "{}", pf.dump_text());
    }
}
