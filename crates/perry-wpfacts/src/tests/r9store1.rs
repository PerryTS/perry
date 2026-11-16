// R9STORE1 (2026-11-16): stores onto one object literal allocation. A
// named store whose receiver set is ONE object literal site, never HARD,
// never escaped, with %Object.prototype% as its whole chain and no accessor
// of the key: ECMA-262 10.1.9.2 OrdinarySet adds or overwrites an own data
// property, exactly (the R9BIRTH4 known-object verdict, its own column).
use super::*;
use perry_hir::Module;

fn facts(src: &str) -> crate::ProgramFacts {
    let path = "a.ts".to_string();
    let module = perry_parser::parse_typescript(src, &path).expect("parses");
    let hir = perry_hir::lower_module(&module, "a", &path).expect("lowers");
    let h = vec![(path, hir)];
    let list: Vec<(String, &Module)> = h.iter().map(|(p, m)| (p.clone(), m)).collect();
    let cfg = FactsConfig {
        unit_io: true,
        ..Default::default()
    };
    crate::compute(&list, &cfg)
}

/// The verdicts (Debug text) of the store sites of `key`.
fn stores(pf: &crate::ProgramFacts, key: &str) -> Vec<String> {
    let mi = pf.module_paths.iter().position(|p| p == "a.ts").expect("a.ts") as u32;
    let v: Vec<String> = pf
        .sites
        .iter()
        .filter(|s| s.module == mi && s.key == key && s.access.is_store())
        .map(|s| format!("{:?}", s.verdict))
        .collect();
    eprintln!("RS1 .{key} store verdicts={v:?}");
    v
}

fn known(pf: &crate::ProgramFacts, key: &str, want: bool) -> bool {
    let s = stores(pf, key);
    !s.is_empty() && s.iter().all(|v| (v == "KnownObjectStore") == want)
}

const TAIL: &str = "console.log(JSON.stringify(f(process.argv[2] ?? \"x\")));\n";

// L1 (RED): `const data = {}` extended later, the toJSON shape: a store
// after a call, and stores under `if`.
#[test]
fn r9store1_l1_fresh_literal_adds() {
    let pf = facts(&format!(
        "function g(x: any) {{ return x.length; }}\nfunction f(v: string) {{ const data: any = {{}}; const n = g(v); data.a = v; if (n > 1) data.b = n; if (n > 2) {{ data.c = [v]; }} return data; }}\n{TAIL}"
    ));
    assert!(known(&pf, "a", true), "L1 data.a not a known-object store");
    assert!(known(&pf, "b", true), "L1 data.b not a known-object store");
    assert!(known(&pf, "c", true), "L1 data.c not a known-object store");
}

// L2 (RED): a literal with keys, overwritten and extended under `if`
// (an overwrite of a birth key is proven already: it is no No either way).
#[test]
fn r9store1_l2_overwrite_under_if() {
    let pf = facts(&format!(
        "function f(v: string) {{ const o: any = {{ k: 1, meta: {{ t: v }} }}; if (v.length > 1) {{ o.k = v; o.m = 2; }} if (v.length > 3) o.k = 3; return o; }}\n{TAIL}"
    ));
    assert!(stores(&pf, "k").iter().all(|v| !v.starts_with("No")), "L2 o.k unproven");
    assert!(known(&pf, "m", true), "L2 o.m not a known-object store");
}

// K1: the literal passed to unknown code before the store.
#[test]
fn r9store1_k1_escaped_literal() {
    let pf = facts(&format!(
        "function f(v: string) {{ const o: any = {{}}; (globalThis as any)[v](o); o.k = v; return o; }}\n{TAIL}"
    ));
    assert!(known(&pf, "k", false), "K1 escaped literal judged known");
}

// K2: a closure that freezes the literal, called before the store.
#[test]
fn r9store1_k2_closure_freezes() {
    let pf = facts(&format!(
        "function f(v: string) {{ const o: any = {{}}; const g = () => {{ Object.freeze(o); }}; g(); o.k = v; return o; }}\n{TAIL}"
    ));
    assert!(known(&pf, "k", false), "K2 frozen literal judged known");
}

// K3: `Object.freeze(o)` before the store.
#[test]
fn r9store1_k3_frozen() {
    let pf = facts(&format!(
        "function f(v: string) {{ const o: any = {{}}; Object.freeze(o); o.k = v; return o; }}\n{TAIL}"
    ));
    assert!(known(&pf, "k", false), "K3 frozen literal judged known");
}

// K4: a `__proto__: p` entry, and a `__proto__` store.
#[test]
fn r9store1_k4_proto() {
    let pf = facts(&format!(
        "const p: any = {{ q: 0 }};\nfunction f(v: string) {{ const o: any = {{ __proto__: p }}; o.k = v; const z: any = {{}}; z.__proto__ = p; return [o, z]; }}\n{TAIL}"
    ));
    assert!(known(&pf, "k", false), "K4 __proto__ literal judged known");
    assert!(known(&pf, "__proto__", false), "K4 __proto__ store judged known");
}

// K5: a computed store from `process.argv[2]` onto the literal: SOME key
// (unproven), and the named store beside it is refused too.
#[test]
fn r9store1_k5_computed_key() {
    let pf = facts(&format!(
        "function f(v: string) {{ const o: any = {{}}; o[v] = 1; o.k = v; return o; }}\n{TAIL}"
    ));
    assert!(known(&pf, "k", false), "K5 literal with a computed store judged known");
}

// K6: the literal has a setter for the key: the store calls it.
#[test]
fn r9store1_k6_setter() {
    let pf = facts(&format!(
        "function f(v: string) {{ const o: any = {{ x: 0, set k(w: any) {{ this.x = w; }} }}; o.k = v; return o; }}\n{TAIL}"
    ));
    assert!(known(&pf, "k", false), "K6 literal setter judged known");
}
