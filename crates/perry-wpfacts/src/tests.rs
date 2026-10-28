//! Unit tests: small programs lowered from source, facts checked directly.

mod accessor_keys;
mod array_ctor;
mod array_elements;
mod array_length;
mod array_named_keys;
mod array_record;
mod binding_write_gap;
mod birth_site;
mod body_identity;
mod builtin_members;
mod callback_origin;
mod captured;
mod cjs_require;
mod class_statics;
mod closed_world;
mod closure_context;
mod dynamic_code;
mod escaped_constructor;
mod escaped_contract_result;
mod escaped_method_this;
mod export_star;
mod for_in_keys;
mod function_values;
mod hub_tokens;
mod indexed_owner;
mod layout_key;
mod named_accessors;
mod native_fs;
mod native_proto;
mod parent_value;
mod prim_result;
mod proof_receivers;
mod proto_value;
mod protocol_calls;
mod protocol_keys;
mod receiver_this;
mod rest_element;
mod run_once_closed;
mod runtime_helpers;
mod summary;
mod super_heritage;
mod unit_alias;
mod unit_cjs;
mod unit_ctor;
mod unit_effects;
mod unit_extends;
mod unit_mode;
mod unit_namespace;
mod unit_shared;
mod unit_this;

use crate::{compute, FactsConfig, ProgramFacts, Verdict};

fn facts(src: &str) -> ProgramFacts {
    facts_with(src, FactsConfig::default())
}

fn facts_with(src: &str, mut cfg: FactsConfig) -> ProgramFacts {
    // Every test solve is bounded: a broken rule ends OVER (no facts, the
    // test fails) instead of running away.
    cfg.max_work.get_or_insert(50_000_000);
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

/// Some layout's field `key` holds a function, and never ⊤.
fn holds_fn(pf: &ProgramFacts, key: &str) -> bool {
    let fs: Vec<_> = pf
        .templates
        .iter()
        .flat_map(|t| &t.fields)
        .filter(|f| f.key == key)
        .collect();
    fs.iter().any(|f| f.kind.funcs > 0) && fs.iter().all(|f| !f.kind.top)
}

/// Every site of `key` is refused, for `why`.
fn refused(pf: &ProgramFacts, key: &str, why: &str) -> bool {
    let s: Vec<_> = pf.sites.iter().filter(|s| s.key == key).collect();
    !s.is_empty()
        && s.iter()
            .all(|s| matches!(&s.verdict, Verdict::No(r) if r.starts_with(why)))
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
/// with no bound in force yet.
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

/// `this.k = v` in a constructor function is a birth writer of `new f()`
/// instances even when `this` already holds instances as the store is
/// generated (a nested `new f()` earlier in the program): the site is
/// marked construction-phase before its constraint applies.
#[test]
fn fn_instance_ctor_stores_are_birth_writers() {
    let pf = facts(
        r#"
        function Vec(this: any, x: number, y: number) {
          this.x = x;
          this.y = y;
        }
        Vec.prototype.add = function (this: any, o: any) {
          return new (Vec as any)(this.x + o.x, this.y + o.y);
        };
        const a = new (Vec as any)(1, 2);
        const b = a.add(new (Vec as any)(3, 4));
        console.log(b.x, b.y);
        "#,
    );
    let v = pf
        .templates
        .iter()
        .find(|t| t.display.ends_with(":Vec"))
        .unwrap_or_else(|| panic!("Vec in\n{}", pf.dump_text()));
    assert!(v.prefix_stable, "{}", pf.dump_text());
    assert!(field(v, "x").readonly, "{}", pf.dump_text());
    assert!(field(v, "y").readonly, "{}", pf.dump_text());
}

// Match the generator's reserved globals, so synthetic source node ids
// cannot accidentally double as the canonical unmodeled-value node.
fn copy_solver_model() -> crate::model::Model {
    use crate::model::{prim, Model, TokKind, TOP};
    let mut m = Model::default();
    assert_eq!(m.tok(TokKind::Top), TOP);
    m.top_tok_node = m.node_l("unmodeled");
    m.add_tok(m.top_tok_node, TOP);
    m.add_prim(m.top_tok_node, prim::ALL);
    m.thrown = m.node_l("thrown");
    m
}

#[test]
fn dense_prototype_copies_preserve_all_values_within_work_bound() {
    use crate::model::{Key, TokKind, TOP};
    // Abstract alternatives can form a cycle even though each concrete
    // prototype chain is acyclic. Exercise both sorted and hashed sets.
    for count in [40, 60] {
        let mut m = copy_solver_model();
        assert_eq!(m.tok(TokKind::Top), TOP);
        m.max_work = 500_000;
        let root = m.tok(TokKind::ObjectProto);
        let holders: Vec<_> = (0..24).map(|i| m.tok(TokKind::ModNs(i))).collect();
        let values: Vec<_> = (100..100 + count)
            .map(|i| m.tok(TokKind::ModNs(i)))
            .collect();
        let any = m.field_any(root);
        m.add(any, &values, 0);
        for &t in &holders {
            let pn = m.proto_node(t);
            m.add(pn, &holders, 0);
            m.add_tok(pn, root);
        }
        let mut reads = Vec::new();
        for i in 0..64 {
            let key = Key::Named(m.atom(&format!("property{i}")));
            for &t in &holders {
                reads.push(m.cv(t, key));
            }
        }
        m.solve();
        assert!(
            !m.aborted,
            "{count}-value copies aborted at {} work",
            m.work
        );
        for n in reads {
            for &v in &values {
                assert!(m.nodes[n as usize].pts.contains_tok(v));
            }
        }
    }
}

#[test]
fn copy_updates_keep_independent_members_across_storage_growth() {
    use crate::model::{TokKind, TOP};
    let mut m = copy_solver_model();
    assert_eq!(m.tok(TokKind::Top), TOP);
    let src = m.node_l("source");
    let dst = m.node_l("destination");
    for i in 0..40 {
        let t = m.tok(TokKind::ModNs(i));
        m.add_tok(src, t);
    }
    m.edge(src, dst);
    m.solve();
    let extra = m.tok(TokKind::ModNs(1000));
    m.add_tok(dst, extra);
    for i in 40..65 {
        let t = m.tok(TokKind::ModNs(i));
        m.add_tok(src, t);
    }
    m.solve();
    assert!(m.nodes[dst as usize].pts.contains_tok(extra));
    assert!(!m.nodes[src as usize].pts.contains_tok(extra));
    assert_eq!(m.nodes[src as usize].pts.len(), 65);
    assert_eq!(m.nodes[dst as usize].pts.len(), 66);
}

#[test]
fn copying_a_base_into_a_superset_keeps_extra_values_and_primitives() {
    use crate::model::{prim, TokKind, TOP};
    let mut m = copy_solver_model();
    assert_eq!(m.tok(TokKind::Top), TOP);
    let src = m.node_l("source");
    let dst = m.node_l("destination");
    let values: Vec<_> = (0..40).map(|i| m.tok(TokKind::ModNs(i))).collect();
    m.add(src, &values, 0);
    let extra = m.tok(TokKind::ModNs(1000));
    m.add_tok(dst, extra);
    m.edge(src, dst);
    m.add_prim(src, prim::NUM);
    m.max_work = 100;
    m.solve();
    assert!(!m.aborted, "base copy used {} work", m.work);
    for t in values {
        assert!(m.nodes[dst as usize].pts.contains_tok(t));
    }
    assert!(m.nodes[dst as usize].pts.contains_tok(extra));
    assert!(!m.nodes[src as usize].pts.contains_tok(extra));
    assert_ne!(m.nodes[dst as usize].prim & prim::NUM, 0);
    m.max_work = 0;
    let later = m.tok(TokKind::ModNs(40));
    m.add_tok(src, later);
    m.add_prim(src, prim::STR);
    m.solve();
    assert!(m.nodes[dst as usize].pts.contains_tok(later));
    assert!(m.nodes[dst as usize].pts.contains_tok(extra));
    assert!(!m.nodes[src as usize].pts.contains_tok(extra));
    assert_ne!(m.nodes[dst as usize].prim & prim::STR, 0);
}

#[test]
fn set_snapshots_keep_sorted_members_across_copy_on_write_and_storage_growth() {
    let mut current = crate::pts::Pts::default();
    let mut snapshots = Vec::new();
    for i in 0..160 {
        snapshots.push(current.shared_clone());
        assert!(current.insert(300 - i));
    }
    for (i, snapshot) in snapshots.iter().enumerate() {
        let expected: Vec<_> = ((301 - i as u32)..=300).collect();
        assert_eq!(snapshot.len(), i);
        assert_eq!(snapshot.to_sorted(), expected);
    }
    assert_eq!(current.len(), 160);
    assert!(!current.insert(200));
}

#[test]
fn singleton_inclusion_keeps_concrete_destination_values_and_later_updates() {
    use crate::model::{prim, TokKind, TOP};
    let mut m = copy_solver_model();
    assert_eq!(m.tok(TokKind::Top), TOP);
    let src = m.node_l("singleton source");
    let dst = m.node_l("superset destination");
    let own = m.tok(TokKind::ModNs(1));
    m.add_tok(dst, own);
    m.add_tok(src, TOP);
    m.edge(src, dst);
    m.add_prim(src, prim::NUM);
    m.solve();
    assert!(m.nodes[dst as usize].pts.contains_tok(own));
    assert!(!m.nodes[src as usize].pts.contains_tok(own));
    let later = m.tok(TokKind::ModNs(2));
    m.add(src, &[later], prim::STR);
    m.solve();
    for t in [TOP, own, later] {
        assert!(m.nodes[dst as usize].pts.contains_tok(t));
    }
    assert_eq!(m.nodes[dst as usize].prim, prim::NUM | prim::STR);
    assert!(!m.aborted);
}

#[test]
fn observed_bases_avoid_recopying_pending_prefixes_and_keep_independent_members() {
    use crate::model::{prim, TokKind, TOP};
    for own_members in [false, true] {
        let mut m = copy_solver_model();
        assert_eq!(m.tok(TokKind::Top), TOP);
        let src = m.node_l("source with observed snapshot");
        let values: Vec<_> = (0..40).map(|i| m.tok(TokKind::ModNs(i))).collect();
        m.add(src, &values, prim::NUM);
        let mut destinations = Vec::new();
        let mut extras = Vec::new();
        for i in 0..1000 {
            let dst = m.node_l("snapshot destination");
            m.edge(src, dst);
            destinations.push(dst);
            if own_members {
                let own = m.tok(TokKind::ModNs(1000 + i));
                m.add_tok(dst, own);
                extras.push(own);
            }
        }
        let later = m.tok(TokKind::ModNs(40));
        m.add(src, &[later], prim::STR);
        // The bound measures the solve's deliveries, not setup.
        m.work = 0;
        m.max_work = 6000;
        m.solve();
        assert!(
            !m.aborted,
            "observed-base copies used {} work (own={own_members})",
            m.work
        );
        for (i, &dst) in destinations.iter().enumerate() {
            let n = &m.nodes[dst as usize];
            for &t in &values {
                assert!(n.pts.contains_tok(t));
            }
            assert!(n.pts.contains_tok(later));
            assert_eq!(n.prim, prim::NUM | prim::STR);
            if own_members {
                assert!(n.pts.contains_tok(extras[i]));
                assert!(!m.nodes[src as usize].pts.contains_tok(extras[i]));
            }
        }
    }
}

#[test]
fn observed_relay_bases_share_new_batches_without_enumerating_each_transfer() {
    use crate::model::{Cons, TokKind, TOP};
    let mut m = copy_solver_model();
    assert_eq!(m.tok(TokKind::Top), TOP);
    let src = m.node_l("source relay");
    let first: Vec<_> = (0..40).map(|i| m.tok(TokKind::ModNs(i))).collect();
    m.add(src, &first, 0);
    let mut destinations = Vec::new();
    for _ in 0..1000 {
        let dst = m.node_l("destination relay");
        m.edge(src, dst);
        destinations.push(dst);
    }
    m.cons(destinations[0], Cons::ThisTo { origin: TOP });
    let next: Vec<_> = (40..80).map(|i| m.tok(TokKind::ModNs(i))).collect();
    m.add(src, &next, 0);
    // The bound measures the solve's deliveries, not setup.
    m.work = 0;
    m.max_work = 5000;
    m.solve();
    assert!(!m.aborted, "batch sharing used {} work", m.work);
    for dst in destinations {
        for &t in first.iter().chain(&next) {
            assert!(m.nodes[dst as usize].pts.contains_tok(t));
        }
    }
}

#[test]
fn ordered_batches_keep_prefixes_and_detach_later_appends() {
    use crate::delta::{Batch, Delta};
    let a = Batch::new(vec![4, 1, 7]);
    let b = Batch::new(vec![8, 2]);
    let mut delta = Delta::default();
    delta.push(9);
    delta.append_batch(&a);
    let mut saved = delta.clone();
    delta.append_batch(&b);
    delta.push(0);
    assert_eq!(
        delta.iter().copied().collect::<Vec<_>>(),
        vec![9, 4, 1, 7, 8, 2, 0]
    );
    assert_eq!(saved.iter().copied().collect::<Vec<_>>(), vec![9, 4, 1, 7]);
    delta.clear();
    saved.push(3);
    assert_eq!(
        saved.iter().copied().collect::<Vec<_>>(),
        vec![9, 4, 1, 7, 3]
    );
    assert_eq!(
        a.tokens().iter().copied().collect::<Vec<_>>(),
        vec![4, 1, 7]
    );
    assert!(delta.is_empty());
}

#[test]
fn small_observed_sets_preserve_superset_members_without_recopying_values() {
    use crate::model::{TokKind, TOP};
    let mut m = copy_solver_model();
    let src = m.node_l("small shared source");
    let a = m.tok(TokKind::ModNs(1));
    let b = m.tok(TokKind::ModNs(2));
    m.add(src, &[a, b], 0);
    let mut destinations = Vec::new();
    let mut extras = Vec::new();
    for i in 0..1000 {
        let dst = m.node_l("small superset");
        let extra = m.tok(TokKind::ModNs(1000 + i));
        m.add_tok(dst, extra);
        m.edge(src, dst);
        destinations.push(dst);
        extras.push(extra);
    }
    // The bound measures the solve's deliveries, not setup.
    m.work = 0;
    m.max_work = 1500;
    m.solve();
    assert!(!m.aborted, "small supersets used {} work", m.work);
    for (i, dst) in destinations.into_iter().enumerate() {
        for t in [a, b, extras[i]] {
            assert!(m.nodes[dst as usize].pts.contains_tok(t));
        }
    }
    assert!(!m.nodes[src as usize].pts.contains_tok(TOP));
    assert!(extras
        .into_iter()
        .all(|t| !m.nodes[src as usize].pts.contains_tok(t)));
}

#[test]
fn equal_snapshot_groups_keep_old_owners_covered_after_coalescing() {
    use crate::model::TokKind;
    let mut m = copy_solver_model();
    let src = m.node_l("first group");
    let values: Vec<_> = (0..40).map(|i| m.tok(TokKind::ModNs(i))).collect();
    m.add(src, &values, 0);
    let mut first = Vec::new();
    for _ in 0..500 {
        let dst = m.node_l("first owner");
        m.edge(src, dst);
        first.push(dst);
    }
    let other = m.node_l("larger equal group");
    m.add(other, &values, 0);
    let mut second = Vec::new();
    for _ in 0..2000 {
        let dst = m.node_l("second owner");
        m.edge(other, dst);
        second.push(dst);
    }
    m.edge(src, second[0]);
    let added = m.tok(TokKind::ModNs(40));
    m.add_tok(src, added);
    // The bound measures the solve's deliveries, not setup.
    m.work = 0;
    m.max_work = 5000;
    m.solve();
    assert!(!m.aborted, "coalesced snapshots used {} work", m.work);
    for dst in first {
        assert!(m.nodes[dst as usize].pts.contains_tok(added));
    }
    for (i, dst) in second.into_iter().enumerate() {
        assert_eq!(m.nodes[dst as usize].pts.contains_tok(added), i == 0);
    }
    assert!(!m.nodes[other as usize].pts.contains_tok(added));
}

#[test]
fn delivered_cardinality_proves_batch_adoption_with_independent_storage() {
    use crate::model::TokKind;
    let mut m = copy_solver_model();
    let src = m.node_l("source relay");
    let first: Vec<_> = (0..40).map(|i| m.tok(TokKind::ModNs(i))).collect();
    m.add(src, &first, 0);
    let mut destinations = Vec::new();
    for _ in 0..1000 {
        let dst = m.node_l("destination relay");
        m.edge(src, dst);
        destinations.push(dst);
    }
    m.solve();
    assert!(!m.aborted);
    // Preserve every value while giving each old destination its own
    // allocation: the proof must not depend on accidental sharing.
    for &dst in &destinations {
        let mut pts = crate::pts::Pts::default();
        for &t in &first {
            pts.insert(t);
        }
        m.nodes[dst as usize].pts = pts;
    }
    let own = m.tok(TokKind::ModNs(1000));
    m.add_tok(destinations[999], own);
    let next: Vec<_> = (40..80).map(|i| m.tok(TokKind::ModNs(i))).collect();
    m.add(src, &next, 0);
    m.work = 0;
    m.max_work = 2000;
    m.solve();
    assert!(!m.aborted, "cardinality batches used {} work", m.work);
    for (i, dst) in destinations.into_iter().enumerate() {
        let n = &m.nodes[dst as usize];
        for &t in first.iter().chain(&next) {
            assert!(n.pts.contains_tok(t));
        }
        assert_eq!(n.pts.contains_tok(own), i == 999);
    }
    assert!(!m.nodes[src as usize].pts.contains_tok(own));
}

#[test]
fn new_subscribers_receive_current_values_once_and_then_later_deliveries() {
    use crate::model::TokKind;
    for group in [5, 20] {
        let mut m = copy_solver_model();
        let src = m.node_l("publisher");
        let a = m.tok(TokKind::ModNs(1));
        let b = m.tok(TokKind::ModNs(2));
        let c = m.tok(TokKind::ModNs(3));
        let mut destinations = Vec::new();
        for token in [a, b, c] {
            m.add_tok(src, token);
            for _ in 0..group {
                let dst = m.node_l("subscriber");
                m.edge(src, dst);
                destinations.push(dst);
            }
        }
        // The bound measures the solve's deliveries, not setup.
        m.work = 0;
        m.max_work = group as u64 * 9;
        m.solve();
        assert!(!m.aborted, "late subscribers used {} work", m.work);
        for &dst in &destinations {
            for t in [a, b, c] {
                assert!(m.nodes[dst as usize].pts.contains_tok(t));
            }
        }
        let d = m.tok(TokKind::ModNs(4));
        m.add_tok(src, d);
        m.work = 0;
        m.max_work = group as u64 * 4;
        m.solve();
        assert!(!m.aborted);
        for dst in destinations {
            for t in [a, b, c, d] {
                assert!(m.nodes[dst as usize].pts.contains_tok(t));
            }
        }
    }
}

#[test]
fn cjs_literal_registry_paths_do_not_merge_unrelated_exports() {
    let pf = facts(
        r#"
        class Left { x = 1; }
        class Right { x = "right"; }
        __perry_register_path_module("/cjsr-left.js", new Left());
        __perry_register_path_module("/cjsr-right.js", new Right());
        const left = __perry_require_path_module("/cjsr-left.js");
        const right = __perry_require_path_module("/cjsr-right.js");
        console.log(left.x, right.x);
    "#,
    );
    let sites: Vec<_> = pf.sites.iter().filter(|s| s.key == "x").collect();
    assert_eq!(sites.len(), 2, "{}", pf.dump_text());
    assert!(
        sites.iter().all(|s| s.verdict == Verdict::T0Mono),
        "{}",
        pf.dump_text()
    );
    assert_ne!(sites[0].receivers, sites[1].receivers);
}

#[test]
fn cjs_nonliteral_publication_can_replace_literal_path() {
    let pf = facts(
        r#"
        class Left { x = 1; }
        class Right { x = "right"; }
        __perry_register_path_module("/cjsr-left.js", new Left());
        function publish(path) { __perry_register_path_module(path, new Right()); }
        publish("/cjsr-left.js");
        console.log(__perry_require_path_module("/cjsr-left.js").x);
    "#,
    );
    let site = pf.sites.iter().find(|s| s.key == "x").expect("site");
    assert_ne!(site.verdict, Verdict::T0Mono, "{}", pf.dump_text());
}

#[test]
fn cjs_dynamic_lookup_keeps_all_publications() {
    let pf = facts(
        r#"
        class Left { x = 1; }
        class Right { x = "right"; }
        __perry_register_path_module("/cjsr-left.js", new Left());
        __perry_register_path_module("/cjsr-right.js", new Right());
        function read(path) { return __perry_require_path_module(path).x; }
        console.log(read("/cjsr-left.js"), read("/cjsr-right.js"));
    "#,
    );
    let site = pf.sites.iter().find(|s| s.key == "x").expect("site");
    assert_eq!(site.receivers.len(), 2, "{}", pf.dump_text());
}

#[test]
fn cjs_registry_summary_does_not_reach_every_module() {
    let mut source = String::new();
    for i in 0..20 {
        source.push_str(&format!("class C{i} {{ x = {i}; }} __perry_register_path_module(\"/cjsr-{i}.js\", new C{i}());\n"));
    }
    source.push_str("console.log(__perry_require_path_module(\"/cjsr-0.js\").x);");
    let pf = facts(&source);
    for i in 0..20 {
        assert!(
            tmpl(&pf, &format!("C{i}")).prefix_stable,
            "{}",
            pf.dump_text()
        );
    }
    let site = pf.sites.iter().find(|s| s.key == "x").expect("site");
    assert_eq!(site.verdict, Verdict::T0Mono, "{}", pf.dump_text());
}

#[test]
fn cjs_cache_slot_holds_its_own_record() {
    // CJL2: a static edge of a program that is not closed reads its target's
    // cache slot by its literal path. The runtime caches a record under its
    // own canonical path only, so that slot holds the record published under
    // it (and whatever user code stores there), never every record the
    // registry summary holds.
    let pf = facts(
        r#"
        class A { x = 1; }
        class B { x = 2; }
        const cache = __perry_declared_keys("/cjsc-a.js", "/cjsc-b.js");
        const ra = { exports: new A(), __perry_cjs_record: true };
        const rb = { exports: new B(), __perry_cjs_record: true };
        cache["/cjsc-a.js"] = ra;
        cache["/cjsc-b.js"] = rb;
        __perry_register_path_module("/cjsc-a.js", ra);
        __perry_register_path_module("/cjsc-b.js", rb);
        __perry_cjs_install(cache, undefined);
        const cached = cache["/cjsc-a.js"];
        console.log(cached.exports.x);
    "#,
    );
    let site = pf.sites.iter().find(|s| s.key == "x").expect("site");
    assert_eq!(site.receivers.len(), 1, "{}", pf.dump_text());
    assert_eq!(site.verdict, Verdict::T0Mono, "{}", pf.dump_text());
}

/// `require.cache` (`__perry_declared_keys`) has a null prototype: a read by
/// a computed key holds the records and `undefined`, never what
/// `Object.prototype` holds (its `constructor`: `Object`, `Function`). A
/// store on what such a read returns (the loader's `child.parent = module`)
/// is then a store on a record, not on a builtin (E20), and a call of it is
/// no `Function(...)` call (E14). With `Object.prototype` as the prototype
/// (`__perry_declared_keys_object`) the same read reaches the builtins.
fn null_proto_read(births: &str) -> Option<String> {
    let pf = facts(&format!(
        r#"
        class Parent {{ x = 1; }}
        class Child {{ y = 2; }}
        const cache = {births}("/npr-a.js", "/npr-b.js");
        cache["/npr-a.js"] = new Child();
        function read(path) {{ return cache[path]; }}
        const parent = new Parent();
        const got = read(String(Math.random()));
        if (got !== undefined) got.parent = parent;
        console.log(parent.x);
    "#
    ));
    tmpl(&pf, "Parent").escape.clone()
}

#[test]
fn a_computed_read_of_a_null_prototype_record_stops_at_the_record() {
    assert_eq!(null_proto_read("__perry_declared_keys"), None);
    let escape = null_proto_read("__perry_declared_keys_object");
    assert!(
        escape.as_deref().is_some_and(|e| e.contains("E20")),
        "{escape:?}"
    );
}

#[test]
fn cjs_main_is_one_record_and_does_not_alias_every_export() {
    let pf = facts(
        r#"
        class Value { x = 1; }
        __perry_cjs_install(__perry_declared_keys(), { exports: undefined, __perry_cjs_record: true });
        const first = process.mainModule;
        const second = process.mainModule;
        first.exports = new Value();
        Object.defineProperties(first, { tag: { value: 1 } });
        console.log(second.exports.x);
    "#,
    );
    assert!(tmpl(&pf, "Value").prefix_stable, "{}", pf.dump_text());
    let site = pf.sites.iter().find(|s| s.key == "x").expect("site");
    assert_eq!(site.verdict, Verdict::T0Mono, "{}", pf.dump_text());
}

#[test]
fn cjs_main_record_reaches_code_through_process_main_module() {
    // The installed main record is what `process.mainModule` reads: a store
    // through it is a store on the record, not lost to unmodeled code.
    let pf = facts(
        r#"
        class Value { x = 1; }
        __perry_cjs_install(__perry_declared_keys(), new Value());
        process.mainModule.x = "written";
    "#,
    );
    let x = field(tmpl(&pf, "Value"), "x");
    assert!(!x.readonly, "{}", pf.dump_text());
    assert!(
        x.kind.prims & crate::model::prim::STR != 0,
        "{}",
        pf.dump_text()
    );
}

#[test]
fn define_properties_invokes_own_descriptor_getters() {
    let pf = facts(
        r#"
        class Value { x = 1; }
        const target = {};
        const descriptors = { get a() { return { value: new Value() }; } };
        Object.defineProperties(target, descriptors);
        console.log(target.a.x);
    "#,
    );
    let site = pf.sites.iter().find(|s| s.key == "x").expect("site");
    assert!(
        site.receivers.iter().any(|r| r.ends_with(":Value")),
        "{}",
        pf.dump_text()
    );
}

#[test]
fn cjs_main_can_carry_a_user_iterator_protocol() {
    let pf = facts(
        r#"
        class Value { x = 1; }
        __perry_cjs_install(__perry_declared_keys(), { exports: undefined, __perry_cjs_record: true });
        const main = process.mainModule;
        main[Symbol.iterator] = () => main;
        main.next = () => ({ value: new Value(), done: false });
        for (const value of main) { console.log(value.x); break; }
    "#,
    );
    let site = pf.sites.iter().find(|s| s.key == "x").expect("site");
    assert!(
        site.receivers.iter().any(|r| r.ends_with(":Value")),
        "{}",
        pf.dump_text()
    );
}

#[test]
fn own_descriptor_enumeration_observes_late_accessor_constraints() {
    // Solver constraints activate in points-to order, independently of the
    // source order: a previously enumerated holder must retain its getter
    // subscription when a later constraint supplies an own accessor.
    let pf = facts(
        r#"
        class Target {}
        class Value { x = 1; }
        const target = new Target();
        const descriptors = {};
        Object.defineProperties(target, descriptors);
        Object.defineProperty(descriptors, "a", { get() { return { value: new Value() }; } });
        console.log(target.a.x);
    "#,
    );
    // The getter ran: its `{ value: Value }` is a descriptor
    // `defineProperties` reads, so `target.a` holds the `Value`. (FVA 4.3a:
    // `descriptors` gains `a` as an accessor key, so the descriptor value is
    // the getter's result, not an unknown accessor's.)
    let x: Vec<_> = pf.sites.iter().filter(|s| s.key == "x").collect();
    assert!(!x.is_empty(), "{}", pf.dump_text());
    assert!(
        x.iter()
            .all(|s| s.receivers.iter().any(|r| r.ends_with(":Value"))),
        "{}",
        pf.dump_text()
    );
}

#[test]
fn a_node_that_left_out_a_top_marker_never_takes_a_shared_set() {
    use crate::model::{TokKind, TOP};
    // DECISIONS 42: `d` holds ⊤, so the marker `mk` (reads as ⊤) is left
    // out of it; `d` no longer holds all of `s`. Equal sizes then prove
    // nothing: sharing `s`'s set would drop ⊤ from `d` (`copy_to`), and
    // so would adopting a later delivery (`try_copy_delivery`). `e` has no
    // ⊤: the marker reaches it as before.
    let mut m = copy_solver_model();
    let mk = m.tok(TokKind::ModNs(900));
    let a = m.tok(TokKind::ModNs(901));
    let b = m.tok(TokKind::ModNs(902));
    let (s, d, e) = (m.node_l("s"), m.node_l("d"), m.node_l("e"));
    m.set_marker_top(mk);
    m.add(s, &[mk, a], 0);
    m.add_tok(d, TOP);
    m.edge(s, d);
    m.edge(s, e);
    m.solve();
    let has = |m: &crate::model::Model, n, t| m.nodes[n as usize].pts.contains_tok(t);
    assert!(
        has(&m, d, TOP) && has(&m, d, a) && !has(&m, d, mk),
        "copy_to"
    );
    assert!(has(&m, e, mk) && has(&m, e, a), "no ⊤ beside: delivered");
    m.add_tok(s, b);
    m.solve();
    assert!(has(&m, d, TOP) && has(&m, d, b), "try_copy_delivery");
}

#[test]
fn a_delivery_never_adopts_a_marker_that_turned_top_beside_a_top() {
    use crate::model::{TokKind, TOP};
    // DECISIONS 42: `x` serves its successors before its rules, so `mk`
    // reaches `s` beside ⊤ while it does not read as ⊤ yet; then `x`'s
    // sink records the escape and `mk` reads as ⊤. `d` holds `s`'s old
    // set, ⊤ included, so sizes allow adopting `s`'s set, which would put
    // `mk` beside ⊤ in `d`. The delivery goes through `add_tokens`, which
    // leaves it out.
    let mut m = copy_solver_model();
    let mk = m.tok(TokKind::ModNs(900));
    let a = m.tok(TokKind::ModNs(901));
    m.param_marker.insert(mk, (0, 0));
    let (x, s, d) = (m.node_l("x"), m.node_l("s"), m.node_l("d"));
    // The nodes of one run: a contract parameter's marker is a token there
    // (in storage it would cross as a member of the node's marker, MRK).
    for n in [x, s, d] {
        m.nodes[n as usize].frame = 1;
    }
    m.add(s, &[a], 0);
    m.add_tok(s, TOP);
    m.edge(s, d);
    m.edge(x, s);
    m.sink_node(x, 0);
    m.solve();
    m.add(x, &[mk], 0);
    m.solve();
    let has = |m: &crate::model::Model, n, t| m.nodes[n as usize].pts.contains_tok(t);
    assert!(m.marker_is_top(mk), "the sink made the marker read as ⊤");
    assert!(has(&m, s, mk), "it reached s before it read as ⊤");
    assert!(
        has(&m, d, TOP) && has(&m, d, a) && !has(&m, d, mk),
        "left out beside ⊤"
    );
}

#[test]
fn a_parameter_node_holds_its_argument_marker() {
    use crate::model::{FuncInfo, TokKind, MARK_READ, NO_CTX, NO_FN};
    // DECISIONS 44, the rule itself: a function's parameter node `p` holds
    // its argument marker in place of the markers that arrive, through a
    // delivery too (`try_copy_delivery` would adopt `s`'s set: `p` held all
    // of it). A marker that arrives after the argument marker was used
    // takes the uses so far.
    let mut m = copy_solver_model();
    let (this, ret, args, p) = (
        m.node_l("this"),
        m.node_l("ret"),
        m.node_l("args"),
        m.node_l("p"),
    );
    let token = m.tok(TokKind::Func(0, NO_CTX));
    m.funcs.push(FuncInfo {
        module: 0,
        func_id: 0,
        name: "f".into(),
        params: vec![p],
        rest: None,
        this,
        ret,
        args_all: args,
        has_arguments: false,
        token,
        ctor_of: None,
        lexical_this: false,
        no_summary: true,
        gen_step: false,
        captures: Vec::new(),
        forwards_super: false,
        parent: NO_FN,
        forwarded: false,
    });
    let (m1, m2) = (m.param_marker_tok(0, 0), m.param_marker_tok(0, 1));
    let a = m.tok(TokKind::ModNs(7));
    let s = m.node_l("s");
    m.add_tok(s, a);
    m.edge(s, p);
    m.solve();
    let has = |m: &crate::model::Model, n, t| m.nodes[n as usize].pts.contains_tok(t);
    assert!(has(&m, p, a), "a real token passes");
    m.add_tok(s, m1);
    m.solve();
    let r = m.arg_marker_of[&p];
    assert!(
        has(&m, p, r) && !has(&m, p, m1),
        "the argument marker in place of the marker"
    );
    assert_eq!(m.marker_members(r), vec![m1]);
    m.mark_use(r, MARK_READ);
    assert_eq!(
        m.marker_use[&m1] & MARK_READ,
        MARK_READ,
        "a member takes the uses"
    );
    m.add_tok(s, m2);
    m.solve();
    assert!(!has(&m, p, m2));
    assert_eq!(
        m.marker_use[&m2] & MARK_READ,
        MARK_READ,
        "a late member takes the uses so far"
    );
}

#[test]
fn a_store_into_a_marker_makes_no_field_of_it() {
    use crate::model::{FuncInfo, Key, TokKind, MARK_ESCAPE, NO_CTX, NO_FN};
    // DECISIONS 41, the rule itself (the unit-level growth test,
    // unit_mode::a_store_into_an_argument_keeps_no_field_of_the_marker, is
    // also held by DECISIONS 42): a marker stands for another unit's value,
    // this unit has no storage for it. DECISIONS 84 refines "the store is
    // its escape": the store is a key of its contract (the reader stores
    // an unknown value there), and the stored value escapes.
    let mut m = copy_solver_model();
    let (this, ret, args) = (m.node_l("this"), m.node_l("ret"), m.node_l("args"));
    let token = m.tok(TokKind::Func(0, NO_CTX));
    m.funcs.push(FuncInfo {
        module: 0,
        func_id: 0,
        name: "f".into(),
        params: Vec::new(),
        rest: None,
        this,
        ret,
        args_all: args,
        has_arguments: false,
        token,
        ctor_of: None,
        lexical_this: false,
        no_summary: true,
        gen_step: false,
        captures: Vec::new(),
        forwards_super: false,
        parent: NO_FN,
        forwarded: false,
    });
    let mk = m.param_marker_tok(0, 0);
    let v = m.node_l("v");
    let o = m.tok(TokKind::ModNs(7));
    m.add_tok(v, o);
    let x = m.atom("x");
    let site = m.anon_post_site("test store");
    m.store_into(mk, Key::Named(x), v, site);
    m.solve();
    assert!(!m.marker_use.get(&mk).is_some_and(|u| u & MARK_ESCAPE != 0));
    assert_eq!(m.param_stores.get(&(0, 0)), Some(&vec![x]));
    // DECISIONS 176: a value of this unit stored into the argument goes to
    // the reader as a result does: the flow holds it, and the contract
    // check after the solve (`Gen::contract_check`, none in this bare
    // model) describes it or lets it escape alone.
    let vals = m.store_flows.values().find_map(|fl| fl.vals);
    assert!(
        vals.is_some_and(|n| m.nodes[n as usize].pts.to_sorted().contains(&o)),
        "the stored value is the flow's"
    );
    assert!(!m.fields.contains_key(&(mk, x)), "a field of the marker");
}

#[test]
fn an_argument_marker_leaves_beside_when_a_valued_member_links() {
    use crate::model::{Cons, FuncInfo, TokKind, NO_CTX, NO_FN};
    // DECISIONS 75: an argument marker stands for its members alone. With
    // none (or only described closed parameter markers) it adds no kinds to
    // arithmetic; once a member that stands for real values links in (`f`
    // is not a described closed function here), the arithmetic result it
    // skipped takes the object kinds (ToPrimitive), monotone.
    let mut m = copy_solver_model();
    let (this, ret, args) = (m.node_l("this"), m.node_l("ret"), m.node_l("args"));
    let token = m.tok(TokKind::Func(0, NO_CTX));
    m.funcs.push(FuncInfo {
        module: 0,
        func_id: 0,
        name: "f".into(),
        params: Vec::new(),
        rest: None,
        this,
        ret,
        args_all: args,
        has_arguments: false,
        token,
        ctor_of: None,
        lexical_this: false,
        no_summary: true,
        gen_step: false,
        captures: Vec::new(),
        forwards_super: false,
        parent: NO_FN,
        forwarded: false,
    });
    let mk = m.param_marker_tok(0, 0);
    // `r2` is a member of `r1` (an instance parameter passed on); `mk`
    // links into `r2` later, so `r1` leaves with it.
    let (n1, n2) = (m.node_l("p1"), m.node_l("p2"));
    let (src, dst) = (m.node_l("src"), m.node_l("dst"));
    let r1 = m.arg_marker(n1);
    let r2 = m.arg_marker(n2);
    m.link_marker(r2, r1);
    m.cons(src, Cons::Arith { dst, add: false });
    m.add_tok(src, r1);
    m.solve();
    assert_eq!(m.nodes[dst as usize].prim, 0, "beside: no kinds of its own");
    m.link_marker(mk, r2);
    m.solve();
    let k = m.nodes[dst as usize].prim;
    assert!(
        k & crate::model::prim::NUM != 0 && k & crate::model::prim::BIGINT != 0,
        "a valued member: the skipped result takes the object kinds ({k:#x})"
    );
}

/// A model with one function `f` whose parameter markers the tests make.
fn marker_model() -> crate::model::Model {
    use crate::model::{FuncInfo, TokKind, NO_CTX, NO_FN};
    let mut m = copy_solver_model();
    let (this, ret, args) = (m.node_l("this"), m.node_l("ret"), m.node_l("args"));
    let token = m.tok(TokKind::Func(0, NO_CTX));
    m.funcs.push(FuncInfo {
        module: 0,
        func_id: 0,
        name: "f".into(),
        params: Vec::new(),
        rest: None,
        this,
        ret,
        args_all: args,
        has_arguments: false,
        token,
        ctor_of: None,
        lexical_this: false,
        no_summary: true,
        gen_step: false,
        forwards_super: false,
        parent: NO_FN,
        captures: Vec::new(),
        forwarded: false,
    });
    m
}

#[test]
fn a_use_through_an_argument_marker_is_solve_work() {
    use crate::model::MARK_ESCAPE;
    // 113-CWL3: a use of an argument marker (DECISIONS 44), under a key
    // too (design §18.4), walks every marker it stands for. The walk is
    // solve work, so the work bound sees it: uncounted, checker.ts#u18
    // took 4.4x main's solve time for the same counted work.
    let mut m = copy_solver_model();
    // A parameter marker exists (else no use is recorded at all).
    m.param_marker.insert(u32::MAX - 1, (0, 0));
    let n0 = m.node_l("p0");
    let r = m.arg_marker(n0);
    let n = 64u64;
    for _ in 0..n {
        let ni = m.node_l("p");
        let ri = m.arg_marker(ni);
        m.link_marker(ri, r);
    }
    let a = m.atom("k");
    let k = m.keyed_reach_of(r, a);
    let w = m.work;
    m.mark_use(k, MARK_ESCAPE);
    assert!(
        m.work - w >= n,
        "a keyed use walked {n} members for {} work",
        m.work - w
    );
    let w = m.work;
    m.mark_use(r, MARK_ESCAPE);
    assert!(
        m.work - w >= n,
        "a use walked {n} members for {} work",
        m.work - w
    );
}

#[test]
fn a_keyed_use_reaches_every_marker_an_argument_marker_stands_for() {
    use crate::model::{MARK_ESCAPE, MARK_READ};
    // Design §18.4: argument marker `r2` stands for `r1` and `r3`, and `r1`
    // for `r2` (a recursion passing its argument on): `mk1` and `mk3`, the
    // markers they stand for, take every use of what `r1` and `r2` reach
    // under a key. `r1` is linked after the use of `inner` (the callee read
    // `p.inner` before the second caller's marker arrived), `r3` before.
    let mut m = marker_model();
    let (mk1, mk3) = (m.param_marker_tok(0, 0), m.param_marker_tok(0, 1));
    let (n1, n2, n3) = (m.node_l("p1"), m.node_l("p2"), m.node_l("p3"));
    let r1 = m.arg_marker(n1);
    let r2 = m.arg_marker(n2);
    let r3 = m.arg_marker(n3);
    let (inner, other) = (m.atom("inner"), m.atom("other"));
    m.link_marker(mk1, r1);
    m.link_marker(mk3, r3);
    m.link_marker(r3, r2);
    let k = m.keyed_reach_of(r2, inner);
    m.mark_use(k, MARK_ESCAPE);
    m.link_marker(r1, r2);
    m.link_marker(r2, r1);
    let k = m.keyed_reach_of(r1, other);
    m.mark_use(k, MARK_READ);
    m.settle_keyed_uses();
    for mk in [mk1, mk3] {
        assert_eq!(
            m.keyed_uses.get(&mk),
            Some(&vec![(inner, MARK_ESCAPE), (other, MARK_READ)]),
            "{:?}",
            m.keyed_uses
        );
    }
    // A keyed use is the contract's fact alone: the marker itself is not ⊤.
    assert!(!m.marker_is_top(mk1) && !m.marker_is_top(mk3));
}

#[test]
fn a_keyed_use_is_a_use_of_each_reach_marker_it_stands_for() {
    use crate::model::MARK_ESCAPE;
    // Design §18.4 with DECISIONS 42: what a parameter reaches arrives at a
    // shared body (`r` stands for the reach marker `pr`); the body lets
    // `p.k` escape, so `pr`, which reaches itself under every key, escaped
    // and reads as ⊤ during the solve. A member that links in after the
    // use (`pr2`) takes it too.
    let mut m = marker_model();
    let (mk, mk2) = (m.param_marker_tok(0, 0), m.param_marker_tok(0, 1));
    let (pr, pr2) = (m.marker_reach[&mk], m.marker_reach[&mk2]);
    let n0 = m.node_l("p");
    let r = m.arg_marker(n0);
    m.link_marker(pr, r);
    let a = m.atom("k");
    let k = m.keyed_reach_of(r, a);
    m.mark_use(k, MARK_ESCAPE);
    assert!(m.marker_is_top(pr), "the reach member escaped");
    assert!(!m.marker_is_top(pr2));
    m.link_marker(pr2, r);
    assert!(
        m.marker_is_top(pr2),
        "a late reach member takes the keyed use"
    );
}

#[test]
fn a_repeated_keyed_use_costs_one_step() {
    use crate::model::MARK_READ;
    // A keyed use costs what the use names: once `p.k` was read, reading
    // it again (each `p.k.x` reads it) is the step on its keyed reach
    // marker alone, not a walk to the argument marker and its members.
    let mut m = marker_model();
    let mk = m.param_marker_tok(0, 0);
    let n0 = m.node_l("p");
    let r = m.arg_marker(n0);
    for _ in 0..64 {
        let ni = m.node_l("pi");
        let ri = m.arg_marker(ni);
        m.link_marker(mk, ri);
        m.link_marker(ri, r);
    }
    let a = m.atom("k");
    let k = m.keyed_reach_of(r, a);
    m.mark_use(k, MARK_READ);
    let reps = 100;
    let w = m.work;
    for _ in 0..reps {
        m.mark_use(k, MARK_READ);
    }
    assert!(
        m.work - w <= reps,
        "{reps} repeated uses cost {} work",
        m.work - w
    );
}

#[test]
fn a_keyed_use_in_a_recursion_walks_each_member_once_per_bit() {
    use crate::model::MARK_ESCAPE;
    // The checker.ts#u18 / typescript.js createTypeChecker fan-out: a ring
    // of `n` argument markers (helpers passing their argument around a
    // recursion) each stand for every other; `keys` keys of it are used.
    // Each key costs its own use; the bits cross each member once, so the
    // work is keys + n, not keys x n. After the solve every parameter
    // marker in the ring has every key.
    let mut m = marker_model();
    let n = 32u32;
    let keys = 64u64;
    let rs: Vec<_> = (0..n)
        .map(|_| {
            let p = m.node_l("p");
            m.arg_marker(p)
        })
        .collect();
    let mks: Vec<_> = (0..n).map(|i| m.param_marker_tok(0, i)).collect();
    for i in 0..n as usize {
        m.link_marker(mks[i], rs[i]);
        m.link_marker(rs[(i + 1) % n as usize], rs[i]);
    }
    let w = m.work;
    let atoms: Vec<_> = (0..keys).map(|i| m.atom(&format!("k{i}"))).collect();
    for &a in &atoms {
        let k = m.keyed_reach_of(rs[0], a);
        m.mark_use(k, MARK_ESCAPE);
    }
    let spent = m.work - w;
    assert!(
        spent <= 3 * keys + 2 * n as u64 + 4,
        "{keys} keys on a ring of {n} cost {spent} work"
    );
    m.settle_keyed_uses();
    for mk in mks {
        let v = m.keyed_uses.get(&mk).map_or(0, |v| v.len());
        assert_eq!(v as u64, keys, "a ring member has {v} of {keys} keys");
    }
}

#[test]
fn a_protocol_touch_of_an_argument_marker_reads_nothing() {
    use crate::model::{Protocol, MARK_ESCAPE, MARK_TOUCH};
    // A marker has no accessors: a touch reads nothing off it (a read node
    // per touch would take a fresh marker each, `Model::crosses`, and JSON
    // would touch that one in turn without end). JSON's nested values are
    // what the marker reaches, touched; `toJSON` is what it reaches under
    // that key, called: an escape (DECISIONS 111).
    let mut m = copy_solver_model();
    m.param_marker.insert(u32::MAX - 1, (0, 0));
    let n0 = m.node_l("p0");
    let r = m.arg_marker(n0);
    let nodes = m.nodes.len();
    for p in [Protocol::Json, Protocol::IterResult, Protocol::ToPrimitive] {
        m.implicit(r, 0, p);
    }
    let reads = (nodes..m.nodes.len())
        .filter(|&n| m.labels[n].starts_with("implicit"))
        .count();
    assert_eq!(reads, 0, "a touch of a marker made read or call nodes");
    let reach = m.reach_of(r);
    assert!(m
        .marker_use
        .get(&reach)
        .is_some_and(|&u| u & MARK_TOUCH != 0));
    let a = m.atom("toJSON");
    let tj = m.marker_load(r, Some(a));
    assert!(m.marker_use.get(&tj).is_some_and(|&u| u & MARK_ESCAPE != 0));
}
