//! Design §14 (DECISIONS 28): a closure's context is keyed by what it
//! captures, and a call site expands its own callee's body only (one-level
//! composition). Each test names the arm (secret-tests design §14.5) that
//! must make it fail.

use super::*;

const CLASSES: &str = r#"
    class A { x = 1; a = 1; }
    class B { y = 2; b = 2; }
"#;

fn sites<'a>(pf: &'a ProgramFacts, key: &str) -> Vec<&'a crate::SiteFacts> {
    let v: Vec<_> = pf.sites.iter().filter(|s| s.key == key).collect();
    assert!(!v.is_empty(), "site {key} in\n{}", pf.dump_text());
    v
}

/// Every read of `key` sees exactly `n` receivers and no ⊤.
fn exact(pf: &ProgramFacts, key: &str, n: usize) {
    for s in sites(pf, key) {
        assert!(
            !s.top && s.receivers.len() == n,
            "{key}: {:?} top={}\n{}",
            s.receivers,
            s.top,
            pf.dump_text()
        );
    }
}

/// Every read of `key` sees `class` among its receivers (or ⊤).
fn sees(pf: &ProgramFacts, key: &str, class: &str) {
    for s in sites(pf, key) {
        let hit = s
            .receivers
            .iter()
            .any(|r| r.ends_with(&format!(":{class}")));
        assert!(
            s.top || hit,
            "{key}: {:?} lacks {class}\n{}",
            s.receivers,
            pf.dump_text()
        );
    }
}

/// Arm A1 (`|| is_arrow`): the arrow captures its creator's `this`, so each
/// of `mk`'s two site instances makes its own context.
#[test]
fn an_arrow_that_never_reads_this_has_no_context() {
    let pf = facts(&format!(
        "{CLASSES}
        function mk() {{ return (n: any) => n.x; }}
        const f1 = mk(); const f2 = mk();
        console.log(f1(new A()), f2(new A()));"
    ));
    assert_eq!(pf.stats.contexts, 0, "{}", pf.dump_text());
    exact(&pf, "x", 1);
}

/// Arm A2 (no `this` capture at all): `this.o` sees nothing.
#[test]
fn an_arrow_that_reads_this_is_keyed_by_the_receiver_layout() {
    let pf = facts(&format!(
        "{CLASSES}
        class H {{ o: any; constructor(o: any) {{ this.o = o; }} run() {{ return () => this.o; }} }}
        class H1 extends H {{ h1 = 1; }}
        class H2 extends H {{ h2 = 2; }}
        const a = new H1(new A()).run()(); const b = new H2(new B()).run()();
        console.log(a.x, b.y);"
    ));
    // One context per receiver layout of `run`: the shape is the context.
    assert_eq!(pf.stats.contexts, 2, "{}", pf.dump_text());
    sees(&pf, "o", "H1");
    sees(&pf, "o", "H2");
    exact(&pf, "x", 1);
    exact(&pf, "y", 1);
}

/// Arm A3 (outer-only keyed by the creator's lexical context): `g`'s two
/// program sites are two lexical contexts of one closure context, and the
/// inner arrow gets a context per site although both hold the cell `x`.
#[test]
fn outer_only_captures_share_the_creator_context() {
    let pf = facts(&format!(
        "{CLASSES}
        function f(x: any) {{ return () => () => x; }}
        const g = f(new A());
        const h1 = g(); const h2 = g();
        console.log(h1().x, h2().x);"
    ));
    // The outer arrow captures `x`, `f`'s parameter: one context for `f`'s
    // one site. The inner arrow captures only the outer arrow's outer term
    // `x`: one context, not one per site of `g`.
    assert_eq!(pf.stats.contexts, 2, "{}", pf.dump_text());
    // The captured cell still carries what `f` was called with.
    exact(&pf, "x", 1);
    sees(&pf, "x", "A");
}

/// Arm A4 (own captures keyed by the creator's closure context): the two
/// `mk` site instances share one context and `x`/`y` see both classes.
#[test]
fn own_captures_keep_one_context_per_creator_context() {
    let pf = facts(&format!(
        "{CLASSES}
        function mk(o: any) {{ return () => o; }}
        const fa = mk(new A()); const fb = mk(new B());
        console.log(fa().x, fb().y);"
    ));
    assert_eq!(pf.stats.contexts, 2, "{}", pf.dump_text());
    exact(&pf, "x", 1);
    exact(&pf, "y", 1);
}

/// Arm B1 (transitive composition): `h` is copied into `f` too, and no
/// instance of `h` is made.
#[test]
fn one_level_composition_copies_the_callee_body_only() {
    let pf = facts(&format!(
        "{CLASSES}
        function h(o: any) {{ return o; }}
        function g(o: any) {{ return h(o); }}
        function f(o: any) {{ return g(o); }}
        const a = f(new A()); const b = f(new B());
        console.log(a, b);"
    ));
    // `f`'s two site instances hold `g`'s body; the `h(o)` call copied with
    // it binds `h`'s one receiver-less instance: 2 + 1.
    assert_eq!(pf.stats.instances, 3, "{}", pf.dump_text());
    // Expanded: g into f, h into g (g's own summary keeps its callee).
    assert_eq!(pf.stats.composed, 2, "{}", pf.dump_text());
}

/// Arm B2 (no composition): `finish` binds one instance for both `make`
/// instances and both results hold A and B.
#[test]
fn the_direct_callee_result_stays_per_site() {
    let pf = facts(&format!(
        "{CLASSES}
        function finish(n: any, pos: number) {{ n.pos = pos; return n; }}
        function make(o: any) {{ return finish(o, 1); }}
        const a = make(new A()); const b = make(new B());
        console.log(a.x, b.y);"
    ));
    exact(&pf, "x", 1);
    exact(&pf, "y", 1);
}

/// Arm B3 (the copied calls are not bound): `h`'s store never runs, which
/// is unsound. Soundness guard of §14.3.
#[test]
fn a_deeper_callee_store_reaches_every_caller() {
    let pf = facts(&format!(
        "{CLASSES}
        class H1 {{ v: any = 0; }}
        class H2 {{ v: any = 0; }}
        function h(o: any, v: any) {{ o.v = v; }}
        function g(o: any, v: any) {{ h(o, v); }}
        function f1(o: any) {{ g(o, new A()); return o; }}
        function f2(o: any) {{ g(o, new B()); return o; }}
        const h1 = f1(new H1()); const h2 = f2(new H2());
        console.log(h1.v.a, h2.v.b);"
    ));
    sees(&pf, "a", "A");
    sees(&pf, "b", "B");
}

/// Pure parameter returns preserve the caller's value two calls deep,
/// while the helper's body still has its layout-keyed observation.
#[test]
fn depth_two_context_is_per_layout() {
    let pf = facts(&format!(
        "{CLASSES}
        function id(o: any) {{ return o; }}
        function g(o: any) {{ return id(o); }}
        function f(o: any) {{ return g(o); }}
        const a = f(new A()); const b = f(new B());
        console.log(a.x, b.y);"
    ));
    // Returning an unchanged parameter needs no additional context.
    exact(&pf, "x", 1);
    exact(&pf, "y", 1);
    sees(&pf, "x", "A");
    sees(&pf, "y", "B");
}
