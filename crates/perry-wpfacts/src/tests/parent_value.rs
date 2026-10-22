//! A class whose `extends` is a value, not a class name (CommonJS
//! `const {Base} = require(..)`, `const P = pick(); class M extends P`): the
//! parent is what that value holds after the solve (DECISIONS 46-47).

use super::*;

/// Destructuring passes the source through `requireObjectCoercible`; its
/// second argument is the pattern offset for the error location, never the
/// result, so the bound value keeps its one class.
#[test]
fn destructured_value_keeps_its_class() {
    let pf = facts(
        r#"
        class A { x: number; constructor() { this.x = 1; } }
        const { k } = { k: new A() };
        console.log(k.x);
    "#,
    );
    assert_eq!(
        site(&pf, "x").verdict,
        Verdict::T0Mono,
        "{}",
        pf.dump_text()
    );
}

/// The parent's birth fields come first: a value parent's keys are not
/// known at generation time, so the subclass layout is settled after the
/// solve (DECISIONS 47).
#[test]
fn value_parent_fields_precede_own_fields() {
    let pf = facts(
        r#"
        class A { a: number; constructor() { this.a = 1; } }
        function pick() { return A; }
        const P = pick();
        class Main extends P { b: number; constructor() { super(); this.b = 2; } get() { return this.a + this.b; } }
        const m = new Main(); let t = 0; for (let i = 0; i < 3; i++) t += m.get();
        console.log(t);
    "#,
    );
    let t = tmpl(&pf, "Main");
    assert!(t.hard.is_none(), "{}", pf.dump_text());
    assert_eq!(
        t.birth_keys,
        vec!["a".to_string(), "b".to_string()],
        "{}",
        pf.dump_text()
    );
    assert_eq!(field(t, "b").slot, Some(1), "{}", pf.dump_text());
    // The parent constructor's store runs inside `new Main()`: a
    // constructor write, not a later one.
    let a = field(t, "a");
    assert!(
        a.readonly && a.writers_post == 0,
        "{a:?}\n{}",
        pf.dump_text()
    );
}

/// A parent value that may be one of two classes has no one layout.
#[test]
fn value_parent_of_two_classes_is_hard() {
    let pf = facts(
        r#"
        class A { a = 1; } class B { z = 2; }
        function pick(c: number) { return c ? A : B; }
        const P = pick(Date.now() & 1);
        class Main extends P { b = 3; }
        console.log(new Main().b);
    "#,
    );
    let h = tmpl(&pf, "Main").hard.clone().unwrap_or_default();
    assert!(h.starts_with("E11"), "{h}\n{}", pf.dump_text());
}

/// A parent constructor that leaks `this` lets other code add keys before
/// the subclass constructor runs: the own key is not definite.
#[test]
fn leaking_value_parent_ctor_keeps_own_keys_indefinite() {
    let pf = facts(
        r#"
        let seen: any = null;
        class A { a: number; constructor() { this.a = 1; seen = this; } }
        function pick() { return A; }
        const P = pick();
        class Main extends P { b: number; constructor() { super(); this.b = 2; } }
        const m = new Main(); console.log(m.b, seen);
    "#,
    );
    let t = tmpl(&pf, "Main");
    assert!(
        !t.fixed_layout || !field(t, "b").definite,
        "{}",
        pf.dump_text()
    );
}

/// A super chain through two value parents runs both constructors: the
/// birth layout is the grandparent first, then the parent.
#[test]
fn value_parent_super_chain_orders_birth_keys() {
    let pf = facts(
        r#"
        class R { k: number; constructor() { this.k = 1; } }
        function pr() { return R; }
        const PR = pr();
        class S extends PR { j: number; constructor() { super(); this.j = 2; } }
        function ps() { return S; }
        const PS = ps();
        class T extends PS { constructor() { super(); } }
        const t = new T(); console.log(t.k + t.j);
    "#,
    );
    assert_eq!(
        tmpl(&pf, "S").birth_keys,
        vec!["k".to_string(), "j".to_string()],
        "{}",
        pf.dump_text()
    );
}
