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
