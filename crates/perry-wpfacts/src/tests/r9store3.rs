// R9STORE3 (2026-11-16): construction-phase stores.
// Rule S: `this.k = v` in a constructor where `this` has no own `k` and the
// chain's class declares ONE setter for `k`: [[Set]] (ECMA-262 10.1.9.2)
// calls that setter with `this`; no key is created; the walk follows its
// body. Rule I in the layout: a key the phase stores after chain reads that
// run no code (R9READ2 rule I, `Model::inert_sure`) is a definite birth key.
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
    eprintln!("RS3 .{key} store verdicts={v:?}");
    v
}

fn proven(pf: &crate::ProgramFacts, key: &str) -> bool {
    let s = stores(pf, key);
    !s.is_empty() && s.iter().all(|v| !v.starts_with("No") && v != "none")
}

/// Not proven by these rules (S, I, N). R9STORE4: a known-object store
/// (rule A: the store adds or sets an own data property, exact for any
/// layout; with rule N beside it) is no slot claim, so it counts here.
fn unproven(pf: &crate::ProgramFacts, key: &str) -> bool {
    stores(pf, key).iter().any(|v| v.starts_with("No") || v == "KnownObjectStore")
}

const TAIL: &str = "console.log(JSON.stringify(f(process.argv[2] ?? \"x\")));\n";

const TEX: &str = "class T { constructor() { this.src = { data: null }; } get image() { return this.src.data; } set image(v: any) { this.src.data = v; } }\n";

// S1 (RED): the DataArrayTexture form: a store through the parent's setter,
// then fresh keys.
#[test]
fn r9store3_s1_setter_followed() {
    let pf = facts(&format!(
        "{TEX}class D extends T {{ constructor(n: number) {{ super(); this.image = {{ n }}; this.wrapR = n; this.layer = new Set(); }} }}\nfunction f(v: string) {{ const d = new D(v.length); d.wrapR = v; return [d.wrapR, d.image, d.layer.size]; }}\n{TAIL}"
    ));
    assert!(proven(&pf, "wrapR"), "S1 wrapR unproven");
    assert!(proven(&pf, "layer"), "S1 layer unproven");
}

// S2 (KEEP): a subclass overrides the setter with one that leaks `this`.
#[test]
fn r9store3_s2_setter_overridden() {
    let pf = facts(&format!(
        "{TEX}class D extends T {{ constructor(n: number) {{ super(); this.image = {{ n }}; this.wrapR = n; }} }}\nclass E extends D {{ set image(v: any) {{ (globalThis as any).g = this; }} }}\nfunction f(v: string) {{ const d = v.length > 1 ? new D(1) : new E(2); return [d.wrapR]; }}\n{TAIL}"
    ));
    assert!(unproven(&pf, "wrapR"), "S2 wrapR proven although E's setter leaks this");
}

// S3 (KEEP): the setter itself stores `this` into a global.
#[test]
fn r9store3_s3_setter_leaks_this() {
    let pf = facts(&format!(
        "class T {{ set image(v: any) {{ (globalThis as any).g = this; }} }}\nclass D extends T {{ constructor(n: number) {{ super(); this.image = n; this.wrapR = n; }} }}\nfunction f(v: string) {{ const d = new D(v.length); return [d.wrapR]; }}\n{TAIL}"
    ));
    assert!(unproven(&pf, "wrapR"), "S3 wrapR proven although the setter leaks this");
}

// I1 (RED): the KeyframeTrack form: a constructor store whose value reads a
// prototype data key (no accessor anywhere under it).
#[test]
fn r9store3_i1_inert_read_store() {
    let pf = facts(&format!(
        "function conv(a: any, T: any) {{ return new T(a); }}\nclass K {{ constructor(n: string, t: any) {{ this.name = n; this.times = conv(t, this.TB); }} }}\n(K.prototype as any).TB = Float32Array;\nfunction f(v: string) {{ const k = new K(v, [1, v.length]); return [k.name, k.times.length]; }}\n{TAIL}"
    ));
    assert!(proven(&pf, "times"), "I1 times unproven");
}

// I2 (KEEP): a subclass defines a getter under the read key.
#[test]
fn r9store3_i2_getter_on_subclass() {
    let pf = facts(&format!(
        "function conv(a: any, T: any) {{ return new T(a); }}\nclass K {{ constructor(n: string, t: any) {{ this.name = n; this.times = conv(t, this.TB); }} }}\n(K.prototype as any).TB = Float32Array;\nclass K2 extends K {{ get TB() {{ (globalThis as any).g = this; return Array; }} }}\nfunction f(v: string) {{ const k = v.length > 1 ? new K(v, [1]) : new K2(v, [2]); return [k.name, k.times.length]; }}\n{TAIL}"
    ));
    assert!(unproven(&pf, "times"), "I2 times proven although K2's getter observes this");
}

const SPH: &str = "class S { constructor() { this.r = 0; this.c = 1; } }\n";

// N1 (RED): the computeBoundingSphere form: a field born `null`, filled
// lazily; the store's receivers are `null` and S instances.
#[test]
fn r9store3_n1_nullish_beside_object() {
    let pf = facts(&format!(
        "{SPH}class G {{ constructor() {{ this.s = null; }} fit(n: number) {{ if (this.s === null) this.s = new S(); this.s.r = Math.sqrt(n); return this.s; }} }}\nfunction f(v: string) {{ const g = new G(); return g.fit(v.length).r; }}\n{TAIL}"
    ));
    assert!(proven(&pf, "r"), "N1 this.s.r unproven");
}

// N2 (KEEP): a number beside the object and the nullish value.
#[test]
fn r9store3_n2_number_beside() {
    let pf = facts(&format!(
        "{SPH}function f(v: string) {{ const o: any = v.length > 2 ? new S() : (v.length > 1 ? null : 5); o.r = 3; return [o]; }}\n{TAIL}"
    ));
    assert!(unproven(&pf, "r"), "N2 store over a number receiver proven");
}

// N3 (KEEP): the object arm is no slot store (it adds a key).
#[test]
fn r9store3_n3_object_arm_adds() {
    let pf = facts(&format!(
        "{SPH}function f(v: string) {{ const o: any = v.length > 2 ? new S() : null; o.z = 3; return [o]; }}\n{TAIL}"
    ));
    assert!(unproven(&pf, "z"), "N3 nullish beside an add proven");
}
