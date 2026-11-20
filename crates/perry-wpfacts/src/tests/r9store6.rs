// R9STORE6 (2026-11-16): named stores decided arm by arm when one arm
// calls ONE known setter (ECMA-262 10.1.9.2 step 3 / 7: Call(setter,
// Receiver, << V >>)) beside own-data arms (item 1), and stores whose chain
// holds the key as a writable DATA property of a program prototype, a
// class method (10.1.9.2 step 2.c-e on the Receiver, item 2).
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
    let mi = pf
        .module_paths
        .iter()
        .position(|p| p == "a.ts")
        .expect("a.ts") as u32;
    let v: Vec<String> = pf
        .sites
        .iter()
        .filter(|s| s.module == mi && s.key == key && s.access.is_store())
        .map(|s| format!("{:?} {:?}", s.verdict, s.receivers))
        .collect();
    eprintln!("RS6 .{key} store verdicts={v:?}");
    v
}

/// Every store of `key` has verdict `want`.
fn all(pf: &crate::ProgramFacts, key: &str, want: &str) -> bool {
    let s = stores(pf, key);
    !s.is_empty() && s.iter().all(|v| v.split(' ').next() == Some(want))
}

/// No store of `key` is a known-object store.
fn not_known(pf: &crate::ProgramFacts, key: &str) -> bool {
    let s = stores(pf, key);
    !s.is_empty() && s.iter().all(|v| !v.starts_with("KnownObjectStore"))
}

const TAIL: &str = "console.log(JSON.stringify(f(process.argv[2] ?? \"x\")));\n";

const P: &str = "\"use strict\";\nclass P { a: number; constructor(n: number) { this.a = n; } }\n\
class Mat { v = 0; set k(x: any) { this.v = this.v + 1; } }\nclass MS extends Mat { w = 1; }\n\
function g(o: any) { o.k = true; }\n";

// S1 (RED): own-data arm (P) beside a known-setter arm (MS -> Mat.prototype).
#[test]
fn r9store6_s1_setter_beside_data() {
    let pf = facts(&format!(
        "{P}function f(v: string) {{ const p = new P(1); const s = new MS(); g(p); g(s); return [p.k, s.v, v]; }}\n{TAIL}"
    ));
    assert!(
        all(&pf, "k", "KnownObjectStore"),
        "S1 k not a known-object store"
    );
}

// S2 (RED): setter arm + own-data arm + undefined: TypeError arm beside.
#[test]
fn r9store6_s2_setter_data_nullish() {
    let pf = facts(&format!(
        "{P}function f(v: string) {{ const p = new P(1); const s = new MS(); g(p); g(s); try {{ g(v.length > 5 ? undefined : p); }} catch (e) {{}} return [p.k, s.v, v]; }}\n{TAIL}"
    ));
    assert!(
        all(&pf, "k", "KnownObjectStore"),
        "S2 k not a known-object store"
    );
}

// K1: a subclass with an own data field of the same name.
#[test]
fn r9store6_k1_subclass_field() {
    let pf = facts(&format!(
        "{P}class MF extends Mat {{ k: any = 1; }}\nfunction f(v: string) {{ const p = new P(1); const s = new MS(); const q = new MF(); g(p); g(s); g(q); return [p.k, s.v, q.k, v]; }}\n{TAIL}"
    ));
    assert!(not_known(&pf, "k"), "K1 claimed");
}

// K2: Object.defineProperty on one instance chosen by argv.
#[test]
fn r9store6_k2_define_instance() {
    let pf = facts(&format!(
        "{P}function f(v: string) {{ const p = new P(1); const s = new MS(); const t = new MS(); Object.defineProperty(v === \"y\" ? s : t, \"k\", {{ value: 1, writable: false }}); g(p); g(s); g(t); return [p.k, s.v, v]; }}\n{TAIL}"
    ));
    assert!(not_known(&pf, "k"), "K2 claimed");
}

// K3: the setter replaced later.
#[test]
fn r9store6_k3_setter_replaced() {
    let pf = facts(&format!(
        "{P}function f(v: string) {{ if (v === \"y\") Object.defineProperty(Mat.prototype, \"k\", {{ set(x: any) {{ throw new Error(\"no\"); }} }}); const p = new P(1); const s = new MS(); g(p); g(s); return [p.k, s.v, v]; }}\n{TAIL}"
    ));
    assert!(not_known(&pf, "k"), "K3 claimed");
}

// K4: a getter-only accessor (strict: TypeError arm) beside own data.
#[test]
fn r9store6_k4_getter_only() {
    let pf = facts(&format!(
        "{P}class GO {{ get k() {{ return 1; }} }}\nfunction f(v: string) {{ const p = new P(1); const o = new GO(); g(p); try {{ g(o); }} catch (e) {{}} return [p.k, o.k, v]; }}\n{TAIL}"
    ));
    assert!(not_known(&pf, "k"), "K4 claimed");
}

const Q: &str =
    "\"use strict\";\nclass Q { n = 0; cb() {} on(f: any) { this.cb = f; return this; } }\n";

// S3 (RED): `this.cb = f` where Q.prototype holds the method `cb`.
#[test]
fn r9store6_s3_method_on_proto() {
    let pf = facts(&format!(
        "{Q}function f(v: string) {{ const q = new Q(); q.on(() => v.length); return [q.n, typeof q.cb, v]; }}\n{TAIL}"
    ));
    assert!(
        all(&pf, "cb", "KnownObjectStore"),
        "S3 cb not a known-object store"
    );
}

// S4 (RED): the method on a superclass prototype, a subclass instance.
#[test]
fn r9store6_s4_method_on_super() {
    let pf = facts(&format!(
        "{Q}class Q2 extends Q {{ m = 1; }}\nfunction f(v: string) {{ const q = new Q2(); q.on(() => v.length); return [q.m, typeof q.cb, v]; }}\n{TAIL}"
    ));
    assert!(
        all(&pf, "cb", "KnownObjectStore"),
        "S4 cb not a known-object store"
    );
}

// K5: Object.defineProperty(Q.prototype, "cb", { writable: false }).
#[test]
fn r9store6_k5_proto_define_readonly() {
    let pf = facts(&format!(
        "{Q}function f(v: string) {{ if (v === \"y\") Object.defineProperty(Q.prototype, \"cb\", {{ writable: false }}); const q = new Q(); try {{ q.on(() => v.length); }} catch (e) {{}} return [q.n, typeof q.cb, v]; }}\n{TAIL}"
    ));
    assert!(not_known(&pf, "cb"), "K5 claimed");
}

// K6: Object.freeze(Q.prototype).
#[test]
fn r9store6_k6_proto_frozen() {
    let pf = facts(&format!(
        "{Q}function f(v: string) {{ if (v === \"y\") Object.freeze(Q.prototype); const q = new Q(); try {{ q.on(() => v.length); }} catch (e) {{}} return [q.n, typeof q.cb, v]; }}\n{TAIL}"
    ));
    assert!(not_known(&pf, "cb"), "K6 claimed");
}

// K7: a getter-only `cb` on the superclass, no method between: not this rule.
#[test]
fn r9store6_k7_getter_on_super() {
    let pf = facts(&format!(
        "\"use strict\";\nclass B {{ get cb(): any {{ return 1; }} }}\nclass Q3 extends B {{ n = 0; on(f: any) {{ this.cb = f; return this; }} }}\nfunction f(v: string) {{ const q = new Q3(); try {{ q.on(() => v.length); }} catch (e) {{}} return [q.n, v]; }}\n{TAIL}"
    ));
    assert!(not_known(&pf, "cb"), "K7 claimed");
}

// K8: the prototype member stored through a computed key from argv.
#[test]
fn r9store6_k8_proto_computed_store() {
    let pf = facts(&format!(
        "{Q}function f(v: string) {{ (Q.prototype as any)[v] = 1; const q = new Q(); q.on(() => v.length); return [q.n, typeof q.cb, v]; }}\n{TAIL}"
    ));
    assert!(not_known(&pf, "cb"), "K8 claimed");
}

/// No store of `key` is a setter claim or a known-object store.
fn no_setter(pf: &crate::ProgramFacts, key: &str) -> bool {
    let s = stores(pf, key);
    !s.is_empty()
        && s.iter()
            .all(|v| !v.starts_with("KnownObjectStore") && !v.starts_with("AccessorSet"))
}

// K1b: the single-kind form of K1: only the subclass with the own field.
#[test]
fn r9store6_k1b_field_alone() {
    let pf = facts(&format!(
        "{P}class MF extends Mat {{ k: any = 1; }}\nfunction f(v: string) {{ const q = new MF(); g(q); return [q.k, q.v, v]; }}\n{TAIL}"
    ));
    assert!(no_setter(&pf, "k"), "K1b claimed");
}

// K2b: the single-kind form of K2: only MS instances, one defined by argv.
#[test]
fn r9store6_k2b_define_alone() {
    let pf = facts(&format!(
        "{P}function f(v: string) {{ const s = new MS(); const t = new MS(); Object.defineProperty(v === \"y\" ? s : t, \"k\", {{ value: 1, writable: false }}); try {{ g(s); g(t); }} catch (e) {{}} return [s.v, t.v, v]; }}\n{TAIL}"
    ));
    assert!(no_setter(&pf, "k"), "K2b claimed");
}

const A: &str = "\"use strict\";\nfunction h(a: any[]) { a.length = 0; }\n";

// S5 (RED): `a.length = 0` onto two array literals.
#[test]
fn r9store6_s5_array_length() {
    let pf = facts(&format!(
        "{A}function f(v: string) {{ const a = [1, 2]; const b = [v]; h(a); h(b); return [a.length, b.length, v]; }}\n{TAIL}"
    ));
    assert!(
        all(&pf, "length", "KnownObjectStore"),
        "S5 length not a known-object store"
    );
}

// K9: a non-configurable element defined on one array by argv.
#[test]
fn r9store6_k9_nonconfigurable_element() {
    let pf = facts(&format!(
        "{A}function f(v: string) {{ const a = [1, 2]; const b = [v]; Object.defineProperty(v === \"y\" ? a : b, \"0\", {{ value: 1, configurable: false }}); try {{ h(a); h(b); }} catch (e) {{}} return [a.length, b.length, v]; }}\n{TAIL}"
    ));
    assert!(not_known(&pf, "length"), "K9 claimed");
}

// K10: a non-writable length by argv.
#[test]
fn r9store6_k10_readonly_length() {
    let pf = facts(&format!(
        "{A}function f(v: string) {{ const a = [1, 2]; if (v === \"y\") Object.defineProperty(a, \"length\", {{ writable: false }}); try {{ h(a); }} catch (e) {{}} return [a.length, v]; }}\n{TAIL}"
    ));
    assert!(not_known(&pf, "length"), "K10 claimed");
}

// K11: Object.freeze on the array by argv.
#[test]
fn r9store6_k11_frozen() {
    let pf = facts(&format!(
        "{A}function f(v: string) {{ const a = [1, 2]; if (v === \"y\") Object.freeze(a); try {{ h(a); }} catch (e) {{}} return [a.length, v]; }}\n{TAIL}"
    ));
    assert!(not_known(&pf, "length"), "K11 claimed");
}

// K12: a value that is not a constant (ToNumber of an object may run code).
#[test]
fn r9store6_k12_value_not_constant() {
    let pf = facts(&format!(
        "\"use strict\";\nfunction h(a: any[], n: any) {{ a.length = n; }}\nfunction f(v: string) {{ const a = [1, 2]; h(a, v.length > 3 ? 1 : {{ valueOf() {{ return 0; }} }}); return [a.length, v]; }}\n{TAIL}"
    ));
    assert!(not_known(&pf, "length"), "K12 claimed");
}

// K13: a negative constant (RangeError).
#[test]
fn r9store6_k13_negative() {
    let pf = facts(&format!(
        "\"use strict\";\nfunction h(a: any[]) {{ a.length = -1; }}\nfunction f(v: string) {{ const a = [1, 2]; try {{ h(a); }} catch (e) {{}} return [a.length, v]; }}\n{TAIL}"
    ));
    assert!(not_known(&pf, "length"), "K13 claimed");
}
