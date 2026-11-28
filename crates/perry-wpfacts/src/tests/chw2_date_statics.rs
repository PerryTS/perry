//! CHW2: a Date static called through the constructor. A program that
//! writes `Date.now` (a virtual clock: the chess-headless harness) lowers
//! every `Date.now()` as the member call `Date.now()` on the live
//! constructor (perry-hir `try_patched_builtin_call`). The read gives the
//! realm's own `Date.now` and the stored replacement; calling the realm's
//! own function is its steps (a Number), not unknown code: the realm is
//! not held. A replacement unknown code may supply still holds it.
use super::*;

/// Lower `src` the way the compiler does (the patched-builtins scan
/// first), then solve the whole program.
fn solved(src: &str) -> crate::gen::Gen<'static> {
    let ast = Box::leak(Box::new(
        perry_parser::parse_typescript(src, "t.ts").expect("parses"),
    ));
    let mut set = perry_hir::PatchedBuiltins::new();
    perry_hir::scan_patched_builtins(ast, &mut set);
    perry_hir::set_patched_builtins(std::sync::Arc::new(set));
    let hir = perry_hir::lower_module(ast, "t", "t.ts");
    perry_hir::clear_patched_builtins();
    let hir = Box::leak(Box::new(hir.expect("lowers")));
    let mut g = crate::gen::Gen::new(vec![("t.ts", &*hir)], false, false);
    g.m.max_work = 50_000_000;
    g.register_all();
    g.link_exports();
    g.walk_all();
    g.feed_parent_values();
    g.m.summarize_and_replay();
    g.m.solve();
    g
}

fn held(src: &str) -> bool {
    solved(src).m.realm_held
}

const CLOCK: &str = "
const clock = { ms: 0 };
(Date as any).now = (): number => 1000 + clock.ms;
";

/// RED: the virtual clock's reads of `Date.now()` do not hold the realm.
#[test]
fn chw2_written_date_now_call_is_the_realm_function() {
    let src = format!("{CLOCK}let last = Date.now();\nexport function tick(): number {{ const n = Date.now(); const d = n - last; last = n; clock.ms += 16; return d; }}\n");
    assert!(!held(&src), "{src}");
}

/// RED: `Date.parse` / `Date.UTC` through a written constructor are the
/// realm's steps as well.
#[test]
fn chw2_written_date_parse_utc_calls() {
    let src = "(Date as any).parse = (s: string): number => 0;\n(Date as any).UTC = (y: number): number => 0;\nexport const a = Date.parse(\"2020-01-01\") + Date.UTC(2020, 1);\n";
    assert!(!held(src), "{src}");
}

fn lower(src: &str) -> perry_hir::Module {
    let ast = perry_parser::parse_typescript(src, "t.ts").expect("parses");
    let mut set = perry_hir::PatchedBuiltins::new();
    perry_hir::scan_patched_builtins(&ast, &mut set);
    perry_hir::set_patched_builtins(std::sync::Arc::new(set));
    let hir = perry_hir::lower_module(&ast, "t", "t.ts");
    perry_hir::clear_patched_builtins();
    hir.expect("lowers")
}

fn facts_of(hir: &perry_hir::Module) -> ProgramFacts {
    let cfg = FactsConfig {
        max_work: Some(50_000_000),
        ..FactsConfig::default()
    };
    crate::compute(&[("t.ts".to_string(), hir)], &cfg)
}

/// Does function `hit` run?
fn hit_runs(src: &str) -> bool {
    let hir = lower(src);
    let pf = facts_of(&hir);
    let id = hir
        .functions
        .iter()
        .find(|f| f.name == "hit")
        .expect("hit")
        .id;
    pf.body_reach.bodies.iter().any(|(_, i, _)| *i == id)
}

/// KEEP: the program's replacement is still the callee besides the
/// realm's function: what it calls runs. Control: without the call,
/// `hit` does not run.
#[test]
fn chw2_keep_replacement_body_runs() {
    let head = "function hit(): number { return 1; }\n(Date as any).now = (): number => hit();\n";
    assert!(hit_runs(&format!("{head}export const t = Date.now();\n")));
    assert!(
        !hit_runs(&format!("{head}export const t = 0;\n")),
        "control"
    );
}

/// Unknown code that holds `Date` (the runtime and the host, closed world,
/// owner 2026-11-14) never replaces `now` (owner premise 2026-11-19): the
/// callees are the program's arrow and the realm's `Date.now`, both give a
/// Number, and `v.k` on it is proven.
#[test]
fn chw2_unknown_holder_keeps_now() {
    let src = "declare function sink(x: any): void;\n(Date as any).now = (): number => 0;\nsink(Date);\nconst v: any = Date.now();\nexport const r = v.k;\n";
    let hir = lower(src);
    let pf = facts_of(&hir);
    let at = src.find("v.k").unwrap() as u32;
    let sites: Vec<_> = pf
        .sites
        .iter()
        .filter(|s| {
            !s.is_call && s.key == "k" && s.byte_offset <= at + 3 && s.byte_offset + 8 >= at
        })
        .collect();
    assert!(!sites.is_empty(), "no site v.k\n{}", pf.dump_text());
    assert!(
        sites.iter().all(|s| !matches!(s.verdict, Verdict::No(_))),
        "v.k unproven: {:?}",
        sites.iter().map(|s| &s.verdict).collect::<Vec<_>>()
    );
}
