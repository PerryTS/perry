//! DTHEN (LADDER c17-A5): a promise resolved with a Date. The resolve
//! function reads the value's `then` (ECMA-262 27.2.1.3.2 step 9).
//! `Date.prototype` owns no `then` (21.4.4) and neither does its chain
//! (`Object.prototype`, 20.1.3), so with no program store under `then` the
//! read is `undefined`, the promise fulfils with the Date (step 12) and no
//! unknown code runs. A program store under `then` is called as it is.
use super::w3l::solve;
use super::*;

/// The facts of `src` through the compile's own pipeline (`compute`).
fn facts_of(src: &str) -> ProgramFacts {
    let path = "main.ts".to_string();
    let module = perry_parser::parse_typescript(src, &path).expect("parses");
    let hir = perry_hir::lower_module(&module, "main", &path).expect("lowers");
    let list: Vec<(String, &perry_hir::Module)> = vec![(path, &hir)];
    let mut cfg = FactsConfig::default();
    cfg.max_work.get_or_insert(50_000_000);
    crate::compute(&list, &cfg)
}

/// `resolve` runs, then a builtin method call hands `n` to native code:
/// with the realm held that call is unknown and `n` escapes, so `n.v` at
/// `/*A*/` is proven only when the resolve ran no unknown code.
fn witness(resolve: &str) -> (Verdict, String) {
    let src = format!(
        "class N {{ v = 1; }}
        {resolve}
        const n = new N();
        [n].forEach((x) => console.log(x.v));
        console.log(/*A*/ n.v);"
    );
    let pf = facts_of(&src);
    let at = src.find("/*A*/").expect("marker") as u32;
    let s = pf
        .sites
        .iter()
        .filter(|s| s.key == "v" && s.byte_offset >= at)
        .min_by_key(|s| s.byte_offset)
        .unwrap_or_else(|| panic!("site .v after /*A*/ in\n{}", pf.dump_text()));
    (s.verdict.clone(), pf.dump_text())
}

/// RED before DTHEN: `Promise.resolve(new Date())` (pair c17-A5, pn62_m5r).
#[test]
fn dthen_promise_resolve_of_a_date_keeps_the_witness() {
    let (v, dump) = witness("const p = Promise.resolve(new Date());");
    assert!(
        !matches!(v, Verdict::No(_)),
        "n.v proven, got {v:?}\n{dump}"
    );
}

/// RED before DTHEN: `resolve(new Date())` in an executor (pn62 m5n).
#[test]
fn dthen_resolver_given_a_date_keeps_the_witness() {
    let (v, dump) = witness("const p = new Promise((resolve) => { resolve(new Date()); });");
    assert!(
        !matches!(v, Verdict::No(_)),
        "n.v proven, got {v:?}\n{dump}"
    );
}

/// The app chain (pair c17-A): a Worker's message listener resolves with
/// `e.data`, the structured clone of what the worker module posts: a Date.
#[test]
fn dthen_worker_message_date_holds_nothing() {
    let main = "import type {} from \"./w.ts\";\n\
        const worker = new Worker(new URL(\"./w.ts\", import.meta.url), { type: \"module\" });\n\
        const p = new Promise((resolve) => {\n\
          worker.addEventListener(\"message\", (e: any) => { resolve(e.data); });\n\
        });\n\
        p.then((v) => console.log(v));\n\
        worker.postMessage(1);\n";
    let w = "postMessage(new Date());\nexport {};\n";
    let (tops, held, _, _) = solve(&[("main", main), ("w", w)], &["w.ts"], &["./w.ts"]);
    assert!(!held, "{tops:?}");
}

/// KEEP: a program function stored as `Date.prototype.then` is the Date's
/// `then`: the resolve function calls it (`f` receives 1).
#[test]
fn dthen_stored_date_then_is_called() {
    let main = "function f(x: any): void { console.log(x); }\n\
        (Date.prototype as any).then = function (res: any) { f({ k: 1 }); res(2); };\n\
        const p = Promise.resolve(new Date());\n";
    let (tops, _, _, ps) = solve(&[("main", main)], &[], &[]);
    assert_eq!(ps, vec![1], "{tops:?}");
}
