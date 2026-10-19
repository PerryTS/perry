//! A method call binds each target's instance `this` with exactly the
//! receivers whose lookup found that target: a call site over several
//! classes never merges them into every target's instance.

use super::*;

fn sites<'a>(pf: &'a ProgramFacts, key: &str) -> Vec<&'a crate::SiteFacts> {
    let v: Vec<_> = pf.sites.iter().filter(|s| s.key == key).collect();
    assert!(!v.is_empty(), "site {key} in\n{}", pf.dump_text());
    v
}

/// Every read of `key` sees exactly the receivers `want` (class names).
fn only(pf: &ProgramFacts, key: &str, want: &[&str]) {
    for s in sites(pf, key) {
        let mut got: Vec<String> = s.receivers.iter().map(|r| r.rsplit(':').next().unwrap().to_string()).collect();
        got.sort();
        assert!(!s.top && got == want, "{key}: {:?} top={} want {want:?}\n{}", s.receivers, s.top, pf.dump_text());
    }
}

// `P.set` writes its argument into `this.v`; `Q.set` writes nothing. An
// instance of `P.set` whose `this` held every receiver of the call would
// write into `Q`'s `v` as well.
const CLASSES: &str = r#"
    class A { a = 1; }
    class B { b = 2; }
    class P { v: any = null; set(x: any) { this.v = x; } }
    class Q { v: any = null; set(x: any) { } }
    function g(o: any, x: any) { o.set(x); }
"#;

#[test]
fn one_call_site_over_two_classes_keeps_each_target_this_apart() {
    let pf = facts(&format!(
        "{CLASSES}
        const p = new P(); const q = new Q(); q.v = new B();
        const os: any[] = [p, q];
        for (const o of os) g(o, new A());
        console.log(q.v.b, p.v.a);"
    ));
    only(&pf, "b", &["B"]);
    only(&pf, "a", &["A"]);
}

#[test]
fn inherited_method_sees_every_receiver_that_finds_it() {
    let pf = facts(&format!(
        "{CLASSES}
        class C1 extends P {{ c1 = 1; }}
        class C2 extends P {{ c2 = 2; }}
        const c1 = new C1(); const c2 = new C2(); const q = new Q(); q.v = new B();
        const os: any[] = [c1, c2, q];
        for (const o of os) g(o, new A());
        console.log(c1.v.a, c2.v.a, q.v.b);"
    ));
    only(&pf, "a", &["A"]);
    only(&pf, "b", &["B"]);
}

#[test]
fn receiver_added_after_the_first_binding_still_reaches_the_instance() {
    let pf = facts(&format!(
        "{CLASSES}
        const p1 = new P(); const p2 = new P(); const q = new Q(); q.v = new B();
        const os: any[] = [p1];
        g(os[0], new A());
        os.push(p2, q);
        for (const o of os) g(o, new A());
        console.log(p1.v.a, p2.v.a, q.v.b);"
    ));
    only(&pf, "a", &["A"]);
    only(&pf, "b", &["B"]);
}
