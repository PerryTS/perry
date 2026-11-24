//! GLOB2: the global object is an ordinary object for the program stores
//! (ECMA-262 9.3, 19). G1: `new K(...)` of a global the program stored
//! over constructs the stored value too (13.3.5.1); globals a native reads
//! by name keep the sink. G2: a store under an unknown string key writes
//! only string-keyed properties of the global object.
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

/// RED (G1): `new CustomEvent(...)` after the program stored its own
/// class as global `CustomEvent`: the stored class is constructed too
/// (its constructor body is live and reads the detail it was given).
#[test]
fn gl2_g1_new_named_constructs_stored() {
    let src = "class N { v = 1; }
        class MyCE { type: string; constructor(t: string, o: any) { this.type = t; const d = o.detail; console.log(/*B*/ d.v); } }
        const g = globalThis as Record<string, unknown>;
        if (process.argv[2] === \"x\") g[\"CustomEvent\"] = MyCE;
        const n = new N();
        const e: any = new CustomEvent(\"x\", { detail: n });
        console.log(e.type);";
    let (v, dump) = verdict(src, "v", "/*B*/");
    assert!(
        proven(&v) && !format!("{v:?}").contains("Dead"),
        "MyCE constructed by `new CustomEvent` (live, proven), got {v:?}\n{dump}"
    );
    assert!(!held(src), "realm not held\n{dump}");
}

/// RED (G1): the three.js devtools shape: a method constructs
/// `new CustomEvent(type, { detail: this })`; the installed class sees
/// `this` and the realm is not held.
#[test]
fn gl2_g1_devtools_shape() {
    let src = "class MyCE { detail: any; constructor(t: string, o: any) { this.detail = o.detail; console.log(/*B*/ this.detail.v); } }
        class Obj3 { v = 1; hook(): any { return new CustomEvent(\"observe\", { detail: this }); } }
        const g: any = globalThis;
        g[\"CustomEvent\"] = MyCE;
        const o = new Obj3();
        const e: any = o.hook();
        console.log(/*A*/ o.v, e);";
    let (v, dump) = verdict(src, "v", "/*B*/");
    assert!(
        proven(&v) && !format!("{v:?}").contains("Dead"),
        "this.detail is o, got {v:?}\n{dump}"
    );
    let (v, dump) = verdict(src, "v", "/*A*/");
    assert!(proven(&v), "o.v proven, got {v:?}\n{dump}");
    assert!(!held(src), "realm not held\n{dump}");
}

/// RED (G1, exactness of the W3GLOB rule): globals a native reads by name
/// and hands on (module exports `url.URL`, `buffer.Blob`, `util.TextEncoder`,
/// stream_web classes) keep the sink: the stored value reaches the native.
#[test]
fn gl2_g1_native_read_names_keep_sink() {
    // // owner 11-24: only names perry-runtime reads at run time (derived; today: onmessage) make a global store escape; URL is not one of them.
    assert!(
        !held(&program(
            "g[\"URL\"] = class U { constructor(s: any) { console.log(s); } };"
        )),
        "URL: the runtime does not read it, realm not held"
    );
    for (name, store) in [
        ("TextEncoder", "g[\"TextEncoder\"] = class T {};"),
        ("Blob", "g[\"Blob\"] = class B {};"),
        ("ReadableStream", "g[\"ReadableStream\"] = class R {};"),
    ] {
        // owner 11-24: only names perry-runtime reads at run time make a global store escape; since 72ce74b350/ec0f0972b3/06245f1d0e the runtime uses the birth <NAME> (here {name}), so a program store under <NAME> stays the program's.
        assert!(
            !held(&program(store)),
            "{store}: the runtime uses the birth constructor, realm not held"
        );
    }
}

/// KEEP: prototype stores, a key that may be a Symbol, namespace and
/// error-class globals (natives read `Error` by name; `Promise` statics
/// bind to the intrinsic), and the special globals.
#[test]
fn gl2_keep() {
    for store in [
        "(Array.prototype as any).zz = (x: any) => x;",
        "(Object.prototype as any).zz = (x: any) => x;",
        "g[process.argv[2] === \"a\" ? Symbol.iterator as any : \"k\"] = (x: any) => x;",
    ] {
        // owner 11-24: E20 carve-out dropped; the stored value escapes to the runtime per its contract, the realm is not held.
        assert!(!held(&program(store)), "{store}: the value escapes, the realm is not held");
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
    for store in [
        "g[\"undefined\"] = 1;",
        "g[\"globalThis\"] = { a: 1 };",
        "delete (globalThis as any).fetch;",
    ] {
        let _ = verdict(&program(store), "v", "/*A*/");
        let _ = held(&program(store));
    }
}
