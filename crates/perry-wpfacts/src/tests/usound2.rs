//! USOUND2 (2026-11-16): the forms the audit2 "u" fixes keep exact.
//! `reaches_g`: after `src`, `f.a()` may call g (JavaScript ran `f.a = g`).
use super::*;

const P: &str = "function g(): any { return 2; }\nconst f: any = { a() { return 1; } };\nfunction cb(this: any, ...r: any[]): any { f.a = g; return 0; }\nconst o: any = { valueOf() { f.a = g; return 1; } };\n";

fn verdicts(src: &str) -> Vec<String> {
    let src = format!("{P}{src}\nf.a();\n");
    let path = "t.ts".to_string();
    let module = perry_parser::parse_typescript(&src, &path).expect("parses");
    let hir = perry_hir::lower_module(&module, "t", &path).expect("lowers");
    let pf = crate::compute(&[(path, &hir)], &FactsConfig::default());
    let mut s: Vec<_> = pf.sites.iter().filter(|s| s.key == "a" && s.is_call).collect();
    s.sort_by_key(|s| s.byte_offset);
    s.iter().map(|s| format!("{:?}", s.verdict)).collect()
}
fn reaches_g(src: &str) -> bool {
    !verdicts(src).iter().any(|v| v == "T0Mono")
}

/// 13.3.7.1 SuperCall: `super(x)` constructs the realm's Promise, whose
/// executor runs (27.2.3.1 step 9), spread or not.
#[test]
fn usound2_super_into_promise_runs_the_executor() {
    assert!(reaches_g("class Q extends Promise<any> { constructor(x: any) { super(x); } } new Q(cb);"));
}

/// KEEP: the direct `process.nextTick(cb)` still runs cb; a spread of
/// Numbers into `Math.max` runs no program code: `f.a()` stays proven.
#[test]
fn usound2_keep_direct_tick_and_plain_spread() {
    assert!(reaches_g("process.nextTick(cb);"));
    assert!(!reaches_g("const xs: number[] = [Number(process.argv[2]), 2]; Math.max(...xs);"));
    assert!(!reaches_g("const xs: number[] = [Number(process.argv[2]), 2]; process.nextTick(...([(): number => 0] as [any]));"));
}

/// KEEP: a promise that never reaches unknown code hands its value to no
/// one; its own reaction sees it (`f.a()` stays the literal method).
#[test]
fn usound2_keep_local_promise() {
    assert!(!reaches_g("Promise.resolve(f).then((x: any) => x);"));
    assert!(!reaches_g("async function h() { return f; } h();"));
}

/// KEEP: a typed-array store under a Number key of a Number runs no program
/// code (10.4.5.16 ToNumber of a Number); a named key is an OrdinarySet
/// (no ToNumber); a Number key of an object does run `valueOf`.
#[test]
fn usound2_keep_typed_array_number_store() {
    assert!(!reaches_g("const te = new Float32Array(16); const x = Number(process.argv[2]); for (let i = 0; i < 16; i++) { te[i] = x * i; }"));
    assert!(!reaches_g("const ta: any = new Float32Array(4); ta.foo = o;"));
    assert!(reaches_g("const ta: any = new Float32Array(4); const k = Math.floor(Math.random() * 2); ta[k] = o;"));
    assert!(reaches_g("const ta: any = new Float32Array(4); ta[String(Math.random())] = o;"));
}
