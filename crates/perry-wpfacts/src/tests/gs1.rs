//! GLOBS1 (2026-11-16): a computed read on the global object under a known
//! key shape (`globalThis["on" + type]`). The global object is an ordinary
//! object (ECMA-262 9.3, 19): its own keys are the realm's declared layout
//! (perry-runtime global_this/populate.rs) and what the program stores; a
//! miss reads `Object.prototype` (10.1.8.1). So the read is the union of the
//! named reads of every declared or program global the shape can spell, the
//! stores under unknown String keys, `Object.prototype` under the key, and
//! `undefined`. A fully unknown key stays ⊤.
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
    eprintln!("GLOBS1 {key}: {v:?}");
    assert!(!v.is_empty(), "no call site of {key}");
    v
}

fn mono(v: &[String]) -> bool {
    v.iter().all(|x| x == "T0Mono" || x.starts_with("T0Mono("))
}

const HEAD: &str = "class C { m() { return 1; } }\n\
const q: any = new C();\n\
const ty = String(process.argv[2] ?? \"x\");\n\
const g: any = globalThis;\n";

/// RED: `"on" + type` spells no declared global and no program global:
/// the read is `undefined` (or what `Object.prototype` holds under such a
/// key: nothing), so the guarded call hands `q` to nothing.
#[test]
fn gs1_on_shape_reads_no_unknown_code() {
    let src = format!(
        "{HEAD}const h = g[\"on\" + ty];\n\
         if (typeof h === \"function\") h.call(g, q);\nq.m();\n"
    );
    assert!(mono(&calls(&src, "m")), "q.m() stays C::m");
}

/// KEEP: a program global the shape spells, stored after the read (and in
/// a function run later): the read sees it, so the call may replace q.m.
#[test]
fn gs1_program_global_stored_after_the_read() {
    let src = format!(
        "{HEAD}function get(): any {{ return g[\"on\" + ty]; }}\n\
         const h = get();\n\
         function install() {{ g.onfoo = function (e: any) {{ e.m = function () {{ return 2; }}; }}; }}\n\
         install();\n\
         if (typeof h === \"function\") h(q);\nq.m();\n"
    );
    assert!(!mono(&calls(&src, "m")), "the stored onfoo may replace q.m");
}

/// KEEP: a shape that spells a declared global function (`"set" +
/// "Timeout"`): the read is that function, which calls its callback.
#[test]
fn gs1_shape_spells_a_declared_global() {
    let src = format!(
        "{HEAD}const f = g[\"set\" + ty];\n\
         if (typeof f === \"function\") f(function () {{ q.m = function () {{ return 2; }}; }}, 0);\n\
         q.m();\n"
    );
    assert!(
        !mono(&calls(&src, "m")),
        "setTimeout's callback may replace q.m"
    );
}

/// KEEP: a shape that also spells an `Object.prototype` member the
/// program stored (`"to" + x`): a global miss reads it.
#[test]
fn gs1_shape_reads_object_prototype() {
    let src = format!(
        "{HEAD}(Object.prototype as any).toZed = function (e: any) {{ e.m = function () {{ return 2; }}; }};\n\
         const h = g[\"to\" + ty];\n\
         if (typeof h === \"function\") h(q);\nq.m();\n"
    );
    assert!(
        !mono(&calls(&src, "m")),
        "Object.prototype.toZed may replace q.m"
    );
}

/// KEEP: a fully unknown key stays ⊤.
#[test]
fn gs1_unknown_key_stays_top() {
    let src = format!(
        "{HEAD}const h = g[ty];\n\
         if (typeof h === \"function\") h(q);\nq.m();\n"
    );
    assert!(!mono(&calls(&src, "m")), "an unknown key may read anything");
}

/// KEEP: a program store under an unknown String key is read too.
#[test]
fn gs1_unknown_key_store_is_read() {
    let src = format!(
        "{HEAD}g[String(Math.random())] = function (e: any) {{ e.m = function () {{ return 2; }}; }};\n\
         const h = g[\"on\" + ty];\n\
         if (typeof h === \"function\") h(q);\nq.m();\n"
    );
    assert!(!mono(&calls(&src, "m")), "the unknown-key store may be onx");
}
