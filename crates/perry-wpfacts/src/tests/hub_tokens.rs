//! The runtime's shared stand-in tokens are not merge points: a container
//! of primitives is one object per producing site, and a builtin function
//! value has no field storage of its own.

use super::*;

fn site<'a>(pf: &'a ProgramFacts, key: &str) -> &'a crate::SiteFacts {
    let v: Vec<_> = pf.sites.iter().filter(|s| s.key == key).collect();
    assert_eq!(v.len(), 1, "site {key} in\n{}", pf.dump_text());
    v[0]
}

fn classes(s: &crate::SiteFacts) -> Vec<String> {
    // Class receivers only: a builtin function value or primitive
    // stand-in on a read is the model's convention, not a merge.
    let mut v: Vec<String> = s
        .receivers
        .iter()
        .filter(|r| r.contains(':'))
        .map(|r| r.rsplit(':').next().unwrap().to_string())
        .collect();
    v.sort();
    v
}

#[test]
fn primitive_containers_are_per_site() {
    // Two `split` results: a field stored on one is not read from the other.
    let pf = facts(
        r#"
        class A { a = 1; }
        class B { b = 2; }
        const p: any = "x,y".split(",");
        const q: any = "u,v".split(",");
        p.tag = new A();
        q.tag = new B();
        console.log(p.tag.a, q.tag.b);
        "#,
    );
    assert_eq!(classes(site(&pf, "a")), ["A"], "{}", pf.dump_text());
    assert_eq!(classes(site(&pf, "b")), ["B"], "{}", pf.dump_text());
}

#[test]
fn builtin_function_values_hold_nothing() {
    // A store onto one builtin function value is not read back from another
    // (there is no shared storage); the read is ⊤, not the stored object.
    let pf = facts(
        r#"
        class A { a = 1; }
        const f: any = [].map;
        f.tag = new A();
        const g: any = "".slice;
        const t = g.tag;
        console.log(t.a);
        "#,
    );
    let s = site(&pf, "a");
    assert!(
        classes(s).is_empty(),
        "{:?}\n{}",
        s.receivers,
        pf.dump_text()
    );
    assert!(s.top, "{}", pf.dump_text());
}
