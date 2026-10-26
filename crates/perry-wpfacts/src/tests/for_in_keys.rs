//! FVA 4.3b: a for-in over a literal whose keys are exactly its birth keys
//! runs its body once per key, the loop variable that constant key, so the
//! keyed read, store and `defineProperty` in it are those of that key. Any
//! other object (a key added after birth, a computed key, an enumerable key
//! the program puts on `Object.prototype`) runs the body with every key.
//!
//! esbuild's `__export(target, { k: () => f })` then gives `target` one
//! getter per key: a read of `target.k` runs only `k`'s getter, so only `f`
//! is called through it. Each function here reads the field of its own
//! class from its parameter: a run with every key puts every getter on
//! every key, and each function would see the other classes too.

use super::*;

/// `__export(ns, literal)` over three getters, each key read and called.
fn export_src(extra: &str, literal: &str) -> String {
    format!(
        r#"
        var __defProp = Object.defineProperty;
        var __export = (target: any, all: any) => {{
          for (var name in all)
            __defProp(target, name, {{ get: all[name], enumerable: true }});
        }};
        class A {{ a = 1; }}
        class B {{ b = 2; }}
        class C {{ c = 3; }}
        function fa(x: any) {{ return x.a; }}
        function fb(x: any) {{ return x.b; }}
        function fc(x: any) {{ return x.c; }}
        {extra}
        var ns: any = {{}};
        __export(ns, {literal});
        function useA(v: any) {{ const g = ns.a; return g(v); }}
        console.log(useA(new A()), ns.b(new B()), ns.c(new C()));
        "#
    )
}

const THREE: &str = "{ a: () => fa, b: () => fb, c: () => fc }";

/// The classes the reads of `key` (`x.a` in `fa`, …) see, other than the
/// literals and `ns`.
fn read_classes(pf: &ProgramFacts, key: &str) -> Vec<String> {
    let mut v: Vec<String> = pf
        .sites
        .iter()
        .filter(|s| s.key == key && !s.is_call)
        .flat_map(|s| s.receivers.iter())
        .filter(|r| !r.contains("__AnonShape"))
        .map(|r| r.rsplit(':').next().unwrap_or(r).to_string())
        .collect();
    v.sort();
    v.dedup();
    v
}

/// Every function is called only with its own class.
fn per_key(pf: &ProgramFacts) -> bool {
    read_classes(pf, "a") == ["A"]
        && read_classes(pf, "b") == ["B"]
        && read_classes(pf, "c") == ["C"]
}

#[test]
fn an_export_literal_runs_the_body_once_per_key() {
    let pf = facts(&export_src("", THREE));
    assert!(per_key(&pf), "{}", pf.dump_text());
    // The target holds the three keys as accessor keys, nothing HARD.
    let ns = pf
        .templates
        .iter()
        .find(|t| t.kind == "litclass" && t.fields.iter().any(|f| f.key == "a" && !f.data_field))
        .unwrap_or_else(|| panic!("the target's layout\n{}", pf.dump_text()));
    assert!(
        ns.hard.is_none() && ns.escape.is_none(),
        "{}",
        pf.dump_text()
    );
}

#[test]
fn an_enumerable_key_on_object_prototype_runs_the_body_with_every_key() {
    // for-in also visits `extra`, inherited from Object.prototype: the
    // literal's own keys are not all the loop's keys.
    let pf = facts(&export_src(
        "(Object.prototype as any).extra = () => fc;",
        THREE,
    ));
    assert!(!per_key(&pf), "{}", pf.dump_text());
}

#[test]
fn a_computed_key_runs_the_body_with_every_key() {
    let pf = facts(&export_src(
        "const kb = [\"b\"].join(\"\");",
        "{ a: () => fa, [kb]: () => fb, c: () => fc }",
    ));
    assert!(!per_key(&pf), "{}", pf.dump_text());
}

#[test]
fn a_key_added_after_birth_runs_the_body_with_every_key() {
    let pf = facts(&export_src(
        "const lit: any = { a: () => fa, b: () => fb }; lit.c = () => fc;",
        "lit",
    ));
    assert!(!per_key(&pf), "{}", pf.dump_text());
}

#[test]
fn a_literal_that_escapes_after_the_runs_runs_the_body_with_every_key() {
    // Shared bodies run every store before the runs per key are decided, so
    // what only a run's instance can change (a literal escaping, turning
    // HARD, gaining a key) is checked again once the worklist is empty
    // (`settle_for_in`): here the literal escapes after the solve, as
    // unknown code could add keys to it.
    let src = export_src("", THREE);
    let module = perry_parser::parse_typescript(&src, "t.ts").expect("parses");
    let hir = perry_hir::lower_module(&module, "t", "t.ts").expect("lowers");
    let mut g = crate::gen::Gen::new(vec![("t.ts", &hir)], false, false);
    g.register_all();
    g.link_exports();
    g.walk_all();
    g.feed_parent_values();
    g.m.summarize_and_replay();
    g.m.solve();
    let &(call, ft, t) = g.m.for_in_runs.first().expect("a run per key");
    assert!(!g.m.for_in_any.contains(&(call, ft)));
    assert!(!g.m.settle_for_in(), "nothing changed since the runs");
    let r = g.m.reason("test: unknown code");
    g.m.escape(t, r);
    assert!(
        g.m.settle_for_in(),
        "the escaped literal runs the body again"
    );
    assert!(g.m.for_in_any.contains(&(call, ft)));
}

#[test]
fn a_keyed_store_writes_the_one_key_of_each_run() {
    // `out[k] = src[k]` copies key by key: out.a holds only what src.a does.
    let src = r#"
        class A { a = 1; }
        class B { b = 2; }
        function copy(src: any) {
          const out: any = { a: null, b: null };
          for (const k in src) out[k] = src[k];
          return out;
        }
        const o = copy({ a: new A(), b: new B() });
        function rd(x: any) { return x.a.a; }
        console.log(rd(o), o.b.b);
    "#;
    let pf = facts(src);
    let b_in_a = pf
        .sites
        .iter()
        .filter(|s| s.key == "a" && !s.is_call)
        .any(|s| s.receivers.iter().any(|r| r.ends_with(":B")));
    assert!(!b_in_a, "out.a holds only an A\n{}", pf.dump_text());
}

#[test]
fn per_unit_is_never_stronger() {
    // Per unit the literal may cross a unit edge: whatever it proves, the
    // whole-program solve proves too.
    let src = export_src("", THREE);
    let module = perry_parser::parse_typescript(&src, "t.ts").expect("parses");
    let hir = perry_hir::lower_module(&module, "t", "t.ts").expect("lowers");
    let u = crate::compute_units(&[("t.ts".to_string(), &hir)], &FactsConfig::default());
    let w = facts(&src);
    for key in ["a", "b", "c"] {
        // A read whose receiver is unknown there claims nothing.
        if u.sites.iter().any(|s| s.key == key && !s.is_call && s.top) {
            continue;
        }
        let (ru, rw) = (read_classes(&u, key), read_classes(&w, key));
        assert!(
            rw.iter().all(|c| ru.contains(c)),
            "{key}: per unit {ru:?}, whole program {rw:?}\n{}",
            u.dump_text()
        );
    }
}
