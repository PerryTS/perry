//! A protocol touch (ToPrimitive, iteration, toJSON ...) runs the receiver's
//! own methods per touched receiver: two receivers of the same method never
//! share one instance, so one receiver's effects stay out of the other.

use super::*;

fn site<'a>(pf: &'a ProgramFacts, key: &str) -> &'a crate::SiteFacts {
    let v: Vec<_> = pf.sites.iter().filter(|s| s.key == key).collect();
    assert_eq!(v.len(), 1, "site {key} in\n{}", pf.dump_text());
    v[0]
}

fn classes(s: &crate::SiteFacts) -> Vec<String> {
    // Class receivers only: a builtin function value or primitive
    // stand-in on a read is the model's convention, not a merge.
    let mut v: Vec<String> = s.receivers.iter().filter(|r| r.contains(':')).map(|r| r.rsplit(':').next().unwrap().to_string()).collect();
    v.sort();
    v
}

#[test]
fn touched_receivers_do_not_share_an_instance() {
    // `${p}` and `${q}` run `P.toString` on each; its write effect must land
    // on the touched receiver only. CWL (layout as the unit of propagation):
    // the touch binds the touched receiver's layout instance, so receivers of
    // two layouts (P1, P2) stay apart. Was: two `P` objects born at two
    // sites (one layout) stayed apart.
    let pf = facts(
        r#"
        class A { a = 1; }
        class B { b = 2; }
        class P {
            v: any; seen: any;
            constructor(x: any) { this.v = x; this.seen = undefined; }
            toString() { this.seen = this.v; return "p"; }
        }
        class P1 extends P { p1 = 1; }
        class P2 extends P { p2 = 2; }
        const p = new P1(new A());
        const q = new P2(new B());
        console.log(`${p}`, `${q}`);
        console.log(p.seen.a, q.seen.b);
        "#,
    );
    assert_eq!(classes(site(&pf, "a")), ["A"], "{}", pf.dump_text());
    assert_eq!(classes(site(&pf, "b")), ["B"], "{}", pf.dump_text());
}

#[test]
fn touched_results_open_no_new_context() {
    // `toString` makes an object that is touched in turn: the births stay in
    // the touched receiver's context, so the solve is finite.
    let pf = facts_with(
        r#"
        class N {
            k: any;
            constructor(k: any) { this.k = k; }
            toString() { const m = new N(this); console.log(`${m}`); return "n"; }
        }
        console.log(`${new N(1)}`);
        "#,
        FactsConfig { max_work: Some(2_000_000), ..FactsConfig::default() },
    );
    assert!(!pf.stats.aborted, "{}", pf.dump_text());
}
