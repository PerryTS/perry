// R9STORE5 (2026-11-16): a named store whose receivers are decided arm
// by arm: program class instances (rule A), builtin function values and
// Arrays (CreateDataProperty on an extensible ordinary object, ECMA-262
// 10.1.9.2 step 2.c-e), and Number / String / Boolean primitives in strict
// code (10.1.9.2 step 2.b returns false, PutValue 6.2.5.6 step 3.e throws
// the TypeError), `undefined` / `null` (rule N / T).
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
        .map(|s| format!("{:?} {:?}", s.verdict, s.receivers))
        .collect();
    eprintln!("RS5 .{key} store verdicts={v:?}");
    v
}

/// Every store of `key` has verdict `want`.
fn all(pf: &crate::ProgramFacts, key: &str, want: &str) -> bool {
    let s = stores(pf, key);
    !s.is_empty() && s.iter().all(|v| v.split(' ').next() == Some(want))
}

/// No store of `key` is proven by a store-effect verdict.
fn refused(pf: &crate::ProgramFacts, key: &str) -> bool {
    let s = stores(pf, key);
    !s.is_empty() && s.iter().all(|v| v.starts_with("No("))
}

const TAIL: &str = "console.log(JSON.stringify(f(process.argv[2] ?? \"x\")));\n";

const P: &str = "\"use strict\";\nclass P { a: number; constructor(n: number) { this.a = n; } }\nfunction g(o: any) { o.flag = true; }\n";

// S1 (RED): a builtin function value beside a class instance.
#[test]
fn r9store5_s1_builtin_fn() {
    let pf = facts(&format!(
        "{P}function f(v: string) {{ const p = new P(1); g(p); g(Math.max); g(Number); return [p.flag, (Math.max as any).flag, v]; }}\n{TAIL}"
    ));
    assert!(all(&pf, "flag", "KnownObjectStore"), "S1 flag not a known-object store");
}

// S2 (RED): an Array beside a class instance.
#[test]
fn r9store5_s2_array() {
    let pf = facts(&format!(
        "{P}function f(v: string) {{ const p = new P(1); const a = [1, 2]; g(p); g(a); return [p.flag, (a as any).flag, v]; }}\n{TAIL}"
    ));
    assert!(all(&pf, "flag", "KnownObjectStore"), "S2 flag not a known-object store");
}

// S3 (RED): number, boolean and undefined beside the instance: TypeError arms.
#[test]
fn r9store5_s3_primitives_beside() {
    let pf = facts(&format!(
        "{P}function f(v: string) {{ const p = new P(1); g(p); for (const x of [v.length, v.length > 1, undefined]) {{ try {{ g(x); }} catch (e) {{}} }} return [p.flag, v]; }}\n{TAIL}"
    ));
    assert!(all(&pf, "flag", "KnownObjectStore"), "S3 flag not a known-object store");
}

// S4 (RED): only primitives: every run throws.
#[test]
fn r9store5_s4_primitives_only() {
    let pf = facts(&format!(
        "\"use strict\";\nfunction h(o: any) {{ o.z = 1; }}\nfunction f(v: string) {{ try {{ h(v.length); }} catch (e) {{}} try {{ h(v); }} catch (e) {{}} return [v]; }}\n{TAIL}"
    ));
    assert!(all(&pf, "z", "Throws"), "S4 z not Throws");
}

// K8 (KEEP): sloppy code: [[Set]] returns false and nothing throws.
#[test]
fn r9store5_k8_sloppy_primitive() {
    let pf = facts(&format!(
        "function h(o: any) {{ o.z = 1; }}\nfunction f(v: string) {{ h(v.length); h(v); return [v]; }}\n{TAIL}"
    ));
    assert!(refused(&pf, "z"), "K8 sloppy primitive store proven");
}

// K1 (KEEP): a setter for the key on Object.prototype.
#[test]
fn r9store5_k1_objproto_setter() {
    let pf = facts(&format!(
        "{P}Object.defineProperty(Object.prototype, \"flag\", {{ set(x: any) {{ console.log(\"set\"); }}, configurable: true }});\nfunction f(v: string) {{ const p = new P(1); g(p); g(Math.max); return [v]; }}\n{TAIL}"
    ));
    assert!(refused(&pf, "flag"), "K1 flag proven with a setter on Object.prototype");
}

// K2 (KEEP): the builtin function is frozen.
#[test]
fn r9store5_k2_frozen_builtin() {
    let pf = facts(&format!(
        "{P}Object.freeze(Math.max);\nfunction f(v: string) {{ const p = new P(1); g(p); try {{ g(Math.max); }} catch (e) {{}} return [v]; }}\n{TAIL}"
    ));
    assert!(refused(&pf, "flag"), "K2 flag proven on a frozen builtin");
}

// K3 (KEEP): own non-writable `name` of a builtin function.
#[test]
fn r9store5_k3_own_name() {
    let pf = facts(&format!(
        "{P}function n(o: any) {{ o.name = \"q\"; }}\nfunction f(v: string) {{ const p = new P(1); n(p); try {{ n(Math.max); }} catch (e) {{}} return [v]; }}\n{TAIL}"
    ));
    assert!(refused(&pf, "name"), "K3 name proven on a builtin function");
}

// K4 (KEEP): a setter for the key on Number.prototype, a number receiver.
#[test]
fn r9store5_k4_number_proto_setter() {
    let pf = facts(&format!(
        "{P}Object.defineProperty(Number.prototype, \"flag\", {{ set(x: any) {{ console.log(\"set\"); }} }});\nfunction f(v: string) {{ const p = new P(1); g(p); g(v.length); return [v]; }}\n{TAIL}"
    ));
    assert!(refused(&pf, "flag"), "K4 flag proven with a setter on Number.prototype");
}

// K5 (KEEP): `__proto__` onto a number runs the Object.prototype setter.
#[test]
fn r9store5_k5_proto_key() {
    let pf = facts(&format!(
        "{P}function s(o: any) {{ o.__proto__ = null; }}\nfunction f(v: string) {{ const p = new P(1); s(p); s(v.length); return [v]; }}\n{TAIL}"
    ));
    let v = stores(&pf, "__proto__");
    assert!(v.iter().all(|x| !x.starts_with("KnownObjectStore") && !x.starts_with("Throws")), "K5 __proto__ proven");
}

// K6 (KEEP): an Array's `length` is no plain key (ArraySetLength).
#[test]
fn r9store5_k6_array_length() {
    let pf = facts(&format!(
        "{P}function l(o: any) {{ o.length = 0; }}\nfunction f(v: string) {{ const a = [1, 2]; l(a); l(Math.max); return [a.length, v]; }}\n{TAIL}"
    ));
    assert!(refused(&pf, "length"), "K6 length proven");
}

// K7 (KEEP): the array escapes to unknown code (it may freeze it).
#[test]
fn r9store5_k7_array_escapes() {
    let pf = facts(&format!(
        "{P}function f(v: string) {{ const a = [1, 2]; (globalThis as any)[v](a); g(a); return [v]; }}\n{TAIL}"
    ));
    assert!(refused(&pf, "flag"), "K7 flag proven on an escaped array");
}
