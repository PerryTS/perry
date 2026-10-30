//! A `new C(...)` is an instance of C only when no constructor on C's chain
//! returns an object: `return expr` with an object value makes that value the
//! result of `new` (and a primitive return is ignored). Per unit, the class
//! is another unit's stand-in: its owner publishes whether its chain can
//! return (`ClassContract::ctor_returns`), and the reader's `new` result is
//! unknown then. Per unit must never prove a read the whole program does not.

use super::*;

fn units(src: &str) -> ProgramFacts {
    let module = perry_parser::parse_typescript(src, "t.ts").expect("parses");
    let hir = perry_hir::lower_module(&module, "t", "t.ts").expect("lowers");
    crate::compute_units(&[("t.ts".to_string(), &hir)], &FactsConfig::default())
}

fn proven(pf: &ProgramFacts, key: &str) -> usize {
    pf.sites
        .iter()
        .filter(|s| s.key == key && matches!(s.verdict, Verdict::T0Mono | Verdict::T0Set))
        .count()
}

/// Per unit proves the read of `x` only if the whole program does too, and
/// exactly when `sound` says the instance is the whole result.
fn check(src: &str, instance: bool) {
    let (pu, pw) = (units(src), facts(src));
    let (u, w) = (proven(&pu, "x"), proven(&pw, "x"));
    assert!(u <= w, "per unit proves {u} > whole {w}\n{}", pu.dump_text());
    assert_eq!(u > 0, instance, "per unit: {u} proven\n{}", pu.dump_text());
}

#[test]
fn a_constructor_returning_an_object_is_not_an_instance() {
    check(
        r#"
        function mk(): any { return { z: 1 }; }
        class B { x = 5; constructor() { return mk(); } }
        const b: any = new B();
        console.log(b.x);
        "#,
        false,
    );
}

#[test]
fn an_inherited_constructor_returning_an_object_is_not_an_instance() {
    check(
        r#"
        function mk(): any { return { z: 1 }; }
        class C { constructor() { return mk(); } }
        class D extends C { x = 5; }
        const d: any = new D();
        console.log(d.x);
        "#,
        false,
    );
}

#[test]
fn a_conditional_object_return_is_not_an_instance() {
    check(
        r#"
        function mk(): any { return { z: 1 }; }
        class B { x = 5; constructor(k: number) { if (k > 1) return mk(); } }
        const b: any = new B(Date.now());
        console.log(b.x);
        "#,
        false,
    );
}

#[test]
fn a_returned_proxy_is_not_an_instance() {
    check(
        r#"
        class B { x = 5; constructor() { return new Proxy({}, {}); } }
        const b: any = new B();
        console.log(b.x);
        "#,
        false,
    );
}

#[test]
fn a_primitive_return_is_still_the_instance() {
    check(
        r#"
        class B { x = 5; constructor() { return 3 as any; } }
        const b = new B();
        console.log(b.x);
        "#,
        true,
    );
}

#[test]
fn returning_this_is_still_the_instance() {
    check(
        r#"
        class B { x = 5; constructor() { return this; } }
        const b = new B();
        console.log(b.x);
        "#,
        true,
    );
}
