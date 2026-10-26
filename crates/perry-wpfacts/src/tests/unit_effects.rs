//! Per-unit mode: the declaration cut follows data dependencies (NSS). A
//! store through a binding (`ns.k = f`) goes into the object the binding
//! holds: a store effect carrying the stored value's contract (decision 17).
//! A write of another unit's module-scope binding (`cur = v`) is the
//! binding's write effect, carrying the written value's contract. A class
//! capture refresh adds no state to its class. None of them joins units,
//! and the per-unit facts never claim more than whole-program facts.

use super::*;
use crate::decl_units::Item;

fn hir(src: &str) -> perry_hir::Module {
    let module = perry_parser::parse_typescript(src, "t.ts").expect("parses");
    perry_hir::lower_module(&module, "t", "t.ts").expect("lowers")
}

fn units(src: &str, cfg: &FactsConfig) -> ProgramFacts {
    let h = hir(src);
    crate::compute_units(&[("t.ts".to_string(), &h)], cfg)
}

fn whole(src: &str) -> ProgramFacts {
    let h = hir(src);
    crate::compute(&[("t.ts".to_string(), &h)], &FactsConfig::default())
}

fn cut(src: &str) -> (perry_hir::Module, Vec<Vec<Item>>) {
    let h = hir(src);
    let h = crate::decl_units::run_once_flat(&h).unwrap_or(h);
    let u = crate::decl_units::decl_units(&h);
    (h, u)
}

fn proven(pf: &ProgramFacts, key: &str) -> bool {
    let s: Vec<_> = pf.sites.iter().filter(|s| s.key == key).collect();
    !s.is_empty()
        && s.iter()
            .all(|s| matches!(s.verdict, Verdict::T0Mono | Verdict::T0Set))
}

/// The prims a field of the literal with birth keys `keys` may hold, or
/// `None` when it may hold anything.
fn field_prims(pf: &ProgramFacts, keys: &[&str], key: &str) -> Option<u16> {
    let t = pf
        .templates
        .iter()
        .find(|t| t.birth_keys == keys)
        .unwrap_or_else(|| panic!("no literal {keys:?}\n{}", pf.dump_text()));
    let f = t
        .fields
        .iter()
        .find(|f| f.key == key)
        .unwrap_or_else(|| panic!("no field {key}\n{}", pf.dump_text()));
    (!f.kind.top).then_some(f.kind.prims)
}

/// Read and call sites the per-unit run decides more strongly than the
/// whole-program run: per site (offset, key), a per-unit receiver set that
/// misses a whole-program receiver, or a whole-program ⊤ beside a per-unit
/// known receiver.
fn sites_stronger(u: &ProgramFacts, w: &ProgramFacts) -> Vec<String> {
    let mut out = Vec::new();
    for su in &u.sites {
        let Some(sw) = w
            .sites
            .iter()
            .find(|s| s.byte_offset == su.byte_offset && s.key == su.key)
        else {
            continue;
        };
        if su.top {
            continue;
        }
        if sw.top || sw.receivers.iter().any(|r| !su.receivers.contains(r)) {
            out.push(format!(
                "{}@{} {:?} vs {:?}",
                su.key, su.byte_offset, su.receivers, sw.receivers
            ));
        }
    }
    out
}

fn assert_sound(src: &str) -> ProgramFacts {
    let u = units(src, &FactsConfig::default());
    let w = whole(src);
    let s = sites_stronger(&u, &w);
    assert!(
        s.is_empty(),
        "per-unit stronger: {s:?}\n{}\n---\n{}",
        u.dump_text(),
        w.dump_text()
    );
    u
}

const STR: u16 = crate::model::prim::STR;

#[test]
fn a_store_through_a_binding_joins_nothing() {
    // The object a binding holds, not the binding, is written: each store
    // statement is a unit of its own. A class's static store is the class's
    // own state: one unit with it.
    let (_, u) = cut("const ns: any = {}; console.log(ns); ns.a = 1; ns['b'] = 'x';");
    assert_eq!(u.len(), 4, "{u:?}");
    let (_, u) = cut("class C { v = 1; } (C as any).k = 1;");
    assert_eq!(u.len(), 1, "{u:?}");
}

#[test]
fn a_namespace_member_stored_by_another_unit_keeps_its_function() {
    // `ns.g = g` is a store effect into `ns`'s literal: it carries `g`'s
    // contract, so the field holds `g` in the reader's unit, not ⊤. The
    // call `ns.g(o)` is proven in neither mode: `g` is no birth key of the
    // literal, so the read has no slot (it is `undefined` before the
    // store), and a call is proven only through the read rule's lookup.
    let src = r#"
        const ns: any = {};
        function g(x: any) { return x.k; }
        ns.g = g;
        function use(o: any) { return ns.g(o); }
        console.log(use({ k: 1 }));
        "#;
    let w = whole(src);
    assert!(
        refused(&w, "g", "key not a definite birth field"),
        "{}",
        w.dump_text()
    );
    let u = assert_sound(src);
    assert!(u.units.len() >= 4, "{:?}", u.units);
    assert!(holds_fn(&u, "g"), "{}", u.dump_text());
    assert!(
        refused(&u, "g", "key not a definite birth field"),
        "{}",
        u.dump_text()
    );
}

#[test]
fn another_units_store_is_held_by_the_owner() {
    // Soundness: `set` (its own unit) stores a string into `ns.v`; the
    // owner's field holds it, so the literal `get` builds from it may hold a
    // string.
    let src = r#"
        const ns: any = { v: 1 };
        function set() { ns.v = 's'; }
        function get() { return { w: ns.v }; }
        set();
        console.log(get().w);
        "#;
    let u = assert_sound(src);
    assert!(u.units.len() >= 3, "{:?}", u.units);
    let p = field_prims(&u, &["w"], "w");
    assert!(p.is_none_or(|p| p & STR != 0), "{p:?}\n{}", u.dump_text());
}

#[test]
fn a_write_of_another_units_binding_joins_nothing() {
    // Two functions writing one `let` are units of their own, each ordered
    // after the binding's unit as its readers are.
    let (_, u) = cut("let k = 0; function inc() { k++; } function dec() { k--; }");
    assert_eq!(u.len(), 3, "{u:?}");
}

#[test]
fn another_units_write_is_held_by_the_binding() {
    // Soundness: `set` writes a string into `k`; `k`'s unit holds it, so
    // `get`'s literal may hold a string.
    let src = r#"
        let k: any = 0;
        function set() { k = 's'; }
        function get() { return { w: k }; }
        set();
        console.log(get().w);
        "#;
    let u = assert_sound(src);
    assert!(u.units.len() >= 3, "{:?}", u.units);
    let p = field_prims(&u, &["w"], "w");
    assert!(p.is_none_or(|p| p & STR != 0), "{p:?}\n{}", u.dump_text());
}

#[test]
fn a_written_object_is_read_through_its_contract() {
    // Soundness: `w` writes its own literal into `cur`; `r` (its own unit)
    // reads `cur.x`. Its receivers are both layouts, never `o`'s alone.
    let src = r#"
        const o = { x: 1 };
        let cur: any = o;
        function w() { cur = { y: 2, x: 's' }; }
        function r() { return cur.x; }
        w();
        console.log(r());
        "#;
    let u = assert_sound(src);
    let s: Vec<_> = u.sites.iter().filter(|s| s.key == "x").collect();
    assert!(!s.is_empty(), "{}", u.dump_text());
    assert!(
        s.iter().all(|s| s.top || s.receivers.len() >= 2),
        "{s:?}\n{}",
        u.dump_text()
    );
}

#[test]
fn a_namespace_object_made_by_its_first_write_keeps_its_members() {
    // TypeScript's namespace: `var eu;` declares the binding, the wrapper's
    // `eu || (eu = {})` makes its object (a write effect of another unit,
    // carrying the literal's layout contract), and `eu2.f = …` stores the
    // member (a store effect). The reader's field holds the function (not
    // ⊤); its call is refused as whole program refuses it: `f` is written
    // after birth, so the read has no slot.
    let src = r#"
        var eu: any;
        ((eu2: any) => { eu2.f = (m: any) => m; })(eu || (eu = {}));
        function a(x: any) { return eu.f(x); }
        console.log(a("s"));
        "#;
    let u = assert_sound(src);
    assert!(u.units.len() >= 3, "{:?}", u.units);
    assert!(holds_fn(&u, "f"), "{}", u.dump_text());
    assert!(
        refused(&u, "f", "key not a definite birth field"),
        "{}",
        u.dump_text()
    );
    let w = whole(src);
    assert!(
        refused(&w, "f", "key not a definite birth field"),
        "{}",
        w.dump_text()
    );
}

#[test]
fn a_capture_refresh_is_no_state_of_its_class() {
    // A class expression of a run-once body captures `label`, assigned after
    // the class: the assignment's refresh re-stores the capture. The model's
    // one value of `label` is already in the class's environment through the
    // evaluation, so the refresh is a unit apart from the class, and the
    // method still sees the string.
    let src = r#"
        (function () {
          let label: any = 1;
          const K = class K { get() { return { v: label }; } };
          label = 's';
          console.log(new K().get().v);
        })();
        "#;
    let (h, u) = cut(src);
    let refresh = |it: &Item| match *it {
        Item::Init(k) => {
            let mut found = false;
            perry_hir::tdz_check::for_each_stmts_expr(std::slice::from_ref(&h.init[k]), &mut |e| {
                found |= matches!(e, perry_hir::Expr::RefreshClassExprCaptures { .. })
            });
            found
        }
        _ => false,
    };
    let class_unit = u
        .iter()
        .position(|items| items.iter().any(|it| matches!(it, Item::Class(_))))
        .expect("a class unit");
    assert!(u.iter().flatten().any(refresh), "no refresh in {u:?}");
    assert!(
        !u[class_unit].iter().any(refresh),
        "refresh joined its class: {u:?}"
    );
    let pf = assert_sound(src);
    let p = field_prims(&pf, &["v"], "v");
    assert!(p.is_none_or(|p| p & STR != 0), "{p:?}\n{}", pf.dump_text());
}

#[test]
fn an_aborted_writer_writes_anything() {
    // The writer `W` is over the bound: it claims nothing, so the binding it
    // writes holds any value. (Called twice: a body run once is module
    // scope for the cut, N5.)
    let mut src =
        String::from("let cur: any = 1;\nfunction W(n: any) {\n  const xs: any[] = [];\n");
    for i in 0..400 {
        src.push_str(&format!("  xs.push({{ k{i}: n, v: [n, {i}] }});\n"));
    }
    src.push_str("  cur = 2;\n  return xs;\n}\n");
    src.push_str("function get() { return { w: cur }; }\nW(1);\nW(2);\nconsole.log(get().w);\n");
    let free = units(&src, &FactsConfig::default());
    let p = field_prims(&free, &["w"], "w");
    assert!(
        p.is_some_and(|p| p & STR == 0),
        "{p:?}\n{}",
        free.dump_text()
    );
    let nodes: Vec<u64> = free
        .dump_text()
        .lines()
        .filter(|l| l.starts_with("# unit "))
        .map(|l| {
            l.split("nodes=")
                .nth(1)
                .unwrap()
                .split_whitespace()
                .next()
                .unwrap()
                .parse()
                .unwrap()
        })
        .collect();
    let big = *nodes.iter().max().unwrap();
    let small = nodes.iter().copied().filter(|&n| n < big).max().unwrap();
    assert!(big > 4 * small, "{nodes:?}");
    let cfg = FactsConfig {
        max_pts: Some(8 * (big / 2)),
        ..FactsConfig::default()
    };
    let u = units(&src, &cfg);
    let dump = u.dump_text();
    // The guard held: exactly the writer's unit aborted.
    let aborted = dump
        .lines()
        .filter(|l| l.starts_with("# unit ") && l.contains("aborted=true"))
        .count();
    assert_eq!(aborted, 1, "{dump}");
    assert_eq!(field_prims(&u, &["w"], "w"), None, "{dump}");
}

#[test]
fn what_a_written_object_holds_leaves_with_it() {
    // Soundness: `w` writes its literal holding `inner` into `cur`; `r`
    // stores a string through it. `inner` left `w`'s unit with the literal
    // (a published value's fields leave), so `w`'s own read of `inner.v`
    // is no number alone.
    let src = r#"
        let cur: any = 0;
        function w() { const inner = { v: 1 }; cur = { inner }; return inner; }
        function r() { cur.inner.v = 's'; }
        function main() { const i = w(); r(); console.log({ z: i.v }.z); }
        main(); main();
        "#;
    let u = assert_sound(src);
    let p = field_prims(&u, &["z"], "z");
    assert!(p.is_none_or(|p| p & STR != 0), "{p:?}\n{}", u.dump_text());
}

#[test]
fn what_a_stored_object_holds_leaves_with_it() {
    // The same through a store effect: `w` stores its literal holding
    // `inner` into `ns.o`.
    let src = r#"
        const ns: any = { o: 0 };
        function w() { const inner = { v: 1 }; ns.o = { inner }; return inner; }
        function r() { ns.o.inner.v = 's'; }
        function main() { const i = w(); r(); console.log({ z: i.v }.z); }
        main(); main();
        "#;
    let u = assert_sound(src);
    let p = field_prims(&u, &["z"], "z");
    assert!(p.is_none_or(|p| p & STR != 0), "{p:?}\n{}", u.dump_text());
}

/// `m` over the bound (a body called twice, so not module scope, N5) calling
/// `a`'s exported functions.
fn aborted_caller() -> String {
    let mut s = String::from(
        "import { get, own, Box } from './a';\nfunction big(n: any) {\n  const xs: any[] = [];\n",
    );
    for i in 0..400 {
        s.push_str(&format!("  xs.push({{ k{i}: n, v: [n, {i}] }});\n"));
    }
    s.push_str("  xs.push(get(xs[0]));\n  xs.push(own.call(xs[1]));\n  xs.push(new Box(1).take());\n  return xs;\n}\nbig(1);\nbig(2);\n");
    s
}

const GRANTED: &str = "export function get(o: any) { return o.k; }\nexport function own(this: any) { return this.k; }\nexport class Box { v: any; constructor(v: any) { this.v = v; } take() { return this.v; } }\n";

fn hirs(mods: &[(&str, &str)]) -> Vec<(String, perry_hir::Module)> {
    mods.iter()
        .map(|(name, src)| {
            let path = format!("{name}.ts");
            let module = perry_parser::parse_typescript(src, &path).expect("parses");
            let mut hir = perry_hir::lower_module(&module, name, &path).expect("lowers");
            for imp in &mut hir.imports {
                if let Some(n) = imp.source.strip_prefix("./") {
                    imp.resolved_path = Some(format!("{n}.ts"));
                }
            }
            (path, hir)
        })
        .collect()
}

#[test]
fn an_aborted_unit_reports_what_no_contract_grants() {
    // Q1 (XUC, 116-CWL3's rule): an aborted unit calling `get` with ⊤
    // arguments does what `get`'s contract grants any caller; it may store
    // into the `Box` instances it holds, and call `own` on a receiver of its
    // own, which no contract grants: `Box` and `own` are broken, `get` is
    // not.
    let m = aborted_caller();
    let h = hirs(&[("a", GRANTED), ("m", m.as_str())]);
    let list: Vec<(String, &perry_hir::Module)> = h.iter().map(|(p, m)| (p.clone(), m)).collect();
    let free = crate::compute_units(&list, &FactsConfig::default());
    let nodes: Vec<u64> = free
        .dump_text()
        .lines()
        .filter(|l| l.starts_with("# unit "))
        .map(|l| {
            l.split("nodes=")
                .nth(1)
                .unwrap()
                .split_whitespace()
                .next()
                .unwrap()
                .parse()
                .unwrap()
        })
        .collect();
    let big = *nodes.iter().max().unwrap();
    let cfg = FactsConfig {
        max_pts: Some(8 * (big / 2)),
        ..FactsConfig::default()
    };
    let u = crate::compute_units(&list, &cfg);
    let dump = u.dump_text();
    // The guard held: exactly the caller's unit aborted.
    let aborted: Vec<&str> = dump
        .lines()
        .filter(|l| l.starts_with("# unit ") && l.contains("aborted=true"))
        .collect();
    assert_eq!(aborted.len(), 1, "{dump}");
    assert!(aborted[0].starts_with("# unit m.ts#"), "{dump}");
    assert!(u.broken.iter().any(|d| d == "a.ts:Box"), "{:?}", u.broken);
    assert!(
        !u.broken.iter().any(|d| d == "fn:a.ts:get"),
        "{:?}",
        u.broken
    );
    assert!(
        u.broken.iter().any(|d| d == "fn:a.ts:own"),
        "{:?}",
        u.broken
    );
    // Soundness: `get` reads `o.k` of anything; whole-program agrees.
    let w = crate::compute(&list, &FactsConfig::default());
    let s = sites_stronger(&u, &w);
    assert!(s.is_empty(), "{s:?}\n{dump}");
}
