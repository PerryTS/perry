//! A program value stored on the global object under the name of a global
//! function the realm declares (`g["fetch"] = f`): the global object is an
//! ordinary object (ECMA-262 9.3, 19): the stored value becomes global
//! `fetch`, which program reads of the name see beside the realm function.
//! No native reads such a global by name (the runtime answers constructors
//! from the realm intrinsics, `builtin_constructor_value`), so the value
//! reaches no unknown code and the realm is not held. Prototype stores, a
//! store under an unknown key and constructor / namespace globals keep
//! the sink of today.
use super::*;

fn facts_of(src: &str) -> ProgramFacts {
    let path = "main.ts".to_string();
    let module = perry_parser::parse_typescript(src, &path).expect("parses");
    let hir = perry_hir::lower_module(&module, "main", &path).expect("lowers");
    let list: Vec<(String, &perry_hir::Module)> = vec![(path, &hir)];
    let mut cfg = FactsConfig::default();
    cfg.max_work.get_or_insert(50_000_000);
    crate::compute(&list, &cfg)
}

/// The verdict of the first site `.key` after `marker`, and the dump.
fn verdict(src: &str, key: &str, marker: &str) -> (Verdict, String) {
    let pf = facts_of(src);
    let at = src.find(marker).expect("marker") as u32;
    let s = pf
        .sites
        .iter()
        .filter(|s| s.key == key && s.byte_offset >= at)
        .min_by_key(|s| s.byte_offset)
        .unwrap_or_else(|| panic!("site .{key} after {marker:?} in\n{}", pf.dump_text()));
    (s.verdict.clone(), pf.dump_text())
}

/// Does unknown code hold the realm after the solve (`Model::realm_held`)?
fn held(src: &str) -> bool {
    let ast = Box::leak(Box::new(
        perry_parser::parse_typescript(src, "t.ts").unwrap(),
    ));
    let hir = Box::leak(Box::new(perry_hir::lower_module(ast, "t", "t.ts").unwrap()));
    let mut g = crate::gen::Gen::new(vec![("t.ts", &*hir)], false, false);
    g.m.max_work = 50_000_000;
    g.register_all();
    g.link_exports();
    g.walk_all();
    g.feed_parent_values();
    g.m.summarize_and_replay();
    g.m.solve();
    g.m.realm_held
}

fn proven(v: &Verdict) -> bool {
    !matches!(v, Verdict::No(_))
}

/// `store` runs, then a builtin method call hands `n` to native code: with
/// the realm held the call is unknown and `n` escapes.
fn program(store: &str) -> String {
    format!(
        "class N {{ v = 1; }}
        const g = globalThis as Record<string, unknown>;
        {store}
        const n = new N();
        [n].forEach((x) => console.log(x.v));
        console.log(/*A*/ n.v);"
    )
}

/// RED: fx8b (bloom/web fetch.ts): a program function stored as `fetch`.
#[test]
fn w3g_global_fn_store_holds_nothing() {
    for store in [
        "g[\"fetch\"] = (u: string): string => u + String(process.argv[2]);",
        "(globalThis as any).fetch = (u: string): string => u + process.argv[2];",
        "g[\"setTimeout\"] = (f: () => void): number => { f(); return 0; };",
    ] {
        let (v, dump) = verdict(&program(store), "v", "/*A*/");
        assert!(proven(&v), "{store}: n.v proven, got {v:?}\n{dump}");
        assert!(
            !held(&program(store)),
            "{store}: no E20 sink, realm not held\n{dump}"
        );
    }
}

/// RED: a global function replaced by an argv choice and called after:
/// the call site sees the program function too (it receives `n`).
#[test]
fn w3g_replaced_global_fn_is_called() {
    let src = "class N { v = 1; }
        class M { w = 2; }
        function mine(o: any): any { console.log(/*B*/ o.v); return new M(); }
        const g = globalThis as Record<string, unknown>;
        if (process.argv[2] === \"x\") g[\"fetch\"] = mine;
        const n = new N();
        const r: any = (fetch as any)(n);
        console.log(r);";
    let (v, dump) = verdict(src, "v", "/*B*/");
    assert!(
        proven(&v) && !matches!(format!("{v:?}").as_str(), s if s.contains("Dead")),
        "o is n (a live, proven read), got {v:?}\n{dump}"
    );
    assert!(!held(src), "no E20 sink, realm not held\n{dump}");
}

/// RED: a new global class installed on the global object and constructed
/// through a global read elsewhere.
#[test]
fn w3g_new_global_class() {
    let src = "class El { v = 1; }
        const g = globalThis as Record<string, unknown>;
        g[\"MyElement\"] = El;
        const C: any = (globalThis as any).MyElement;
        const e = new C();
        [e].forEach((x: any) => console.log(x.v));
        console.log(/*A*/ e.v);";
    let (v, dump) = verdict(src, "v", "/*A*/");
    assert!(proven(&v), "e.v proven, got {v:?}\n{dump}");
}

/// KEEP: stores on builtin prototypes, under an unknown key on the global
/// object under a Symbol-able key, and on a constructor global stay sinks.
/// (An unknown String key on the global object is GLOBK G2: tests/gk.rs.)
#[test]
fn w3g_keep_sinks() {
    for store in [
        "(Array.prototype as any).zz = (x: any) => x;",
        "(Object.prototype as any).zz = (x: any) => x;",
        "(Function.prototype as any).zz = (x: any) => x;",
    ] {
        assert!(held(&program(store)), "{store}: still a sink, realm held");
    }
    // owner 11-24: only names perry-runtime reads at run time make a global store escape; since 72ce74b350/ec0f0972b3/06245f1d0e the runtime uses the birth Error, so a program store under Error stays the program's.
    assert!(
        !held(&program(
            "g[\"Error\"] = class E { constructor(m: any) { console.log(m); } };"
        )),
        "Error: the runtime uses the birth Error, realm not held"
    );
    // // owner 11-24: only names perry-runtime reads at run time (derived; today: onmessage) make a global store escape; Promise is not one of them.
    assert!(
        !held(&program(
            "g[\"Promise\"] = class P { constructor(f: any) { f(() => 0, () => 0); } };"
        )),
        "Promise: not read at run time, realm not held"
    );
}

/// KEEP: `globalThis.globalThis = x` and the non-writable globals
/// (`undefined`, `NaN`, `Infinity`, ECMA-262 19.1: the store fails).
#[test]
fn w3g_keep_special_globals() {
    for store in [
        "g[\"globalThis\"] = { a: 1 };",
        "g[\"undefined\"] = 5;",
        "g[\"NaN\"] = 5;",
        "g[\"Infinity\"] = 5;",
    ] {
        let _ = verdict(&program(store), "v", "/*A*/");
        let _ = held(&program(store));
    }
}
