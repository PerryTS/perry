//! N5 run-once flattening takes only closed bodies (decision 13): a
//! function named once in its module is a body that runs once there only
//! when nothing publishes it. An exported function, an exported arrow, one
//! stored into an exported object or the CommonJS export runs for callers
//! the module does not see; its flat copy would claim their bindings.

use super::*;
use perry_hir::{Expr, Module, Stmt};

fn hirs(mods: &[(&str, &str)]) -> Vec<(String, Module)> {
    mods.iter()
        .map(|(name, src)| {
            let path = format!("{name}.ts");
            let module = perry_parser::parse_typescript(src, &path).expect("parses");
            let mut hir = perry_hir::lower_module(&module, name, &path).expect("lowers");
            for imp in &mut hir.imports {
                if let Some(n) = imp.source.strip_prefix("./") {
                    imp.resolved_path = Some(format!("{n}.ts"));
                }
            }
            (path, hir)
        })
        .collect()
}

fn run(h: &[(String, Module)], unit: bool) -> ProgramFacts {
    let list: Vec<(String, &Module)> = h.iter().map(|(p, m)| (p.clone(), m)).collect();
    match unit {
        true => crate::compute_units(&list, &FactsConfig::default()),
        false => crate::compute(&list, &FactsConfig::default()),
    }
}

/// The `let`s of `m`'s module scope bound to a function literal and named
/// `name`, at any depth of its statements.
fn literal_lets(m: &Module, name: &str) -> usize {
    let mut n = 0;
    perry_hir::tdz_check::for_each_stmt_deep(&m.init, &mut |s| {
        n += matches!(s, Stmt::Let { name: l, init: Some(Expr::Closure { .. }), .. } if l == name)
            as usize
    });
    n
}

/// Whether the run-once view of `m` still declares the function or the
/// module-scope `let` named `name` (not copied into module scope).
fn kept(m: &Module, name: &str) -> bool {
    let flat = crate::decl_units::run_once_flat(m).unwrap_or_else(|| m.clone());
    flat.functions.iter().any(|f| f.name == name)
        || flat
            .init
            .iter()
            .any(|s| matches!(s, Stmt::Let { name: n, init: Some(Expr::Closure { .. }), .. } if n == name))
}

/// The number of bodies per-unit facts give the parameter `param` (of a
/// function or a function literal) in module `path`; `None` without facts.
fn param_bodies(
    pf: &ProgramFacts,
    h: &[(String, Module)],
    path: &str,
    param: &str,
) -> Option<usize> {
    let m = &h.iter().find(|(p, _)| p == path).expect("the module").1;
    let mut id = None;
    let mut look = |ps: &[perry_hir::Param]| {
        if let Some(p) = ps.iter().find(|p| p.name == param) {
            id = Some(p.id);
        }
    };
    for f in &m.functions {
        look(&f.params);
    }
    perry_hir::tdz_check::for_each_module_expr(m, &mut |e| {
        if let Expr::Closure { params, .. } = e {
            look(params);
        }
    });
    let id = id.expect("the parameter");
    pf.bindings_for(path)?
        .locals
        .iter()
        .find(|(l, _)| *l == id)
        .map(|(_, b)| b.bodies.len())
}

/// Per unit never claims more than whole program: a proven call there is
/// proven in whole program with no body the per-unit set lacks.
fn never_stronger(h: &[(String, Module)]) {
    let (u, w) = (run(h, true), run(h, false));
    let name = |t: &String| t.split('#').next().unwrap_or(t).to_string();
    for s in u
        .sites
        .iter()
        .filter(|s| s.is_call && !matches!(s.verdict, Verdict::No(_)))
    {
        let ws = w
            .sites
            .iter()
            .find(|x| (x.module, x.byte_offset, &x.key) == (s.module, s.byte_offset, &s.key))
            .unwrap_or_else(|| panic!("{s:?} in\n{}", w.dump_text()));
        assert!(
            !matches!(ws.verdict, Verdict::No(_)),
            "{s:?}\n{}",
            w.dump_text()
        );
        let have: Vec<String> = s.targets.iter().map(name).collect();
        assert!(
            ws.targets.iter().all(|t| have.contains(&name(t))),
            "per unit stronger: {s:?} vs {ws:?}\n{}",
            u.dump_text()
        );
    }
}

const CACHED: &str = r#"
    export function cached<T>(getter: () => T): () => T {
        return () => getter();
    }
    export const flag = cached(() => true);
"#;

const IMPORTER: &str = r#"
    import { cached } from "./util";
    const box = cached(() => ({ keys: ["a", "b"] }));
    console.log(typeof box());
"#;

#[test]
fn an_exported_function_called_once_in_its_module_is_not_flattened() {
    // The zod 4.4.3 shape: `cached` runs for the importer's getter too. Its
    // parameter never holds just the module's one closure (a direct call of
    // that closure for every caller was the miscompile).
    let h = hirs(&[("util", CACHED), ("main", IMPORTER)]);
    assert!(kept(&h[0].1, "cached"));
    let pf = run(&h, true);
    assert_ne!(
        param_bodies(&pf, &h, "util.ts", "getter"),
        Some(1),
        "{}",
        pf.dump_text()
    );
    never_stronger(&h);
}

#[test]
fn two_importers_each_keep_their_own_body() {
    let other = r#"
        import { cached } from "./util";
        export const named = cached(() => "other");
    "#;
    let h = hirs(&[("util", CACHED), ("other", other), ("main", IMPORTER)]);
    assert!(kept(&h[0].1, "cached"));
    let pf = run(&h, true);
    assert_ne!(
        param_bodies(&pf, &h, "util.ts", "getter"),
        Some(1),
        "{}",
        pf.dump_text()
    );
    never_stronger(&h);
}

#[test]
fn an_exported_arrow_is_not_flattened() {
    // A `let` bound to a literal, exported by name: no `FuncRef` names it,
    // so a test of the exported module functions misses it.
    let arrow = r#"
        export const lazy = <T,>(getter: () => T): (() => T) => () => getter();
        export const ready = lazy(() => 1);
    "#;
    let main = r#"
        import { lazy } from "./arrow";
        const late = lazy(() => ({ keys: ["c"] }));
        console.log(typeof late());
    "#;
    let h = hirs(&[("arrow", arrow), ("main", main)]);
    assert!(kept(&h[0].1, "lazy"));
    let pf = run(&h, true);
    assert_ne!(
        param_bodies(&pf, &h, "arrow.ts", "getter"),
        Some(1),
        "{}",
        pf.dump_text()
    );
    never_stronger(&h);
}

#[test]
fn a_function_stored_into_an_exported_object_is_not_flattened() {
    let stored = r#"
        function make<T>(getter: () => T): () => T {
            return () => getter();
        }
        export const api: any = {};
        api.make = make;
        export const made = make(() => "stored");
    "#;
    let main = r#"
        import { api } from "./stored";
        const via = api.make(() => ({ keys: ["d"] }));
        console.log(typeof via());
    "#;
    let h = hirs(&[("stored", stored), ("main", main)]);
    assert!(kept(&h[0].1, "make"));
    never_stronger(&h);
}

#[test]
fn a_function_passed_as_a_value_in_its_module_is_not_flattened() {
    let src = r#"
        function wrap<T>(getter: () => T): () => T {
            return () => getter();
        }
        const table: any[] = [];
        table.push(wrap);
        const once = wrap(() => "own");
        const other = table[0](() => ({ keys: ["f"] }));
        console.log(once(), typeof other());
    "#;
    let h = hirs(&[("passed", src)]);
    assert!(kept(&h[0].1, "wrap"));
    never_stronger(&h);
}

/// The pipeline's CommonJS shape (`cjs_wrap`): the factory, a local
/// function named at its call and by its module record.
fn cjs(inner: &str) -> String {
    format!(
        r#"
        const _cjs = (function () {{
            function __perry_cjs_factory() {{
                const module: any = {{ exports: {{}} }};
                {inner}
                return module.exports;
            }}
            __perry_register_path_module_partial("t.cjs", {{}}, __perry_cjs_factory);
            return __perry_cjs_factory();
        }})();
        export default _cjs;
        "#
    )
}

#[test]
fn the_commonjs_export_is_not_flattened() {
    // The factory runs once; `cached`, published through `module.exports`,
    // stays a declaration of the flat module scope.
    let twin = cjs(r#"
        function cached(getter: any) {
            return () => getter();
        }
        module.exports.cached = cached;
        module.exports.flag = cached(() => true);
    "#);
    let h = hirs(&[("twin", &twin)]);
    let flat = crate::decl_units::run_once_flat(&h[0].1).expect("the factory runs once");
    assert_eq!(
        literal_lets(&flat, "__perry_cjs_factory"),
        0,
        "{:#?}",
        flat.init
    );
    assert_eq!(literal_lets(&flat, "cached"), 1, "{:#?}", flat.init);
    never_stronger(&h);
}

#[test]
fn a_factory_held_beyond_its_record_is_not_flattened() {
    // Every use of the factory but its call is a module record; a value use
    // of it is a caller the module does not see.
    let src = cjs("module.exports.k = 1;").replace(
        "return __perry_cjs_factory();",
        "(globalThis as any).again = __perry_cjs_factory; return __perry_cjs_factory();",
    );
    let h = hirs(&[("held", &src)]);
    let flat = crate::decl_units::run_once_flat(&h[0].1).unwrap_or_else(|| h[0].1.clone());
    assert_eq!(
        literal_lets(&flat, "__perry_cjs_factory"),
        1,
        "{:#?}",
        flat.init
    );
}

#[test]
fn a_module_local_function_called_once_still_flattens() {
    // Closed and named once: its body is the module scope's, its private
    // literal keeps its proof.
    let src = r#"
        function setup() {
            const o = { k: 1 };
            return o;
        }
        const r = setup();
        console.log(r.k);
    "#;
    let h = hirs(&[("local", src)]);
    assert!(!kept(&h[0].1, "setup"));
    let pf = run(&h, true);
    let k: Vec<_> = pf.sites.iter().filter(|s| s.key == "k").collect();
    assert!(
        !k.is_empty() && k.iter().all(|s| !matches!(s.verdict, Verdict::No(_))),
        "{}",
        pf.dump_text()
    );
}

#[test]
#[should_panic(expected = "internal compiler error")]
fn a_copy_of_a_published_body_is_an_internal_compiler_error() {
    // The guard holds without the premise: flattening that takes every
    // function for closed copies the exported `cached`, whose emitted body
    // other modules call.
    let h = hirs(&[("util", CACHED)]);
    let m = &h[0].1;
    let all = m
        .functions
        .iter()
        .map(|f| ((true, f.id), (f.name.clone(), f.id)))
        .collect();
    crate::decl_units::flatten(m, &all);
}

#[test]
fn a_deep_chain_flattens_on_a_test_thread_stack() {
    // The run-once view copies the statements of the bodies it flattens. A
    // derived `Expr::clone` recursed per operand, about 16 KB a level in a
    // debug build: the 150 operands below took about 2.4 MB, beyond the 2 MB
    // of a test thread (the 64 of
    // `a_closed_local_called_only_in_its_unit_costs_its_own_stores` took 1.1).
    let n = 150;
    let sum: Vec<String> = (0..n).map(|i| format!("o.k{i}")).collect();
    let src = format!(
        "function run(o: any) {{ return {}; }}\nconst r = run({{}});\nconsole.log(r);\n",
        sum.join(" + ")
    );
    let h = hirs(&[("a", src.as_str())]);
    let flat = crate::decl_units::run_once_flat(&h[0].1).expect("`run` runs once");
    assert!(
        flat.functions.iter().all(|f| f.name != "run"),
        "`run` is flat"
    );
    // `r` is bound to the chain: n - 1 additions down its left spine.
    let r = flat
        .init
        .iter()
        .find_map(|s| match s {
            Stmt::Let {
                name,
                init: Some(e),
                ..
            } if name == "r" => Some(e),
            _ => None,
        })
        .expect("the `let` of `r`");
    let mut depth = 0;
    let mut at = r;
    while let Expr::Binary { left, .. } = at {
        depth += 1;
        at = left;
    }
    assert_eq!(depth, n - 1, "{at:?}");
}
