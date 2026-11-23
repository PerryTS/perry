//! CTOR-RESULT (ECMA-262 10.2.2 [[Construct]] steps 10-12): `new F(...)`
//! yields F's body result when it is an Object; only a path whose result is
//! no Object (for a derived class: `undefined`) yields the this-object. A
//! constructor whose every path returns an object never lets its
//! this-object reach the `new` result.

use super::*;

/// The receivers of every site with `key`, joined.
fn recv(pf: &ProgramFacts, key: &str) -> Vec<String> {
    let mut v: Vec<String> = pf
        .sites
        .iter()
        .filter(|s| s.key == key)
        .flat_map(|s| s.receivers.clone())
        .collect();
    v.sort();
    v.dedup();
    v
}

fn proven(pf: &ProgramFacts, key: &str) -> usize {
    pf.sites
        .iter()
        .filter(|s| s.key == key && matches!(s.verdict, Verdict::T0Mono | Verdict::T0Set))
        .count()
}

/// Whether the receivers of `key` hold the this-object of `new <name>`.
fn holds_this(pf: &ProgramFacts, key: &str, name: &str) -> bool {
    let tag = format!("new {name}");
    recv(pf, key)
        .iter()
        .any(|r| r.contains(&tag) || r.ends_with(&format!(":{name}")))
}

/// RED/GREEN: the three.js `WebGLPrograms` shape. `p` is the returned
/// literal only, so `p.m()` resolves to its method.
#[test]
fn a_function_constructor_returning_a_literal_is_not_its_this() {
    let pf = facts(
        r#"
        function F(this: any): any { this.k = 1; return { m() { return 1; } }; }
        const p: any = new (F as any)();
        console.log(p.m());
        "#,
    );
    assert!(
        !holds_this(&pf, "m", "F"),
        "{:?}\n{}",
        recv(&pf, "m"),
        pf.dump_text()
    );
    assert!(
        proven(&pf, "m") > 0,
        "{:?}\n{}",
        recv(&pf, "m"),
        pf.dump_text()
    );
}

/// Guard: an object on some paths only; the other path completes, so `new`
/// yields the this-object there.
#[test]
fn a_function_constructor_returning_an_object_sometimes_keeps_its_this() {
    let pf = facts(
        r#"
        function F(this: any, k: number): any { this.m = function () { return 2; }; if (k > 1) { return { m() { return 1; } }; } }
        const p: any = new (F as any)(Date.now());
        console.log(p.m());
        "#,
    );
    assert!(
        holds_this(&pf, "m", "F"),
        "{:?}\n{}",
        recv(&pf, "m"),
        pf.dump_text()
    );
}

/// Guard: a returned value that may be a primitive keeps the this-object.
#[test]
fn a_function_constructor_returning_object_or_primitive_keeps_its_this() {
    let pf = facts(
        r#"
        function F(this: any, k: number): any { this.m = function () { return 2; }; const o: any = k > 1 ? { m() { return 1; } } : 3; return o; }
        const p: any = new (F as any)(Date.now());
        console.log(p.m());
        "#,
    );
    assert!(
        holds_this(&pf, "m", "F"),
        "{:?}\n{}",
        recv(&pf, "m"),
        pf.dump_text()
    );
}

/// Guard: a primitive return is ignored by `new`: the this-object.
#[test]
fn a_function_constructor_returning_a_primitive_is_its_this() {
    let pf = facts(
        r#"
        function F(this: any): any { this.m = function () { return 2; }; return 3; }
        const p: any = new (F as any)();
        console.log(p.m());
        "#,
    );
    assert!(
        holds_this(&pf, "m", "F"),
        "{:?}\n{}",
        recv(&pf, "m"),
        pf.dump_text()
    );
    assert!(
        proven(&pf, "m") > 0,
        "{:?}\n{}",
        recv(&pf, "m"),
        pf.dump_text()
    );
}

/// Guard: `return;` yields the this-object.
#[test]
fn a_function_constructor_with_an_empty_return_keeps_its_this() {
    let pf = facts(
        r#"
        function F(this: any, k: number): any { this.m = function () { return 2; }; if (k > 1) { return; } return { m() { return 1; } }; }
        const p: any = new (F as any)(Date.now());
        console.log(p.m());
        "#,
    );
    assert!(
        holds_this(&pf, "m", "F"),
        "{:?}\n{}",
        recv(&pf, "m"),
        pf.dump_text()
    );
}

/// A class constructor returning an object: the instance is not the result.
#[test]
fn a_class_constructor_returning_a_literal_is_not_its_this() {
    let pf = facts(
        r#"
        class B { m(): number { return 2; } constructor() { return { m() { return 1; } } as any; } }
        const p: any = new B();
        console.log(p.m());
        "#,
    );
    assert!(
        !holds_this(&pf, "m", "B"),
        "{:?}\n{}",
        recv(&pf, "m"),
        pf.dump_text()
    );
    assert!(
        proven(&pf, "m") > 0,
        "{:?}\n{}",
        recv(&pf, "m"),
        pf.dump_text()
    );
}

/// Guard: a class constructor that may complete keeps the instance.
#[test]
fn a_class_constructor_returning_an_object_sometimes_keeps_its_this() {
    let pf = facts(
        r#"
        class B { m(): number { return 2; } constructor(k: number) { if (k > 1) { return { m() { return 1; } } as any; } } }
        const p: any = new B(Date.now());
        console.log(p.m());
        "#,
    );
    assert!(
        holds_this(&pf, "m", "B"),
        "{:?}\n{}",
        recv(&pf, "m"),
        pf.dump_text()
    );
}

/// A derived class constructor returning an object: not the instance.
#[test]
fn a_derived_class_constructor_returning_a_literal_is_not_its_this() {
    let pf = facts(
        r#"
        class A { m(): number { return 3; } }
        class B extends A { constructor() { super(); return { m() { return 1; } } as any; } }
        const p: any = new B();
        console.log(p.m());
        "#,
    );
    assert!(
        !holds_this(&pf, "m", "B"),
        "{:?}\n{}",
        recv(&pf, "m"),
        pf.dump_text()
    );
}

/// Guard: a derived class constructor that completes keeps the instance.
#[test]
fn a_derived_class_constructor_that_completes_keeps_its_this() {
    let pf = facts(
        r#"
        class A { m(): number { return 3; } }
        class B extends A { constructor(k: number) { super(); if (k > 1) { return { m() { return 1; } } as any; } } }
        const p: any = new B(Date.now());
        console.log(p.m());
        "#,
    );
    assert!(
        holds_this(&pf, "m", "B"),
        "{:?}\n{}",
        recv(&pf, "m"),
        pf.dump_text()
    );
}
