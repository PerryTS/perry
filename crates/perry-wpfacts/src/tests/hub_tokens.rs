//! The runtime's shared stand-in tokens are not merge points: a container
//! of primitives is one object per producing site. A builtin function value
//! keeps a store under a key no native declares in the one field every
//! native function value shares under that key (rule R1).

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
fn builtin_function_values_share_one_field_per_key() {
    // A computed member is anonymous: its field write can reach any native.
    let pf = facts(
        r#"
        class A { a = 1; }
        const f: any = [][process.argv[2] ?? "map"];
        f.tag = new A();
        const g: any = "".slice;
        const t = g.tag;
        console.log(t.a);
        "#,
    );
    let s = site(&pf, "a");
    assert_eq!(classes(s), ["A"], "{}", pf.dump_text());
    assert!(!s.top, "{}", pf.dump_text());
}

#[test]
fn identified_array_and_string_functions_have_distinct_fields() {
    let pf = facts(
        r#"
        class A { a = 1; }
        const f: any = [].map;
        f.tag = new A();
        const g: any = "".slice;
        console.log(g.tag.a);
    "#,
    );
    assert!(classes(site(&pf, "a")).is_empty(), "{}", pf.dump_text());
}

#[test]
fn an_anonymous_native_read_sees_an_identified_native_write() {
    let pf = facts(
        r#"
        class A { a = 1; }
        const f: any = [].map;
        f.tag = new A();
        const g: any = [][process.argv[2] ?? "map"];
        console.log(g.tag.a);
    "#,
    );
    assert_eq!(classes(site(&pf, "a")), ["A"], "{}", pf.dump_text());
}
