//! What a runtime helper does with a function value: one that only names it
//! keeps it, one whose protocol really calls user code still runs it.

use super::*;

fn site<'a>(pf: &'a ProgramFacts, key: &str) -> &'a crate::SiteFacts {
    let v: Vec<_> = pf.sites.iter().filter(|s| s.key == key).collect();
    assert_eq!(v.len(), 1, "site {key} in\n{}", pf.dump_text());
    v[0]
}

#[test]
fn a_function_named_by_a_computed_key_keeps_its_callers_values() {
    // `{ [k]: f }` names `f` after the key (SetFunctionName); the runtime
    // calls nothing, so `f` is called only by `f(new A())` and its parameter
    // holds the A instance. Was: the helper escaped `f`, `x` was ⊤.
    let pf = facts(
        r#"
        class A { a = 1; }
        function f(x: any) { return x.a; }
        const k = String.fromCharCode(107);
        const t: any = { [k]: f };
        console.log(f(new A()), typeof t);
        "#,
    );
    let s = site(&pf, "a");
    assert!(!s.top, "{}", pf.dump_text());
    assert!(
        matches!(s.verdict, crate::Verdict::T0Mono),
        "{:?}\n{}",
        s.verdict,
        pf.dump_text()
    );
}

#[test]
fn a_computed_key_object_still_runs_its_to_string() {
    // `{ [o]: 1 }`: ToPropertyKey(o) calls `o.toString()` with `this` = o,
    // so `this.v.b` reads the B instance (a helper that really calls user
    // code keeps its protocol call).
    let pf = facts(
        r#"
        class B { b = 2; }
        class O {
            v: any;
            constructor(x: any) { this.v = x; }
            toString() { return "k" + this.v.b; }
        }
        const o: any = new O(new B());
        const t: any = { [o]: 1 };
        console.log(Object.keys(t).length);
        "#,
    );
    let s = site(&pf, "b");
    assert!(
        s.receivers.iter().any(|r| r.ends_with(":B")),
        "{:?}\n{}",
        s.receivers,
        pf.dump_text()
    );
}
