//! A callback a builtin method runs (forEach, map, sort ...) is keyed on the
//! method call's site even when the callback function is already known when
//! the call is attached: a solver-made call carries its site from creation.

use super::*;

fn site<'a>(pf: &'a ProgramFacts, key: &str) -> &'a crate::SiteFacts {
    let v: Vec<_> = pf.sites.iter().filter(|s| s.key == key).collect();
    assert_eq!(v.len(), 1, "site {key} in\n{}", pf.dump_text());
    v[0]
}

fn classes(s: &crate::SiteFacts) -> Vec<String> {
    let mut v: Vec<String> = s.receivers.iter().filter(|r| r.contains(':')).map(|r| r.rsplit(':').next().unwrap().to_string()).collect();
    v.sort();
    v
}

#[test]
fn early_bound_callbacks_key_on_their_site() {
    // One function value passed to `map` at two sites: each site's result
    // holds what that site's callback returned, not the union.
    let pf = facts(
        r#"
        class A { a = 1; }
        class B { b = 2; }
        const f = function (x: any) { return x; };
        const ra = [new A()].map(f);
        const rb = [new B()].map(f);
        console.log(ra[0].a, rb[0].b);
        "#,
    );
    assert_eq!(classes(site(&pf, "a")), ["A"], "{}", pf.dump_text());
    assert_eq!(classes(site(&pf, "b")), ["B"], "{}", pf.dump_text());
}

#[test]
fn call_and_apply_key_on_their_site() {
    // `f.call` / `f.apply` make the target's call while the builtin runs;
    // it is bound at once, so it must carry the site from its creation.
    let pf = facts(
        r#"
        class A { a = 1; }
        class B { b = 2; }
        const id = function (x: any) { return x; };
        const ra = id.call(null, new A());
        const rb = id.call(null, new B());
        const pa = id.apply(null, [new A()]);
        const pb = id.apply(null, [new B()]);
        console.log(ra.a, rb.b, pa.a, pb.b);
        "#,
    );
    // CWL (layout as the unit of propagation): the target call `call` and
    // `apply` make binds the target's one instance (no `this` term), so all
    // four results hold both classes; each still reaches its site. Was:
    // each site's result held its own class only.
    let d = pf.dump_text();
    for k in ["a", "b"] {
        let v: Vec<_> = pf.sites.iter().filter(|s| s.key == k).collect();
        assert_eq!(v.len(), 2, "{d}");
        for s in v {
            assert_eq!(classes(s), ["A", "B"], "{d}");
        }
    }
}
