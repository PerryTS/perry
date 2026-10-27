//! Function summaries: each direct call gets the callee's effect over its
//! own arguments (callers never merge through the shared parameters), the
//! shared body observes every call, and unknown code stays ⊤.

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

#[test]
fn identity_callers_keep_their_own_class() {
    let pf = facts(&format!(
        "{CLASSES}
        function id(o: any) {{ return o; }}
        const a = id(new A()); const b = id(new B());
        console.log(a.x, b.y);"
    ));
    exact(&pf, "x", 1);
    exact(&pf, "y", 1);
}

#[test]
fn field_read_through_a_helper() {
    let pf = facts(&format!(
        "{CLASSES}
        class H1 {{ f = new A(); }}
        class H2 {{ f = new B(); }}
        function get(o: any) {{ return o.f; }}
        const a = get(new H1()); const b = get(new H2());
        console.log(a.a, b.b);"
    ));
    exact(&pf, "a", 1);
    exact(&pf, "b", 1);
    // The helper's own site observes both callers.
    exact(&pf, "f", 2);
}

#[test]
fn pipe_through_two_helpers() {
    let pf = facts(&format!(
        "{CLASSES}
        function h(x: any) {{ return x; }}
        function g(x: any) {{ return x; }}
        function f(x: any) {{ return g(h(x)); }}
        const a = f(new A()); const b = f(new B());
        console.log(a.x, b.y);"
    ));
    exact(&pf, "x", 1);
    exact(&pf, "y", 1);
}

#[test]
fn store_through_a_helper_reaches_only_its_receiver() {
    let pf = facts(&format!(
        "{CLASSES}
        class H1 {{ v: any = 0; }}
        class H2 {{ v: any = 0; }}
        function setv(o: any, v: any) {{ o.v = v; return o; }}
        const h1 = setv(new H1(), new A()); const h2 = setv(new H2(), new B());
        console.log(h1.v.a, h2.v.b);"
    ));
    exact(&pf, "a", 1);
    exact(&pf, "b", 1);
}

#[test]
fn callback_result_per_call() {
    let pf = facts(&format!(
        "{CLASSES}
        function id(o: any) {{ return o; }}
        function apply(cb: any, v: any) {{ return cb(v); }}
        const a = apply(id, new A()); const b = apply(id, new B());
        console.log(a.x, b.y);"
    ));
    // `id` is a pure parameter-to-return copy: each callback returns
    // this call's argument, although its observation instance is shared.
    exact(&pf, "x", 1);
    exact(&pf, "y", 1);
    sees(&pf, "x", "A");
    sees(&pf, "y", "B");
}

/// A callee that itself calls: the calls instances make share one instance
/// per template call (sound, merged).
#[test]
fn nested_non_leaf_callee_stays_sound() {
    let pf = facts(&format!(
        "{CLASSES}
        function id(o: any) {{ return o; }}
        function wrap(o: any) {{ return id(o); }}
        function apply(cb: any, v: any) {{ return cb(v); }}
        const a = apply(wrap, new A()); const b = apply(wrap, new B());
        console.log(a.x, b.y);"
    ));
    sees(&pf, "x", "A");
    sees(&pf, "y", "B");
}

#[test]
fn recursion_stays_sound() {
    let pf = facts(&format!(
        "{CLASSES}
        function walk(o: any, n: number): any {{ return n > 0 ? walk(o, n - 1) : o; }}
        function even(o: any, n: number): any {{ return n > 0 ? odd(o, n - 1) : o; }}
        function odd(o: any, n: number): any {{ return n > 0 ? even(o, n - 1) : o; }}
        const a = walk(new A(), 3); const b = walk(new B(), 3);
        const c = even(new A(), 3);
        console.log(a.x, b.y, c.a);"
    ));
    for k in ["x", "y", "a"] {
        for s in sites(&pf, k) {
            assert!(
                !s.receivers.is_empty() || s.top,
                "{k}: lost receivers\n{}",
                pf.dump_text()
            );
        }
    }
    let x = &sites(&pf, "x")[0];
    assert!(
        x.top || x.receivers.iter().any(|r| r.ends_with(":A")),
        "{:?}",
        x.receivers
    );
}

/// Unknown code handed a summarized helper's argument: the argument
/// escapes. Interpreted code (`new Function`) gets the same verdict as
/// without the helper.
#[test]
fn sabotage_unknown_code_writes_a_helper_argument() {
    let pf = facts(&format!(
        "{CLASSES}
        declare const ext: any;
        function pass(o: any, f: any) {{ f(o); return o; }}
        const a = pass(new A(), ext);
        console.log(a.x);"
    ));
    assert!(tmpl(&pf, "A").escape.is_some(), "{}", pf.dump_text());
    assert_eq!(tmpl(&pf, "B").escape, None);
    let direct = facts_with(
        &format!(
            "{CLASSES} const a = new A(); (new Function('o', 'o.x = 1'))(a); console.log(a.x);"
        ),
        interp_cfg(),
    );
    let helper = facts_with(
        &format!(
            "{CLASSES}
            function pass(o: any, f: any) {{ f(o); return o; }}
            const a = pass(new A(), new Function('o', 'o.x = 1'));
            console.log(a.x);"
        ),
        interp_cfg(),
    );
    assert_eq!(
        tmpl(&direct, "A").escape.is_some(),
        tmpl(&helper, "A").escape.is_some()
    );
    assert_eq!(direct.interp_guard.is_some(), helper.interp_guard.is_some());
}

/// A callback a helper hands to unknown code runs with unknown arguments.
#[test]
fn sabotage_callback_passed_to_unknown_code() {
    let pf = facts(&format!(
        "{CLASSES}
        declare const ext: any;
        function cb(o: any) {{ return o.x; }}
        function give(f: any) {{ ext(f); }}
        give(cb); cb(new A());"
    ));
    let x = &sites(&pf, "x")[0];
    assert!(x.top, "{:?}\n{}", x.receivers, pf.dump_text());
}

/// A user write to a builtin prototype through a helper's argument.
#[test]
fn sabotage_user_write_to_a_builtin_through_a_helper() {
    let direct = facts("(Object.prototype as any).bind = function () {}; console.log(1);");
    let pf = facts(
        "function patch(p: any, f: any) { p.bind = f; }
         patch(Object.prototype, function () {}); console.log(1);",
    );
    assert_eq!(
        direct.fnproto_bind_written.is_some(),
        pf.fnproto_bind_written.is_some()
    );
    assert!(pf.fnproto_bind_written.is_some(), "{}", pf.dump_text());
}

#[test]
fn composed_pipe_makes_no_callee_instances() {
    let pf = facts(&format!(
        "{CLASSES}
        class H1 {{ f = new A(); }}
        class H2 {{ f = new B(); }}
        function h(o: any) {{ return o.f; }}
        function g(o: any) {{ return o; }}
        function f(x: any) {{ return g(h(x)); }}
        const a = f(new H1()); const b = f(new H2());
        console.log(a.a, b.b);"
    ));
    exact(&pf, "a", 1);
    exact(&pf, "b", 1);
    // `g` and `h` are expanded into `f`'s summary: only `f`'s two program
    // calls make instances.
    assert_eq!(pf.stats.composed, 2, "{}", pf.dump_text());
    assert_eq!(pf.stats.instances, 2, "{}", pf.dump_text());
}

#[test]
fn callback_instance_per_passing_call() {
    let pf = facts(&format!(
        "{CLASSES}
        function id(o: any) {{ return o; }}
        const r1 = [new A()].map(id); const r2 = [new B()].map(id);
        console.log(r1[0].x, r2[0].y);"
    ));
    // One instance of `id` per `map` call that passes it.
    assert_eq!(pf.stats.instances, 2, "{}", pf.dump_text());
    exact(&pf, "x", 1);
    exact(&pf, "y", 1);
}

/// Diagnostics on a file: `CW_SRC=<file> cargo test -- --ignored debug_file --nocapture`.
#[test]
#[ignore]
fn debug_file() {
    let path = std::env::var("CW_SRC").expect("CW_SRC");
    let src = std::fs::read_to_string(&path).expect("read");
    let pf = facts_with(
        &src,
        FactsConfig {
            explain: true,
            unit: std::env::var_os("CW_UNIT").is_some(),
            ..Default::default()
        },
    );
    let proven = pf
        .sites
        .iter()
        .filter(|s| matches!(s.verdict, Verdict::T0Mono | Verdict::T0Set))
        .count();
    eprintln!(
        "proven {proven} of {} work {}",
        pf.sites.len(),
        pf.stats.work
    );
    if let Ok(out) = std::env::var("CW_DUMP") {
        std::fs::write(out, pf.dump_text()).unwrap();
    }
}
