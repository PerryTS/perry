//! A call is a call of the bodies its callee set holds (FVA §3): proven
//! when the lookup is the read rule's and every value it gives is a body.
//! Several bodies are fine; ⊤, a builtin function value, a bound function,
//! a class or a value that is no function is not.

use super::*;

/// The one site of `key` (`()` names a call through a value).
fn site<'a>(pf: &'a ProgramFacts, key: &str) -> &'a crate::SiteFacts {
    let v: Vec<_> = pf.sites.iter().filter(|s| s.key == key).collect();
    assert_eq!(v.len(), 1, "site {key} in\n{}", pf.dump_text());
    v[0]
}

/// The bodies' names, without their module and HIR id.
fn bodies(s: &crate::SiteFacts) -> Vec<&str> {
    s.targets
        .iter()
        .map(|t| t.rsplit(':').next().unwrap_or(t))
        .map(|t| t.split('#').next().unwrap_or(t))
        .collect()
}

fn units(src: &str) -> ProgramFacts {
    let module = perry_parser::parse_typescript(src, "t.ts").expect("parses");
    let hir = perry_hir::lower_module(&module, "t", "t.ts").expect("lowers");
    crate::compute_units(&[("t.ts".to_string(), &hir)], &FactsConfig::default())
}

/// Per unit never claims more than whole program: a proven call there is
/// proven here, with a body set that holds this one's.
fn never_stronger(src: &str) {
    let (u, w) = (units(src), facts(src));
    for s in u.sites.iter().filter(|s| s.is_call) {
        if matches!(s.verdict, Verdict::No(_)) {
            continue;
        }
        let ws = w
            .sites
            .iter()
            .find(|x| (x.module, x.byte_offset, &x.key) == (s.module, s.byte_offset, &s.key))
            .unwrap_or_else(|| {
                panic!(
                    "{} {} {} in\n{}",
                    s.module,
                    s.byte_offset,
                    s.key,
                    w.dump_text()
                )
            });
        assert!(
            !matches!(ws.verdict, Verdict::No(_)),
            "{s:?}\n{}",
            w.dump_text()
        );
        // Another unit's body is named by its contract, without the HIR id.
        let name = |t: &String| t.split('#').next().unwrap_or(t).to_string();
        let have: Vec<String> = s.targets.iter().map(name).collect();
        assert!(
            ws.targets.iter().all(|t| have.contains(&name(t))),
            "{s:?} {ws:?}"
        );
    }
}

const FIELDS: &str = r#"
    function fa(x: number) { return x + 1; }
    function fb(x: number) { return x * 2; }
    function run(o: any, v: number) { return o.f(v); }
    const p: any = { f: fa, tag: 1 };
    const q: any = { f: fb, tag: 2 };
    let s = 0;
    for (let i = 0; i < 10; i++) s += run(i % 2 ? p : q, i);
    console.log(s);
"#;

#[test]
fn a_member_call_over_two_layouts_runs_both_bodies() {
    // Two literals with a function field each, one call site: the field is
    // one slot of each layout, and the set is {fa, fb}.
    let pf = facts(FIELDS);
    let s = site(&pf, "f");
    assert_eq!(s.verdict, Verdict::T0Set, "{}", pf.dump_text());
    assert_eq!(bodies(s), ["fa", "fb"], "{}", pf.dump_text());
    never_stronger(FIELDS);
}

#[test]
fn a_builtin_function_value_in_the_set_is_not_proven() {
    // A third literal stores `Math.abs` into the field: the site keeps the
    // generic call (the builtin, called as a method, also gets the literal
    // as its receiver: unknown code, so the layout escapes first).
    let src = FIELDS.replace(
        "let s = 0;",
        "const r: any = { f: Math.abs, tag: 3 }; run(r, -1); let s = 0;",
    );
    let pf = facts(&src);
    let s = site(&pf, "f");
    assert!(matches!(s.verdict, Verdict::No(_)), "{}", pf.dump_text());
    assert!(s.targets.is_empty());
    // Through a value the builtin is the set's own reason.
    let src = r#"
        function fa(x: number) { return x + 1; }
        function pick(n: number): any { return n > 1 ? fa : Math.abs; }
        console.log(pick(2)(1), pick(0)(-1));
    "#;
    let pf = facts(src);
    let calls: Vec<_> = pf.sites.iter().filter(|s| s.key == "()").collect();
    assert_eq!(calls.len(), 2, "{}", pf.dump_text());
    for s in calls {
        assert_eq!(
            s.verdict,
            Verdict::No("callee set holds a builtin function value".into()),
            "{}",
            pf.dump_text()
        );
    }
}

#[test]
fn virtual_dispatch_is_a_set_of_methods() {
    // `r.m(x)` over two classes: each resolves `m` on its chain, one body
    // per receiver layout.
    let src = r#"
        class A { k = 1; m(x: number) { return x + this.k; } }
        class B { k = 2; m(x: number) { return x * this.k; } }
        function call(r: A | B, x: number) { return r.m(x); }
        const rs = [new A(), new B()];
        let s = 0;
        for (let i = 0; i < 10; i++) s += call(rs[i % 2], i);
        console.log(s);
    "#;
    let pf = facts(src);
    let s = site(&pf, "m");
    assert_eq!(s.verdict, Verdict::T0Set, "{}", pf.dump_text());
    assert_eq!(bodies(s), ["m"; 2], "{}", pf.dump_text());
    never_stronger(src);
}

#[test]
fn a_receiver_without_the_method_is_not_proven() {
    // `B` has no `m`: its read is `undefined`, the call would throw.
    let src = r#"
        class A { k = 1; m() { return this.k; } }
        class B { k = 2; }
        function call(r: any) { try { return r.m(); } catch (e) { return 0; } }
        console.log(call(new A()), call(new B()));
    "#;
    let pf = facts(src);
    let s = site(&pf, "m");
    assert_eq!(
        s.verdict,
        Verdict::No("method not found on the chain".into()),
        "{}",
        pf.dump_text()
    );
}

#[test]
fn a_call_through_a_value_runs_the_bodies_the_value_is() {
    // `g(g(x))` through a parameter, `h(i)` through a local holding one of
    // two closures.
    let src = r#"
        const inc = (x: number) => x + 1;
        function twice(g: (x: number) => number, x: number) { return g(g(x)); }
        function pick(n: number) { return n > 3 ? inc : (x: number) => x - 1; }
        let s = 0;
        for (let i = 0; i < 6; i++) { const h = pick(i); s += h(i); }
        console.log(twice(inc, 1), s);
    "#;
    let pf = facts(src);
    let calls: Vec<_> = pf.sites.iter().filter(|s| s.key == "()").collect();
    assert_eq!(calls.len(), 3, "{}", pf.dump_text());
    let one = calls
        .iter()
        .filter(|s| s.verdict == Verdict::T0Mono)
        .count();
    let two: Vec<_> = calls
        .iter()
        .filter(|s| s.verdict == Verdict::T0Set)
        .collect();
    assert_eq!((one, two.len()), (2, 1), "{}", pf.dump_text());
    assert_eq!(two[0].targets.len(), 2, "{}", pf.dump_text());
    never_stronger(src);
}

#[test]
fn a_value_that_may_be_no_function_is_not_proven() {
    // `g` is `undefined` until the assignment: calling it throws.
    let src = r#"
        let g: any = undefined;
        function late() { try { return g(1); } catch (e) { return "threw"; } }
        console.log(late());
        g = (x: number) => x + 100;
        console.log(late());
    "#;
    let pf = facts(src);
    assert_eq!(
        site(&pf, "()").verdict,
        Verdict::No("callee set holds a non-function value".into()),
        "{}",
        pf.dump_text()
    );
}

#[test]
fn a_callee_from_unknown_code_is_not_proven() {
    // The callee comes from a global no model describes: ⊤.
    let src = r#"
        const f: any = (globalThis as any).someHook;
        console.log(f(1));
    "#;
    let pf = facts(src);
    let s = site(&pf, "()");
    assert!(
        matches!(&s.verdict, Verdict::No(r) if r.starts_with("callee set holds ⊤")),
        "{:?}\n{}",
        s.verdict,
        pf.dump_text()
    );
}

#[test]
fn a_class_or_a_bound_function_in_the_set_is_not_proven() {
    for (src, why) in [
        (
            "class C { v = 1; } const k: any = C; try { k(); } catch (e) { console.log(1); }",
            "callee set holds a class",
        ),
        (
            "function f(this: any) { return 1; } const b: any = f.bind({}); console.log(b());",
            "callee set holds a bound function",
        ),
    ] {
        let pf = facts(src);
        assert_eq!(
            site(&pf, "()").verdict,
            Verdict::No(why.into()),
            "{}",
            pf.dump_text()
        );
    }
}

#[test]
fn a_body_beside_top_is_not_proven() {
    // One branch gives a body, the other a value from unknown code: the set
    // holds ⊤ beside `fa`.
    let src = r#"
        function fa(x: number) { return x + 1; }
        const f: any = Math.random() > 2 ? fa : (globalThis as any).hook;
        console.log(typeof f === "function" ? f(1) : 0);
    "#;
    let pf = facts(src);
    let s = site(&pf, "()");
    assert!(
        matches!(&s.verdict, Verdict::No(r) if r.starts_with("callee set holds ⊤")),
        "{:?}\n{}",
        s.verdict,
        pf.dump_text()
    );
}

#[test]
fn bindings_and_fields_carry_their_bodies_for_codegen() {
    // What codegen reads: a local holding two closures has both; a local
    // that may be undefined has none; a literal field holding a function
    // per layout token has their union.
    let src = r#"
        const inc = (x: number) => x + 1;
        const dec = (x: number) => x - 1;
        function pick(n: number) { return n > 3 ? inc : dec; }
        let late: any = undefined;
        function useLate() { return typeof late === "function" ? late(1) : 0; }
        let s = 0;
        for (let i = 0; i < 6; i++) { const h = pick(i); s += h(i); }
        late = inc;
        console.log(s, useLate());
    "#
    .to_string()
        + FIELDS;
    let pf = facts(&src);
    let with: Vec<usize> = pf.bindings[0]
        .locals
        .iter()
        .map(|(_, b)| b.bodies.len())
        .filter(|&n| n > 0)
        .collect();
    assert!(with.contains(&2), "{with:?}");
    for (id, b) in &pf.bindings[0].locals {
        // Bodies only where nothing else flows (`late` may be undefined).
        assert!(
            b.bodies.is_empty() || (b.prims == 0 && b.class_ids.is_empty()),
            "local {id}: {b:?}"
        );
        assert!(b.prims == 0 || b.bodies.is_empty(), "local {id}: {b:?}");
    }
    let lit = pf
        .templates
        .iter()
        .find(|t| t.fields.iter().any(|f| f.key == "f"))
        .expect("the literal layout");
    let f = lit.fields.iter().find(|f| f.key == "f").unwrap();
    assert_eq!(f.fns.len(), 2, "{f:?}");
    assert!(f.fns.iter().all(|b| b.plain && !b.closure), "{f:?}");
}
