// R9STORE4 (2026-11-16): rule A, a named store onto instances of a
// complete set of prefix-stable program classes whose chains hold no
// accessor and no data property under the key: ECMA-262 10.1.9.2 sets the
// own writable data property in place or creates it (step 2.e), no user
// code runs: `Verdict::KnownObjectStore`, whether or not the key is
// present. With a nullish receiver beside it, rule N composes.
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
    eprintln!("RS4 .{key} store verdicts={v:?}");
    v
}

/// Every store of `key` is the rule's verdict.
fn known(pf: &crate::ProgramFacts, key: &str) -> bool {
    let s = stores(pf, key);
    !s.is_empty() && s.iter().all(|v| v == "KnownObjectStore")
}

/// No store of `key` is the rule's verdict (an exact accessor verdict,
/// `AccessorSet`, may stand instead).
fn refused(pf: &crate::ProgramFacts, key: &str) -> bool {
    let s = stores(pf, key);
    !s.is_empty() && s.iter().all(|v| v != "KnownObjectStore")
}

const TAIL: &str = "console.log(JSON.stringify(f(process.argv[2] ?? \"x\")));\n";

const P: &str = "class P { a: number; constructor(n: number) { this.a = n; } bind(s: any) { this.skel = s; return this; } }\n";

// A1 (RED): a key a method adds after birth (the SkinnedMesh `bind` form).
#[test]
fn r9store4_a1_method_adds_key() {
    let pf = facts(&format!(
        "{P}function f(v: string) {{ const p = new P(v.length); p.bind(v); const q = new P(2); q.bind(1); return [p.skel, q.skel, p.a]; }}\n{TAIL}"
    ));
    assert!(known(&pf, "skel"), "A1 skel not a known-object store");
}

// A2 (RED): a subclass instance beside the class's own: one store, two
// classes, no accessor on either chain.
#[test]
fn r9store4_a2_two_classes() {
    let pf = facts(&format!(
        "{P}class D extends P {{ b = 1; }}\nfunction f(v: string) {{ const p = v.length > 1 ? new P(1) : new D(2); p.bind(v); return [p.skel]; }}\n{TAIL}"
    ));
    assert!(known(&pf, "skel"), "A2 skel not a known-object store");
}

// A3 (RED): rule N composes: `undefined` beside the instance.
#[test]
fn r9store4_a3_nullish_beside() {
    let pf = facts(&format!(
        "{P}function g(o: any, i: number) {{ o.idx = i; }}\nfunction f(v: string) {{ const p = new P(1); g(p, 3); try {{ g(v.length > 9 ? undefined : p, 4); }} catch (e) {{}} return [p.idx]; }}\n{TAIL}"
    ));
    assert!(known(&pf, "idx"), "A3 idx not a known-object store");
}

// K1: a subclass prototype holds a setter for the key. Since R9STORE6
// (CWN decision 2026-11-16, per-kind store verdicts) the P arm adds the own
// key and the D arm calls the one known setter: a KnownObjectStore (no slot
// or setter claim: the generic store), never a rule-A-only verdict.
#[test]
fn r9store4_k1_subclass_setter() {
    let pf = facts(&format!(
        "{P}class D extends P {{ set skel(x: any) {{ (globalThis as any).g = x; }} }}\nfunction f(v: string) {{ const p = v.length > 1 ? new P(1) : new D(2); p.bind(v); return [p.a]; }}\n{TAIL}"
    ));
    assert!(known(&pf, "skel"), "K1 skel: setter + own-data arms not the per-kind store verdict");
}

// K2 (KEEP): defineProperty puts a setter for the key on the prototype.
#[test]
fn r9store4_k2_define_proto_setter() {
    let pf = facts(&format!(
        "{P}Object.defineProperty(P.prototype, \"skel\", {{ set(x: any) {{ (globalThis as any).g = x; }} }});\nfunction f(v: string) {{ const p = new P(1); p.bind(v); return [p.a]; }}\n{TAIL}"
    ));
    assert!(refused(&pf, "skel"), "K2 skel a known-object store although the prototype has a setter");
}

// K3 (KEEP): one instance, chosen by argv, gets a non-writable own key.
#[test]
fn r9store4_k3_define_instance_readonly() {
    let pf = facts(&format!(
        "{P}function f(v: string) {{ const ps = [new P(1), new P(2)]; Object.defineProperty(ps[v.length % 2], \"skel\", {{ value: 1, writable: false }}); ps[0].bind(v); return [ps[0].skel]; }}\n{TAIL}"
    ));
    assert!(refused(&pf, "skel"), "K3 skel proven although an instance has it non-writable");
}

// K4 (KEEP): one instance is frozen.
#[test]
fn r9store4_k4_freeze() {
    let pf = facts(&format!(
        "{P}function f(v: string) {{ const p = new P(1); const q = new P(2); Object.freeze(v.length > 1 ? p : q); p.bind(v); q.bind(v); return [p.a]; }}\n{TAIL}"
    ));
    assert!(refused(&pf, "skel"), "K4 skel proven although an instance may be frozen");
}

// K5 (KEEP): a class extending Array (exotic instance).
#[test]
fn r9store4_k5_extends_array() {
    let pf = facts(&format!(
        "class A extends Array {{ bind(s: any) {{ (this as any).skel = s; return this; }} }}\nfunction f(v: string) {{ const a = new A(); a.bind(v); return [(a as any).skel]; }}\n{TAIL}"
    ));
    assert!(refused(&pf, "skel"), "K5 skel proven on an Array subclass");
}

// K6 (KEEP): a computed store with a key from argv onto the prototype.
#[test]
fn r9store4_k6_computed_proto_store() {
    let pf = facts(&format!(
        "{P}(P.prototype as any)[process.argv[2] ?? \"y\"] = 1;\nfunction f(v: string) {{ const p = new P(1); p.bind(v); return [p.a]; }}\n{TAIL}"
    ));
    assert!(refused(&pf, "skel"), "K6 skel proven although the prototype may hold the key");
}

// K7 (KEEP): a number beside the instance (not this rule).
#[test]
fn r9store4_k7_number_beside() {
    let pf = facts(&format!(
        "{P}function g(o: any, i: number) {{ o.idx = i; }}\nfunction f(v: string) {{ const p = new P(1); g(p, 3); g(v.length > 9 ? 5 : p, 4); return [p.idx]; }}\n{TAIL}"
    ));
    assert!(refused(&pf, "idx"), "K7 idx proven with a number receiver");
}

// T1 (RED): only `undefined` reaches the base: the store always throws.
#[test]
fn r9store4_t1_only_nullish_throws() {
    let pf = facts(&format!(
        "{P}function g(o: any, i: number) {{ o.idx = i; }}\nfunction f(v: string) {{ const p = new P(1); try {{ g(v.length > 99 ? undefined : undefined, 4); }} catch (e) {{}} return [p.a]; }}\n{TAIL}"
    ));
    let s = stores(&pf, "idx");
    assert!(!s.is_empty() && s.iter().all(|v| v == "Throws"), "T1 idx not Throws");
}

// T2 (KEEP): a string beside `undefined` (no TypeError on every run).
#[test]
fn r9store4_t2_string_beside_nullish() {
    let pf = facts(&format!(
        "{P}function g(o: any, i: number) {{ o.idx = i; }}\nfunction f(v: string) {{ try {{ g(v.length > 9 ? undefined : v, 4); }} catch (e) {{}} return [v]; }}\n{TAIL}"
    ));
    let s = stores(&pf, "idx");
    assert!(s.iter().all(|v| v != "Throws"), "T2 idx Throws with a string receiver");
}
