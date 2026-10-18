//! Unit tests: small programs lowered from source, facts checked directly.

use crate::{compute, FactsConfig, ProgramFacts, Verdict};

fn facts(src: &str) -> ProgramFacts {
    facts_with(src, FactsConfig::default())
}

fn facts_with(src: &str, cfg: FactsConfig) -> ProgramFacts {
    let module = perry_parser::parse_typescript(src, "t.ts").expect("parses");
    let hir = perry_hir::lower_module(&module, "t", "t.ts").expect("lowers");
    compute(&[("t.ts".to_string(), &hir)], &cfg)
}

fn tmpl<'a>(pf: &'a ProgramFacts, name: &str) -> &'a crate::TemplateFacts {
    pf.templates
        .iter()
        .find(|t| t.display.ends_with(&format!(":{name}")))
        .unwrap_or_else(|| panic!("template {name} in\n{}", pf.dump_text()))
}

fn field<'a>(t: &'a crate::TemplateFacts, k: &str) -> &'a crate::FieldFacts {
    t.fields.iter().find(|f| f.key == k).expect("field")
}

#[test]
fn constructor_only_field_is_readonly_definite_and_t0() {
    let pf = facts(
        r#"
        class P { x: number; constructor(x: number) { this.x = x; } }
        function get(p: P) { return p.x; }
        console.log(get(new P(1)));
        "#,
    );
    let p = tmpl(&pf, "P");
    assert!(p.prefix_stable, "{}", pf.dump_text());
    assert!(p.fixed_layout, "{}", pf.dump_text());
    let x = field(p, "x");
    assert!(x.readonly && x.definite);
    assert_eq!(x.kind.desc(), "num");
    let site = pf.sites.iter().find(|s| s.key == "x").expect("site");
    assert_eq!(site.verdict, Verdict::T0Mono, "{}", pf.dump_text());
}

#[test]
fn post_construction_store_kills_readonly() {
    let pf = facts(
        r#"
        class P { x: number; constructor() { this.x = 1; } bump() { this.x = 2; } }
        const p = new P(); p.bump(); console.log(p.x);
        "#,
    );
    let p = tmpl(&pf, "P");
    assert!(!field(p, "x").readonly, "{}", pf.dump_text());
    assert!(p.prefix_stable);
}

#[test]
fn delete_is_hard() {
    let pf = facts(
        r#"
        class P { x = 1; y = 2; }
        const p = new P(); delete (p as any).x; console.log(p.y);
        "#,
    );
    assert!(!tmpl(&pf, "P").prefix_stable, "{}", pf.dump_text());
}

#[test]
fn define_property_is_hard() {
    let pf = facts(
        r#"
        class P { x = 1; }
        const p = new P(); Object.defineProperty(p, "x", { get() { return 5; } });
        console.log(p.x);
        "#,
    );
    assert!(!tmpl(&pf, "P").prefix_stable, "{}", pf.dump_text());
}

#[test]
fn any_alias_store_widens_kind() {
    let pf = facts(
        r#"
        class P { x: number = 1; }
        const p = new P(); const a: any = p; a.x = "s"; console.log(p.x);
        "#,
    );
    let x = field(tmpl(&pf, "P"), "x");
    assert!(!x.readonly);
    assert!(x.kind.desc().contains("str"), "{}", pf.dump_text());
}

#[test]
fn object_create_of_prototype_is_hard() {
    let pf = facts(
        r#"
        class P { x = 1; m() { return this.x; } }
        const o = Object.create(P.prototype); console.log(o.m(), new P().m());
        "#,
    );
    assert!(!tmpl(&pf, "P").prefix_stable, "{}", pf.dump_text());
}

#[test]
fn set_prototype_of_is_hard() {
    let pf = facts(
        r#"
        class P { x = 1; }
        class Q { y = 2; }
        const p = new P(); Object.setPrototypeOf(p, Q.prototype); console.log(p.x);
        "#,
    );
    assert!(!tmpl(&pf, "P").prefix_stable, "{}", pf.dump_text());
}

#[test]
fn closure_writer_is_post_construction() {
    let pf = facts(
        r#"
        class P { x = 1; }
        const p = new P();
        const set = () => { p.x = 3; };
        set(); console.log(p.x);
        "#,
    );
    assert!(!field(tmpl(&pf, "P"), "x").readonly, "{}", pf.dump_text());
}

#[test]
fn unknown_callee_escapes_argument() {
    let pf = facts(
        r#"
        class P { x = 1; }
        declare const ext: any;
        const p = new P(); ext(p); console.log(p.x);
        "#,
    );
    let p = tmpl(&pf, "P");
    assert!(p.escape.is_some() && !p.prefix_stable, "{}", pf.dump_text());
}

#[test]
fn method_call_resolves_one_target() {
    let pf = facts(
        r#"
        class P { x = 1; m() { return this.x; } }
        function f(p: P) { return p.m(); }
        console.log(f(new P()));
        "#,
    );
    let site = pf
        .sites
        .iter()
        .find(|s| s.key == "m" && s.is_call)
        .expect("call site");
    assert_eq!(site.verdict, Verdict::T0Mono, "{}", pf.dump_text());
}

#[test]
fn literal_template_facts() {
    let pf = facts(
        r#"
        function mk(n: number) { return { a: n, b: "s" }; }
        function rd(o: { a: number; b: string }) { return o.a; }
        console.log(rd(mk(1)));
        "#,
    );
    let site = pf.sites.iter().find(|s| s.key == "a").expect("site");
    assert_eq!(site.verdict, Verdict::T0Mono, "{}", pf.dump_text());
}

#[test]
fn sabotage_plants_the_fact() {
    let src = r#"
        class P { x: number = 1; bump() { this.x++; } }
        const p = new P(); p.bump(); console.log(p.x);
    "#;
    let pf = facts(src);
    assert!(!field(tmpl(&pf, "P"), "x").readonly);
    let pf2 = facts_with(
        src,
        FactsConfig {
            relations: false,
            sabotage: Some("readonly:P.x".into()),
            dynamic_code: None,
            budget_secs: None,
            profile: false,
            max_pts: None,
            widen_k: None,
            ..Default::default()
        },
    );
    assert!(field(tmpl(&pf2, "P"), "x").readonly);
    assert!(pf2.dump_text().contains("# sabotage readonly:P.x"));
    assert_ne!(pf.digest_for("t.ts"), pf2.digest_for("t.ts"));
}

/// Writes `<dir>/t.facts` and `<dir>/t.facts.rel` for the checker scripts
/// when `WPFACTS_TEST_DUMP_DIR` is set (developer aid; a no-op otherwise).
#[test]
fn write_dump_for_checker() {
    let Some(dir) = std::env::var_os("WPFACTS_TEST_DUMP_DIR") else {
        return;
    };
    let src = std::env::var("WPFACTS_TEST_SRC").unwrap_or_else(|_| {
        r#"
        class P { x: number; y = "s"; constructor(x: number) { this.x = x; } m() { return this.x; } }
        class Q extends P { z = 1; get g() { return this.z; } }
        function f(p: P) { return p.x + p.m(); }
        const o = { a: 1, b: [1, 2] };
        const q = new Q(2);
        q.z = 3;
        console.log(f(new P(1)), f(q), q.g, o.a, o.b.length);
        "#
        .to_string()
    });
    let pf = facts_with(
        &src,
        FactsConfig {
            relations: true,
            sabotage: std::env::var("WPFACTS_TEST_SABOTAGE").ok(),
            dynamic_code: None,
            budget_secs: None,
            profile: false,
            max_pts: None,
            widen_k: None,
            ..Default::default()
        },
    );
    let dir = std::path::PathBuf::from(dir);
    std::fs::write(dir.join("t.facts"), pf.dump_text()).unwrap();
    std::fs::write(dir.join("t.facts.rel"), pf.relations_text().unwrap()).unwrap();
}

/// Decision 150: dynamic code naming the global scope is no kill. The class
/// keeps its facts; the program's proofs are guarded by the runtime's
/// interpreted-prototype-write word instead.
#[test]
fn dynamic_code_reach_guards_instead_of_killing() {
    let mut reach = perry_hir::dynamic_code::DynamicCodeReach::default();
    reach.record_global("new Function(...) @ t.ts:4".into());
    let pf = facts_with(
        "class P { x = 1; m() { return this.x; } } console.log(new P().m());",
        FactsConfig {
            dynamic_code_reach: reach,
            relations: true,
            ..Default::default()
        },
    );
    assert!(pf.dynamic_code_reach.disables_builtin_constants());
    assert!(pf.relations_text().unwrap().contains("dynamicreach 1"));
    assert!(pf
        .interp_guard
        .as_deref()
        .unwrap()
        .contains("dynamic_code_reach"));
    assert!(tmpl(&pf, "P").prefix_stable, "{}", pf.dump_text());
    assert!(field(tmpl(&pf, "P"), "x").readonly);
}

fn interp_cfg() -> FactsConfig {
    FactsConfig {
        dynamic_code: Some("E14 test".into()),
        ..Default::default()
    }
}

/// Interpreted code is ONE summary node (⊤): what is passed to a function
/// it made, and what such a function returns, escapes; a class whose
/// instances never reach it keeps every fact. `this` and locals that can
/// hold both get the class plus ⊤ (a checked proof in codegen).
#[test]
fn interpreted_function_is_a_summary_node() {
    let pf = facts_with(
        r#"
        class Kept { x: number; constructor(x: number) { this.x = x; } }
        class Given { y: number; constructor(y: number) { this.y = y; } }
        const body = ["return o"].join("");
        const f = new Function("o", body);
        const back = f(new Given(2));
        function rd(k: Kept) { return k.x; }
        let either: any = new Kept(3);
        if (back) either = back;
        console.log(rd(new Kept(1)), back.y, either.x);
        "#,
        interp_cfg(),
    );
    assert!(pf.interp_guard.is_some(), "{}", pf.dump_text());
    let kept = tmpl(&pf, "Kept");
    assert!(
        kept.prefix_stable && kept.fixed_layout,
        "{}",
        pf.dump_text()
    );
    assert!(field(kept, "x").readonly);
    assert_eq!(field(kept, "x").kind.desc(), "num");
    let given = tmpl(&pf, "Given");
    assert!(!given.prefix_stable, "{}", pf.dump_text());
    assert!(field(given, "y").kind.top);
    // `either` holds Kept and ⊤ (the interpreted function's result).
    let kept_id = kept.class_id.unwrap();
    let mb = &pf.bindings[0];
    assert!(
        mb.locals
            .iter()
            .any(|(_, b)| b.other && b.class_ids == vec![kept_id]),
        "{:?}",
        mb.locals
    );
}

/// A ⊤ callee that may be the Function constructor (tsc's 84k calls) only
/// connects values that are already ⊤: an object that never reaches a ⊤
/// call keeps its facts.
#[test]
fn top_call_connects_only_top_values() {
    let pf = facts_with(
        r#"
        class Kept { x: number; constructor(x: number) { this.x = x; } }
        const u: any = (globalThis as any).someUnknown;
        const r = u("a", 1);
        function rd(k: Kept) { return k.x; }
        console.log(rd(new Kept(1)), r);
        "#,
        interp_cfg(),
    );
    assert!(pf.stats.top_calls > 0);
    assert!(pf.interp_guard.is_some());
    let kept = tmpl(&pf, "Kept");
    assert!(kept.prefix_stable, "{}", pf.dump_text());
    assert!(field(kept, "x").readonly);
}

/// The bounds hold from the first constraint. Generation solves online
/// (`cons`/`edge` apply each rule to what the node already holds), so a
/// bound passed while the constraints are generated must stop the pass
/// there. The Next.js server spent 12+ minutes and 50 GB in generation,
/// with no bound (not even widening) in force yet.
#[test]
fn bounds_hold_during_generation() {
    let mut src = String::new();
    for i in 0..200 {
        src.push_str(&format!(
            "class C{i} {{ v = {i}; m() {{ return this.v; }} }}\n\
             function f{i}(x: any) {{ return x.m() + x.m() + x.m() + new C{i}().m() + [x, x].length; }}\n"
        ));
    }
    src.push_str("console.log(f0(new C1()));\n");
    let full = facts(&src);
    assert!(!full.stats.aborted);
    // A node bound (max_pts / 8) far below what generation creates.
    let bounded = facts_with(
        &src,
        FactsConfig {
            max_pts: Some(800),
            ..Default::default()
        },
    );
    assert!(bounded.stats.aborted);
    assert!(bounded.templates.is_empty() && bounded.sites.is_empty());
    assert!(
        bounded.stats.nodes * 2 < full.stats.nodes,
        "generation went on past the bound: {} of {} nodes",
        bounded.stats.nodes,
        full.stats.nodes
    );
}

/// Re-binding a bound function (`f = f.bind(x)` in a loop) and calling or
/// constructing the result must reach a fixpoint: one bound value per bind
/// site for bound targets, and one merged call per bound value for the
/// calls the solver itself makes through a bound target.
#[test]
fn bind_chains_terminate() {
    let src = r#"
        let f: any = function (this: any, a: any) { return a; };
        let g: any = function (this: any, a: any) { this.a = a; };
        for (let i = 0; i < 3; i++) { f = f.bind(null, i); g = g.bind(null); }
        console.log(f(1), new g(2));
    "#;
    let pf = facts_with(
        src,
        FactsConfig {
            max_work: Some(2_000_000),
            ..Default::default()
        },
    );
    assert!(!pf.stats.aborted, "{}", pf.dump_text());
}

fn site<'a>(pf: &'a ProgramFacts, key: &str) -> &'a crate::SiteFacts {
    pf.sites
        .iter()
        .find(|s| s.key == key)
        .unwrap_or_else(|| panic!("site .{key} in\n{}", pf.dump_text()))
}

/// The iteration protocol written out by hand over builtin iterators
/// (array, `values()`, Map entries, Set): `next().value` is the element,
/// not a value from unmodeled code, and the source keeps its facts.
#[test]
fn builtin_iterator_next_yields_elements() {
    let pf = facts(
        r#"
        class A { a: number; constructor(a: number) { this.a = a; } }
        class B { b: number; constructor(b: number) { this.b = b; } }
        class C { c: number; constructor(c: number) { this.c = c; } }
        class D { d: number; constructor(d: number) { this.d = d; } }
        const xs = [new A(1), new A(2)];
        const it = xs[Symbol.iterator]();
        let s = 0;
        for (let r = it.next(); !r.done; r = it.next()) s += r.value.a;
        const ys = [new B(1)];
        const vi = ys.values();
        const v = vi.next();
        if (!v.done) s += v.value.b;
        const mp = new Map([["k", new C(3)]]);
        const ei = mp.entries();
        const e = ei.next();
        if (!e.done) s += e.value[1].c;
        const st = new Set([new D(4)]);
        const si = st.values();
        const w = si.next();
        if (!w.done) s += w.value.d;
        console.log(s);
        "#,
    );
    for k in ["a", "b", "c", "d"] {
        let st = site(&pf, k);
        assert!(!st.top, ".{k} receiver carries TOP\n{}", pf.dump_text());
        assert!(
            matches!(st.verdict, Verdict::T0Mono | Verdict::T0Set),
            ".{k}: {:?}\n{}",
            st.verdict,
            pf.dump_text()
        );
    }
}

/// for-of, spread and destructuring over a hand-written iterable flow the
/// user's `next().value` into the loop variable.
#[test]
fn user_iterable_flows_through_for_of_spread_destructuring() {
    let pf = facts(
        r#"
        class P { p: number; constructor(p: number) { this.p = p; } }
        class Bag {
          items: P[];
          constructor() { this.items = [new P(1), new P(2)]; }
          [Symbol.iterator]() {
            let i = 0;
            const items = this.items;
            return { next: () => i < items.length ? { value: items[i++], done: false } : { value: undefined, done: true } };
          }
        }
        const bag = new Bag();
        let s = 0;
        for (const x of bag) s += x.p;
        const [first] = bag;
        if (first) s += first.p;
        console.log(s + [...bag].length);
        "#,
    );
    for st in pf.sites.iter().filter(|s| s.key == "p") {
        assert!(!st.top, ".p receiver carries TOP\n{}", pf.dump_text());
    }
}
