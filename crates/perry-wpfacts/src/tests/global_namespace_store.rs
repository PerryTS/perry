//! DEC 255: a store into the global object under a key that may name a
//! namespace (`Object`, `Date`, ...) is seen by the namespace call that reads
//! the binding. Each row stores a value whose `keys`/`now` may call `load`
//! (the dynamic import) and then makes the namespace call: the import must be
//! live exactly when the stored value can reach it. Modules go through the
//! driver's order: every module is scanned (`scan_patched_builtins`), then
//! lowered under the program's set, then solved.

use super::*;

const LIB: &str = "export const v = 1;\n";

fn program(main: &str, interp: bool) -> Vec<(String, perry_hir::Module)> {
    let srcs = [("fmtlib", LIB.to_string()), ("main", main.to_string())];
    let asts: Vec<_> = srcs
        .iter()
        .map(|(n, s)| perry_parser::parse_typescript(s, &format!("{n}.ts")).expect("parses"))
        .collect();
    let mut pb = perry_hir::patched_builtins::PatchedBuiltins::new();
    for a in &asts {
        perry_hir::scan_patched_builtins(a, &mut pb);
    }
    if interp {
        pb.dynamic_code_reach
            .record_global("new Function(...) @ x.ts (byte 1) (names globals)".into());
    }
    perry_hir::patched_builtins::set_patched_builtins(std::sync::Arc::new(pb));
    let out = srcs
        .iter()
        .zip(&asts)
        .map(|((n, _), a)| {
            let path = format!("{n}.ts");
            let mut h = perry_hir::lower_module(a, n, &path).expect("lowers");
            let mut sites = 0;
            perry_hir::for_each_dynamic_import_mut(&mut h, &mut |e| {
                if let perry_hir::Expr::DynamicImport { paths, .. } = e {
                    *paths = vec!["./fmtlib.js".to_string()];
                    sites += 1;
                }
            });
            if sites > 0 {
                h.imports.push(perry_hir::Import {
                    is_deferred: false,
                    source: "./fmtlib.js".to_string(),
                    specifiers: Vec::new(),
                    is_native: false,
                    module_kind: Default::default(),
                    resolved_path: Some("fmtlib.ts".to_string()),
                    type_only: false,
                    runtime_erased: false,
                    is_dynamic: true,
                    is_dynamic_target: false,
                    is_deferred_require: false,
                    is_adopted_require: false,
                });
            }
            (path, h)
        })
        .collect();
    perry_hir::patched_builtins::clear_patched_builtins();
    out
}

fn live(tail: &str, interp: bool) -> bool {
    let main = format!(
        "const load = () => import(\"./fmtlib.js\");\n\
         const unknown: any = (globalThis as any)[process.argv[2]];\n{tail}\nconsole.log(1);\n"
    );
    let hirs = program(&main, interp);
    let list: Vec<(String, &perry_hir::Module)> =
        hirs.iter().map(|(p, h)| (p.clone(), h)).collect();
    let mut cfg = FactsConfig::default();
    if interp {
        cfg.dynamic_code = Some("new Function(...) @ x.ts (names globals)".into());
    }
    let a = crate::activation(&list, &cfg);
    let l = a.live_dynamic_imports().expect("in bound");
    l.contains(&("main.ts".to_string(), "./fmtlib.js".to_string()))
}

const CALLS: &str = "const w: any = { keys(f: any) { f(); return []; } };";

fn check(rows: &[(&str, String, bool)]) {
    let mut bad = Vec::new();
    for interp in [false, true] {
        for (name, tail, want) in rows {
            let got = live(tail, interp);
            if got != *want {
                bad.push(format!(
                    "{name} (interp={interp}): want live={want}, got {got}"
                ));
            }
        }
    }
    assert!(bad.is_empty(), "{}", bad.join("\n"));
}

/// The rows main already decides right, the d rows (hunk 1): a computed key
/// and `Object.assign` onto the global object guard the call, so its generic
/// arm reads the stored value (both d rows are dead on main), and the a5 rows
/// (hunk 2): the strict existing-global store `Object = w` (lowered to
/// `js_global_assign_existing_or_throw`) is a store into the global object
/// the arm reads (dead before hunk 2: the extern was unmodeled). The a5 rows
/// carry `export {}`: the module goal makes the module strict; a sloppy
/// `Object = w` lowers to a property set that main already sees. The a3 rows
/// (hunk 3): a parenthesized or TS-as target `(Object as any) = w` is the same
/// store, strict and sloppy (dead before hunk 3: the scan did not read the
/// wrapped target, so the call stayed Plain).
#[test]
fn global_namespace_store_reaches_the_namespace_call() {
    let date = "function createDate(D: any) { return class extends D { static now() { load(); return D.now(); } }; }";
    check(&[
        ("control: intrinsic keys never calls", "Object.keys(load as any);".into(), false),
        ("control: load handed to unknown", "unknown.fn(load);".into(), true),
        (
            "a1 static store of an object that calls",
            format!("{CALLS}\n(globalThis as any).Object = w;\nObject.keys(load as any);"),
            true,
        ),
        (
            "a4 class wrapper Date",
            format!("{date}\n(globalThis as any).Date = createDate(Date);\nconsole.log(Date.now());"),
            true,
        ),
        (
            "b1 unknown value stored",
            "(globalThis as any).Object = unknown.o;\nObject.keys(load as any);".into(),
            true,
        ),
        (
            "b2 unknown, called through globalThis.Object",
            "(globalThis as any).Object = unknown.o;\n(globalThis as any).Object.keys(load as any);"
                .into(),
            true,
        ),
        (
            "a5 strict bare `Object = w` of an object that calls",
            format!("export {{}};\n{CALLS}\nObject = w;\nObject.keys(load as any);"),
            true,
        ),
        (
            "a5' strict bare `Object = unknown`",
            "export {};\nObject = unknown.o;\nObject.keys(load as any);".into(),
            true,
        ),
        (
            "a3 strict `(Object as any) = w` of an object that calls",
            format!("export {{}};\n{CALLS}\n(Object as any) = w;\nObject.keys(load as any);"),
            true,
        ),
        (
            "a3' sloppy `(Object as any) = unknown`",
            "(Object as any) = unknown.o;\nObject.keys(load as any);".into(),
            true,
        ),
        (
            "d1 computed-key store",
            "(globalThis as any)[process.argv[3]] = unknown.o;\nObject.keys(load as any);".into(),
            true,
        ),
        (
            "d1' computed-key store of a compiled object",
            format!("{CALLS}\n(globalThis as any)[process.argv[3]] = w;\nObject.keys(load as any);"),
            true,
        ),
        (
            "d2 Object.assign onto the global object",
            "Object.assign(globalThis, unknown.shims);\nObject.keys(load as any);".into(),
            true,
        ),
        (
            "e1 store a number",
            "(globalThis as any).Object = 5;\nObject.keys(load as any);".into(),
            false,
        ),
        (
            "e2 store Object itself",
            "(globalThis as any).Object = Object;\nObject.keys(load as any);".into(),
            false,
        ),
        (
            "e3 store w, call another namespace",
            "const w: any = { keys(f: any) { return []; } };\n(globalThis as any).Object = w;\nMath.max(load as any);"
                .into(),
            false,
        ),
    ]);
}
