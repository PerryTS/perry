//! Layout as the unit of propagation (CWL, secret-tests
//! CW_LAYOUT_CONTEXT_2026-10-20.md): a summary instance is keyed by the
//! receiver's layout, never by the call site that reaches it; the birth of
//! an object runs per birth site; a receiver node binds per token.

use super::*;

const CLASSES: &str = r#"
    class A { x = 1; a = 1; }
    class B { y = 2; b = 2; }
"#;

fn exact(pf: &ProgramFacts, key: &str, n: usize) {
    let v: Vec<_> = pf.sites.iter().filter(|s| s.key == key).collect();
    assert!(!v.is_empty(), "site {key} in\n{}", pf.dump_text());
    for s in v {
        assert!(!s.top && s.receivers.len() == n, "{key}: {:?} top={}\n{}", s.receivers, s.top, pf.dump_text());
    }
}

#[test]
fn method_instances_are_per_layout_not_per_site() {
    // Thirty call sites of one method on one layout: one instance of `get`
    // (and one birth run of the field initializer), not one per site.
    let mut src = String::from("class P { v = 1; get() { return this.v; } }\nconst p = new P();\n");
    for _ in 0..30 {
        src.push_str("console.log(p.get());\n");
    }
    let pf = facts(&src);
    assert!(pf.stats.instances <= 3, "{}", pf.dump_text());
    let site = pf.sites.iter().find(|s| s.key == "v" && !s.is_call).expect("site");
    assert_eq!(site.verdict, Verdict::T0Mono, "{}", pf.dump_text());
}

#[test]
fn a_receiver_node_binds_per_layout() {
    // `get.call(h)` hands the receiver over as a node: each token binds the
    // instance of its own layout, so the two results stay apart.
    let pf = facts(&format!(
        "{CLASSES}
        class H1 {{ v = new A(); }}
        class H2 {{ v = new B(); }}
        const get = function (this: any) {{ return this.v; }};
        const ra = get.call(new H1()); const rb = get.call(new H2());
        console.log(ra.a, rb.b);"
    ));
    exact(&pf, "a", 1);
    exact(&pf, "b", 1);
}

#[test]
fn a_birth_runs_per_birth_site() {
    // Two literals of one shape are one layout; the constructor that fills
    // each one's fields runs per birth site, so the fields stay apart.
    let pf = facts(&format!(
        "{CLASSES}
        const p = {{ k: new A() }}; const q = {{ k: new B() }};
        console.log(p.k.a, q.k.b);"
    ));
    exact(&pf, "a", 1);
    exact(&pf, "b", 1);
}
