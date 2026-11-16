//! GLOBK (GLOB2 G2): a store on the global object under a String key the
//! solver cannot name writes one string-named property of the global
//! object and nothing else (ECMA-262 7.1.19, 10.1.9, 19.1): every global
//! binding read sees {its original} ∪ {the values stored so}; no builtin
//! member and no symbol-keyed property is written.
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

fn proven(v: &Verdict) -> bool {
    !matches!(v, Verdict::No(_))
}

fn live(v: &Verdict) -> bool {
    proven(v) && !format!("{v:?}").contains("Dead")
}

/// `store` runs, then `n` goes to builtin members (`forEach`,
/// `console.warn`, `console.log`): with those written, `n` escapes.
fn program(store: &str) -> String {
    format!(
        "class N {{ v = 1; }}
        class A {{ a = 1; }}
        class B {{ b = 2; }}
        const g = globalThis as Record<string, unknown>;
        {store}
        const n = new N();
        [n].forEach((x) => console.log(x.v));
        console.warn(n);
        console.log(/*A*/ n.v, n instanceof N);"
    )
}

/// RED (G2 a, b): a store under an unknown String key (`argv`) leaves
/// `console.warn`, `Array.prototype.forEach`, `@@hasInstance` untouched.
#[test]
fn gk_g2_unknown_string_key() {
    let src = program("g[String(process.argv[2])] = A;");
    let (v, dump) = verdict(&src, "v", "/*A*/");
    assert!(proven(&v), "n.v proven, got {v:?}\n{dump}");
}

/// RED (G2 c): the app's loop: the key node also holds the classes
/// (ToPropertyKey of a class: Function.prototype.toString, a String).
#[test]
fn gk_g2_entries_loop() {
    let src = program("const classes = { A, B };\n        for (const [k, v] of Object.entries(classes)) g[k] = v;");
    let (v, dump) = verdict(&src, "v", "/*A*/");
    assert!(proven(&v), "n.v proven, got {v:?}\n{dump}");
    // typed as a record: the pair's key may be a Number (an index name
    // of the global object, still a String property: 7.1.19)
    let src = program("const classes: Record<string, unknown> = { A, B };\n        for (const [k, v] of Object.entries(classes)) g[k] = v;");
    let (v, dump) = verdict(&src, "v", "/*A*/");
    assert!(proven(&v), "record loop: n.v proven, got {v:?}\n{dump}");
    let src = program("const k: any = process.argv[2] === \"a\" ? A : \"s\";\n        g[k] = B;");
    let (v, dump) = verdict(&src, "v", "/*A*/");
    assert!(proven(&v), "class key: n.v proven, got {v:?}\n{dump}");
}

/// RED (G2 a): `new CustomEvent(...)` after a store under an unknown
/// String key constructs the stored class too (13.3.5.1).
#[test]
fn gk_g2_new_constructs_stored() {
    let src = "class N { v = 1; }
        class MyCE { type: string; constructor(t: string, o: any) { this.type = t; const d = o.detail; console.log(/*B*/ d.v); } }
        const g = globalThis as Record<string, unknown>;
        g[String(process.argv[2])] = MyCE;
        const n = new N();
        const e: any = new CustomEvent(\"x\", { detail: n });
        console.log(e.type, /*A*/ n.v);";
    let (v, dump) = verdict(src, "v", "/*B*/");
    assert!(live(&v), "MyCE constructed by `new CustomEvent` (live, proven), got {v:?}\n{dump}");
    let (v, dump) = verdict(src, "v", "/*A*/");
    assert!(proven(&v), "n.v proven, got {v:?}\n{dump}");
}

/// KEEP (G2 exactness, native readers): a module export equal to a global a native
/// reads by name (`util.TextEncoder`) may be the stored class.
#[test]
fn gk_g2_native_reader_sees_stored() {
    let src = "class N { v = 1; }
        class U { constructor(s: any) { console.log(/*B*/ s.v); } }
        const g = globalThis as Record<string, unknown>;
        g[String(process.argv[2])] = U;
        const util = require(\"util\");
        const n = new N();
        const TE: any = util.TextEncoder;
        const t: any = new TE(n);
        console.log(t);";
    let (v, dump) = verdict(src, "v", "/*B*/");
    assert!(!format!("{v:?}").contains("Dead"), "U may be constructed through util.TextEncoder, got {v:?}\n{dump}");
}

/// KEEP: the old behaviour (n escapes) where the rule does not apply.
#[test]
fn gk_keep() {
    for store in [
        // not the global object
        "(Array.prototype as any).zz = (x: any) => x;",
        // the key may be a Symbol
        "g[process.argv[2] === \"a\" ? Symbol.iterator as any : \"k\"] = A;",
        // ToPropertyKey of the key may be a Symbol (a program `toString`)
        "class S { toString(): any { return Symbol.iterator; } }\n        const k: any = process.argv[2] === \"a\" ? new S() : \"x\";\n        g[k] = A;",
        // a native hands a global read by name on (`.constructor`)
        "g[String(process.argv[2])] = A;\n        const ts: any = new TextEncoderStream();\n        console.log(ts.constructor);",
    ] {
        let src = program(store);
        let (v, dump) = verdict(&src, "v", "/*A*/");
        assert!(!proven(&v), "{store}: n escapes as before, got {v:?}\n{dump}");
    }
    // (d) and the special cases solve.
    for store in [
        "g[\"undefined\"] = 1; console.log(undefined, NaN, Infinity);",
        "g[String(process.argv[2])] = A; console.log(undefined, NaN, (globalThis as any).navigator);",
        "delete (globalThis as any).fetch; console.log((globalThis as any).fetch);",
    ] {
        let _ = verdict(&program(store), "v", "/*A*/");
    }
}
