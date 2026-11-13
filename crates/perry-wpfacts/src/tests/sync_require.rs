//! A synchronous CommonJS `require(expr)` (HIR `DynamicImport { synchronous:
//! true }`, codegen `lower_dynamic_require`) returns the target value itself:
//! no promise and no `then` adoption (node's CJS loader never calls `then` on
//! `module.exports`). The model wrapped it as an `import()` promise, so its
//! no-match fallback's ⊤ got `then(resolve, reject)` with the program-wide
//! rejecter: the rejecter escaped and every `catch` in the program read ⊤
//! (tssrc `compiler/sys.ts` `require(modulePath)`: the THROWN blocker).

use super::*;
use crate::SiteFacts;

/// A module whose namespace has an own `then`.
const T: &str = r#"
export const V = 1;
export function then(resolve: any) { return resolve; }
"#;
/// A module whose body throws a program object.
const BOOM: &str = r#"
export class Boom { why = "init"; }
throw new Boom();
"#;

/// `load` has a `require(p)` (or, `synchronous` false, an ESM `import(p)`)
/// in a try; `f` is an unrelated catch.
const FIXTURE: &str = r#"
function load(p: any) { try { return import(p); } catch (e: any) { return e.message; } }
function f(o: any) { try { return o.k; } catch (e: any) { return e.message; } }
load(process.argv[2]); f({ k: 1 });
"#;
const THEN: &str = r#"
function load(p: any) { try { return import(p); } catch (e: any) { return undefined; } }
const m = load("./t.js");
console.log(m.V);
"#;
const INIT_THROWS: &str = r#"
function load(p: any) { try { return import(p); } catch (e: any) { return e.why; } }
console.log(load("./boom.js"));
"#;

/// The program as the compile driver hands it over (as in
/// `dynamic_import::program`): each `import()` site's `paths` filled, each
/// target an `is_dynamic` import entry resolved to `<name>.ts`, and every
/// site made a synchronous require when `sync`.
fn program(main: &str, paths: &[&str], sync: bool) -> ProgramFacts {
    let hirs: Vec<(String, perry_hir::Module)> = [("t", T), ("boom", BOOM), ("main", main)]
        .iter()
        .map(|(name, src)| {
            let path = format!("{name}.ts");
            let module = perry_parser::parse_typescript(src, &path).expect("parses");
            let mut hir = perry_hir::lower_module(&module, name, &path).expect("lowers");
            let mut sites = 0;
            perry_hir::for_each_dynamic_import_mut(&mut hir, &mut |e| {
                if let perry_hir::Expr::DynamicImport {
                    paths: p,
                    synchronous,
                    ..
                } = e
                {
                    *p = paths.iter().map(|s| s.to_string()).collect();
                    *synchronous = sync;
                    sites += 1;
                }
            });
            if sites > 0 {
                for source in paths {
                    hir.imports.push(perry_hir::Import {
                        is_deferred: false,
                        source: source.to_string(),
                        specifiers: Vec::new(),
                        is_native: false,
                        module_kind: Default::default(),
                        resolved_path: Some(format!(
                            "{}.ts",
                            source.trim_start_matches("./").trim_end_matches(".js")
                        )),
                        type_only: false,
                        runtime_erased: false,
                        is_dynamic: true,
                        is_dynamic_target: false,
                        is_deferred_require: false,
                        is_adopted_require: false,
                    });
                }
            }
            (path, hir)
        })
        .collect();
    let list: Vec<(String, &perry_hir::Module)> =
        hirs.iter().map(|(p, h)| (p.clone(), h)).collect();
    let cfg = FactsConfig {
        sabotage: std::env::var("WPFACTS_TEST_SABOTAGE").ok(),
        ..Default::default()
    };
    crate::compute(&list, &cfg)
}

/// The `key` read sites of main.ts, in source order.
fn reads<'p>(pf: &'p ProgramFacts, key: &str) -> Vec<&'p SiteFacts> {
    let main = pf
        .module_paths
        .iter()
        .position(|p| p == "main.ts")
        .expect("main.ts") as u32;
    let mut v: Vec<&SiteFacts> = pf
        .sites
        .iter()
        .filter(|s| s.module == main && s.key == key && !s.is_call)
        .collect();
    v.sort_by_key(|s| s.byte_offset);
    v
}

/// Whether t.ts's `then` body ran.
fn then_runs(pf: &ProgramFacts) -> bool {
    let module = perry_parser::parse_typescript(T, "t.ts").expect("parses");
    let hir = perry_hir::lower_module(&module, "t", "t.ts").expect("lowers");
    let id = hir
        .functions
        .iter()
        .find(|f| f.name == "then")
        .expect("then")
        .id;
    pf.body_reach
        .bodies
        .iter()
        .any(|(p, i, _)| p == "t.ts" && *i == id)
}

/// Red on main: the unrelated catch in `f` reads ⊤ (the require's ⊤
/// fallback got `then(resolve, reject)` with the program-wide rejecter).
#[test]
fn a_sync_require_leaves_unrelated_catches_known() {
    let pf = program(FIXTURE, &[], true);
    let m = reads(&pf, "message");
    assert_eq!(m.len(), 2, "two e.message sites\n{}", pf.dump_text());
    let f = m[1];
    assert!(
        !f.top && f.receivers.iter().all(|r| r == "obj:runtime:Error"),
        "f's catch: {:?} top={}\n{}",
        f.receivers,
        f.top,
        pf.dump_text()
    );
}

/// The require's own failure (MODULE_NOT_FOUND) reaches its catch: `e` in
/// `load` holds the realm error (red on main: ⊤).
#[test]
fn a_failing_sync_require_throws_a_runtime_error_into_its_catch() {
    let pf = program(FIXTURE, &[], true);
    let l = reads(&pf, "message")[0];
    assert!(
        !l.top && l.receivers.iter().any(|r| r == "obj:runtime:Error"),
        "load's catch: {:?} top={}\n{}",
        l.receivers,
        l.top,
        pf.dump_text()
    );
}

/// A required module's body runs at the site: what it throws reaches the
/// site's catch (node: `require` rethrows the module's evaluation error
/// synchronously).
#[test]
fn a_sync_required_module_body_throw_reaches_the_catch() {
    let pf = program(INIT_THROWS, &["./boom.js"], true);
    let w = reads(&pf, "why");
    assert_eq!(w.len(), 1, "one e.why site\n{}", pf.dump_text());
    assert!(
        !w[0].top && w[0].receivers.iter().any(|r| r.contains("Boom")),
        "load's catch: {:?} top={}\n{}",
        w[0].receivers,
        w[0].top,
        pf.dump_text()
    );
}

/// node: `require` never calls `then` on the module value. Red on main
/// (the adoption called the namespace's `then`).
#[test]
fn a_sync_require_never_calls_the_modules_then() {
    let pf = program(THEN, &["./t.js"], true);
    assert!(pf.body_reach.complete, "activated");
    assert!(!then_runs(&pf), "t.then ran\n{:?}", pf.body_reach.bodies);
    // The value is the namespace itself.
    let v = reads(&pf, "V");
    assert!(
        v.iter()
            .any(|s| s.receivers.iter().any(|r| r.starts_with("ns:"))),
        "m.V reads t's namespace: {:?}",
        v.iter().map(|s| &s.receivers).collect::<Vec<_>>()
    );
}

/// Control: the same program with an ESM `import(p)` still adopts its ⊤
/// fallback (a promise: `then(resolve, reject)` runs on it), so the shared
/// rejecter still escapes and the unrelated catch stays ⊤ (per-promise
/// rejection is a separate rule).
#[test]
fn an_esm_import_of_the_fallback_still_adopts() {
    let pf = program(FIXTURE, &[], false);
    let m = reads(&pf, "message");
    assert_eq!(m.len(), 2, "two e.message sites\n{}", pf.dump_text());
    assert!(
        m[1].top,
        "f's catch not ⊤: {:?}\n{}",
        m[1].receivers,
        pf.dump_text()
    );
}

/// Control: an ESM `import()` of a module exporting `then` adopts it (node:
/// the namespace is a thenable, `then` runs).
#[test]
fn an_esm_import_calls_the_modules_then() {
    let pf = program(THEN, &["./t.js"], false);
    assert!(
        then_runs(&pf),
        "t.then did not run\n{:?}",
        pf.body_reach.bodies
    );
}
