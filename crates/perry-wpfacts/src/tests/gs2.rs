//! GLOBS3 (2026-11-17): the own fields a computed read of the global object
//! takes under a key of known shape (`globalThis["on" + type]`). The global
//! object is ordinary (ECMA-262 9.3, 19): `O[P]` is ToPropertyKey, then
//! OrdinaryGet (13.3.2.1, 7.3.2, 10.1.8.1), so the read yields the own
//! property the key names and the chain only. The key's shape picks the
//! program globals it can spell (`key_fields`, `global_read_split`), not
//! every field of the global object; a fully unknown key keeps them all.
use super::*;

fn one(src: &str) -> ProgramFacts {
    let path = "t.ts".to_string();
    let module = perry_parser::parse_typescript(src, &path).expect("parses");
    let hir = perry_hir::lower_module(&module, "t", &path).expect("lowers");
    crate::compute(&[(path, &hir)], &FactsConfig::default())
}

fn calls(src: &str, key: &str) -> Vec<String> {
    let pf = one(src);
    let mut s: Vec<_> = pf
        .sites
        .iter()
        .filter(|s| s.key == key && s.is_call)
        .collect();
    s.sort_by_key(|s| s.byte_offset);
    let v: Vec<String> = s.iter().map(|s| format!("{:?}", s.verdict)).collect();
    eprintln!("GLOBS3 {key}: {v:?}");
    assert!(!v.is_empty(), "no call site of {key}");
    v
}

fn mono(v: &[String]) -> bool {
    v.iter().all(|x| x == "T0Mono" || x.starts_with("T0Mono("))
}

/// The global holds itself under `win`, a class under `A`, and a global
/// function `call` that replaces its second argument's `m`.
const HEAD: &str = "class C { m() { return 1; } }\n\
class A { x = 1; }\n\
const q: any = new C();\n\
const ty = String(process.argv[2] ?? \"x\");\n\
const g: any = globalThis;\n\
g.A = A;\n\
g.win = g;\n\
g.call = function (_t: any, e: any) { e.m = function () { return 2; }; };\n";

/// RED: `"on" + ty` spells neither `win` nor `A` nor `call`: the read is
/// not the global object, so `h.call` never runs the global `call`.
#[test]
fn gs2_shape_skips_fields_it_cannot_spell() {
    let src = format!(
        "{HEAD}const h = g[\"on\" + ty];\n\
         if (typeof h === \"function\") h.call(g, q);\nq.m();\n"
    );
    assert!(mono(&calls(&src, "m")), "q.m() stays C::m");
}

/// KEEP: a program global the shape spells holds the global object: the
/// read takes it, and `h.call` runs the global `call`.
#[test]
fn gs2_spelled_global_holding_the_global_is_kept() {
    let src = format!(
        "{HEAD}g.onwin = g;\n\
         const h = g[\"on\" + ty];\n\
         if (typeof h === \"function\") h.call(g, q);\nq.m();\n"
    );
    assert!(!mono(&calls(&src, "m")), "g.onwin.call is g.call");
}

/// KEEP: a fully unknown key reads every own field of the global object.
#[test]
fn gs2_unknown_key_keeps_every_field() {
    let src = format!(
        "{HEAD}const h = g[ty];\n\
         if (typeof h === \"function\") h.call(g, q);\nq.m();\n"
    );
    assert!(!mono(&calls(&src, "m")), "g[ty] may be the global object");
}

/// KEEP: a computed method call on the global under the shape calls the
/// spelled program global, which replaces `q.m`.
#[test]
fn gs2_spelled_method_call_runs() {
    let src = format!(
        "{HEAD}g.onfoo = function (e: any) {{ e.m = function () {{ return 2; }}; }};\n\
         g[\"on\" + ty](q);\nq.m();\n"
    );
    assert!(!mono(&calls(&src, "m")), "g.onfoo may replace q.m");
}
