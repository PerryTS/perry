//! AMP-LOOP: three rules that keep one unknown value (⊤) from reaching
//! what it does not flow to, whole program.
//! K10 (premise P-N1, owner 2026-11-23): a value the analysis does not model
//! is no intrinsic, so a write onto ⊤ writes no built-in member.
//! K8 (owner 2026-11-14, R-GLOBAL 2026-11-18): a call or `new` of an
//! unknown value does not hold the realm.
//! K2 (Phase B, owner 2026-11-23): a program value handed to a builtin by
//! its contract gets no write under any key; one handed to a ⊤ callee
//! still does (until AMP-RESOLVE resolves the callee).
use super::*;

const DECL: &str = "declare function sink(x: any): void;\ndeclare function get(): any;\n";
const OBJ: &str = "const o = { m() { return 1; } };\n";

fn one(src: &str) -> ProgramFacts {
    let path = "t.ts".to_string();
    let module = perry_parser::parse_typescript(src, &path).expect("parses");
    let hir = perry_hir::lower_module(&module, "t", &path).expect("lowers");
    let mut cfg = FactsConfig::default();
    cfg.max_work.get_or_insert(50_000_000);
    crate::compute(&[(path, &hir)], &cfg)
}

/// The verdict of the first call site `.m(` after `marker`.
fn m_after(src: &str, marker: &str) -> String {
    let pf = one(src);
    let at = src.find(marker).expect("marker") as u32;
    let s = pf
        .sites
        .iter()
        .filter(|s| s.key == "m" && s.is_call && s.byte_offset >= at)
        .min_by_key(|s| s.byte_offset)
        .unwrap_or_else(|| panic!("no .m( after {marker:?}"));
    let v = format!("{:?}", s.verdict);
    eprintln!("AMP-LOOP {src:?}: {v}");
    v
}

/// One target, proven: no other value reached `o.m`.
fn mono(v: &str) -> bool {
    v == "T0Mono" || v.starts_with("T0Mono(")
}

/// Does unknown code hold the realm after the solve (`Model::realm_held`)?
fn held(src: &str) -> bool {
    let ast = Box::leak(Box::new(perry_parser::parse_typescript(src, "t.ts").unwrap()));
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

/// K10: the global object handed to unknown code puts ⊤ in reach of every
/// built-in (`global_in_top`); a store into ⊤ under `now` was then a write
/// of `performance.now`, and the call of it handed `o` to unknown code.
#[test]
fn k10_store_into_top_writes_no_intrinsic() {
    let src = format!(
        "{DECL}{OBJ}sink(globalThis);\nget().now = (x: any) => {{ x.m = () => 2; }};\n(performance as any).now(o);\nconst r = /*A*/ o.m();\nconsole.log(r);\n"
    );
    assert!(mono(&m_after(&src, "/*A*/")), "a store into ⊤ wrote performance.now");
}

/// K10 keep: a program store onto the named intrinsic is still its write.
#[test]
fn k10_keep_program_store_onto_the_intrinsic() {
    let src = format!(
        "{DECL}{OBJ}sink(globalThis);\n(performance as any).now = (x: any) => {{ x.m = () => 2; }};\n(performance as any).now(o);\nconst r = /*A*/ o.m();\nconsole.log(r);\n"
    );
    assert!(!mono(&m_after(&src, "/*A*/")), "the program's own performance.now was lost");
}

/// K8: a call and a `new` of an unknown value hold nothing, whole program.
#[test]
fn k8_unknown_call_holds_no_realm() {
    assert!(!held(&format!("{DECL}const r = get();\nconsole.log(r);\n")), "⊤ call held the realm");
    assert!(
        !held(&format!("{DECL}const C: any = get();\nconst r = new C(1);\nconsole.log(r);\n")),
        "⊤ new held the realm"
    );
}

/// K2: `o` handed to a host contract (the stream keeps the chunk): the
/// host writes no key of it, so its
/// field `n` holds only the program's number and `x.toFixed` is proven.
#[test]
fn k2_contract_escape_writes_no_key() {
    for h in ["process.stdout.write(o as any);"] {
        let src = format!(
            "const o = {{ m() {{ return 1; }}, n: 5 }};\n{h}\nconst x = o.n;\nconst r = /*A*/ x.toFixed(1);\nconsole.log(r);\n"
        );
        let pf = one(&src);
        let at = src.find("/*A*/").unwrap() as u32;
        let s = pf
            .sites
            .iter()
            .filter(|s| s.key == "toFixed" && s.byte_offset >= at)
            .min_by_key(|s| s.byte_offset)
            .expect("toFixed site");
        let v = format!("{:?}", s.verdict);
        eprintln!("AMP-LOOP {h}: {v}");
        assert!(!v.starts_with("No("), "a contract hand-off wrote o.n: {h}: {v}");
    }
}

/// K2 keep: `o` handed to a ⊤ callee may be written under any key.
#[test]
fn k2_keep_top_callee_writes() {
    let src = format!("{DECL}{OBJ}const f = get();\nf(o);\nconst r = /*A*/ o.m();\nconsole.log(r);\n");
    assert!(!mono(&m_after(&src, "/*A*/")), "a ⊤ callee's argument kept o.m");
}

/// K2 keep (upgrade): handed by contract first, then to a ⊤ callee.
#[test]
fn k2_keep_contract_then_top_callee() {
    let src = format!(
        "{DECL}{OBJ}process.stdout.write(o as any);\nconst f = get();\nf(o);\nconst r = /*A*/ o.m();\nconsole.log(r);\n"
    );
    assert!(!mono(&m_after(&src, "/*A*/")), "a ⊤ callee holding p could not write o.m");
}

/// K2 keep: a program store through `x` ∈ {o, ⊤} writes `o` (o handed by
/// contract is no ⊤: it stays beside ⊤ and gets the store).
#[test]
fn k2_keep_store_through_top_beside_contract_escape() {
    let src = format!(
        "{DECL}{OBJ}process.stdout.write(o as any);\nconst x: any = Math.random() > 0.5 ? o : get();\nx.m = () => 2;\nconst r = /*A*/ o.m();\nconsole.log(r);\n"
    );
    assert!(!mono(&m_after(&src, "/*A*/")), "the store through x missed o.m");
}

/// The verdict of the first site of `key` (a call when `call`) after `marker`.
fn site_after(src: &str, marker: &str, key: &str, call: bool) -> String {
    let pf = one(src);
    let at = src.find(marker).expect("marker") as u32;
    let s = pf
        .sites
        .iter()
        .filter(|s| s.key == key && s.is_call == call && s.byte_offset >= at)
        .min_by_key(|s| s.byte_offset)
        .unwrap_or_else(|| panic!("no {key} site after {marker:?}"));
    let v = format!("{:?}", s.verdict);
    eprintln!("AMP-LOOP2 {src:?}: {key}: {v}");
    v
}

const STREAM: &str = "process.stdout.write(o as any);\n";

/// K2 derive (template escape): a literal handed by contract keeps its
/// layout, so its own method call is proven.
#[test]
fn k2d_contract_escape_keeps_the_layout() {
    let src = format!("{OBJ}{STREAM}const r = /*A*/ o.m();\nconsole.log(r);\n");
    let v = m_after(&src, "/*A*/");
    assert!(mono(&v), "a contract hand-off changed o's layout: {v}");
}

/// K2 derive (prototype escape): an instance handed by contract takes its
/// prototype with it; the prototype keeps its methods.
#[test]
fn k2d_contract_escape_keeps_the_prototype() {
    let src = "class C { m() { return 1; } }\nconst o = new C();\nprocess.stdout.write(o as any);\nconst r = /*A*/ o.m();\nconsole.log(r);\n";
    let v = m_after(src, "/*A*/");
    assert!(mono(&v), "a contract hand-off changed C.prototype: {v}");
}

/// K2 derive (own data or absent): a read of the handed literal's own
/// field finds the program's data property.
#[test]
fn k2d_contract_escape_keeps_own_data() {
    let src = format!("const o = {{ m() {{ return 1; }}, n: 5 }};\n{STREAM}const x = /*A*/ o.n;\nconsole.log(x);\n");
    let v = site_after(&src, "/*A*/", "n", false);
    assert!(!v.starts_with("No("), "a contract hand-off made o.n unknown: {v}");
}

/// K2 derive keep: handed to a ⊤ callee, the layout is open (the reason
/// names that code).
#[test]
fn k2d_keep_top_callee_changes_the_layout() {
    let src = format!("{DECL}{OBJ}const f = get();\nf(o);\nconst x = /*A*/ o.m;\nconsole.log(x);\n");
    let v = site_after(&src, "/*A*/", "m", false);
    assert!(v.starts_with("No("), "a ⊤ callee's argument kept o's layout: {v}");
}
