//! Local require must observe deletion and replacement of live cache records.
use super::wrap_commonjs;

fn wrapped(source: &str) -> String {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("dep.cjs"), "module.exports = { value: 1 };").unwrap();
    let result = wrap_commonjs(source, &dir.path().join("entry.cjs"));
    perry_parser::parse_typescript(&result, "wrapped.ts").expect("wrapper parses");
    result
}

/// The edge function of `require('./dep.cjs')`, split into its arm for a
/// program that is not closed and its arm for a closed one.
fn edge_arms(source: &str) -> (&str, &str) {
    let start = source
        .find("function __perry_require_static_0() {")
        .expect("the edge");
    let body = &source[start..];
    let body = &body[..body.find("\n}\n").expect("its end")];
    let split = body.find("} else {").expect("two arms");
    (&body[..split], &body[split..])
}

#[test]
fn a_lazy_require_is_the_static_edge() {
    let source = wrapped("module.exports = function load() { return require('./dep.cjs'); };");
    assert!(
        source.contains("return __perry_require_static_0();"),
        "{source}"
    );
    assert!(!source.contains("__perry_cjs_require_deferred"), "{source}");
}

#[test]
fn only_a_program_that_is_not_closed_reads_the_live_cache() {
    let source =
        wrapped("const first = require('./dep.cjs'); module.exports = require('./dep.cjs');");
    let (open, closed) = edge_arms(&source);
    // #11249: an edge outlives `require.cache` deletion and replacement only
    // where a module can write the cache: there it reads the live record.
    assert!(
        open.starts_with("function __perry_require_static_0() {\n    if (__perry_cjs_open()) {"),
        "{open}"
    );
    assert!(open.contains("const cached = __perry_cjs_cache["), "{open}");
    assert!(open.contains("return cached.exports;"), "{open}");
    // A fresh record is linked once, by the pair flag
    // (Node pushes a new Module without a scan).
    assert!(open.contains("if (__perry_cjs_lk_0 === false) {"), "{open}");
    assert!(
        open.contains("__perry_cjs_record.children.push(record);"),
        "{open}"
    );
    // Closed: the record binding is the link, one flag per pair, no scan.
    assert!(!closed.contains("__perry_cjs_cache"), "{closed}");
    assert!(
        !closed.contains("indexOf") && !closed.contains("__perry_cjs_link"),
        "{closed}"
    );
    assert!(
        closed.contains("const record = __perry_cjs_init(__perry_cjs_rec_0);"),
        "{closed}"
    );
    assert!(
        closed.contains("if (__perry_cjs_lk_0 === false) {"),
        "{closed}"
    );
}

#[test]
fn computed_requires_refresh_evicted_path_modules() {
    let source = wrapped("module.exports = name => require('./' + name + '.cjs');");
    assert!(
        source.contains("return __perry_cjs_require_computed(specifier, "),
        "computed requests go through the support module: {source}"
    );
    // ... whose path-module arm re-evaluates an evicted module's factory.
    let support = super::super::support::source(&["/x/entry.cjs".to_string()], false);
    assert!(
        support.contains("return refresh(path, pathModule, reloader);"),
        "computed requests need factory re-evaluation too: {support}"
    );
}

/// A hoisted class reads only module-scope bindings, so the wrapper evaluates
/// before it only a target whose alias it surfaces at module scope. A
/// conditional alias stays in the body: its target loads when the edge runs
/// (conditional_require_init: `class Unrelated {}` made `dep.cjs` run before
/// the entry's first line).
#[test]
fn a_hoisted_class_does_not_evaluate_a_conditional_require_first() {
    let pre_init = "\n__perry_cjs_init(__perry_cjs_rec_0);\n";
    let conditional = wrapped(
        "class Unrelated {}\nif (process.argv.includes('--load')) {\n    const Value = require('./dep.cjs');\n    console.log(Value.value);\n}\n",
    );
    assert!(conditional.contains("class Unrelated {}"), "{conditional}");
    assert!(!conditional.contains(pre_init), "{conditional}");
    // A module-scope alias, which a class could read, is still evaluated first.
    let module_scope =
        wrapped("class Unrelated {}\nconst Dep = require('./dep.cjs');\nconsole.log(Dep.value);\n");
    assert!(module_scope.contains(pre_init), "{module_scope}");
}

#[test]
fn the_reload_slot_is_factory_local_and_undefined_seeded() {
    use perry_hir::{Expr, Stmt};
    let path = "/fixture/reload.cjs";
    let source = wrap_commonjs("module.exports = () => 1;", std::path::Path::new(path));
    let ast = perry_parser::parse_typescript(&source, path).unwrap();
    let module = perry_hir::lower_module(&ast, path, path).unwrap();
    let mut factories = 0;
    perry_hir::tdz_check::for_each_module_expr(&module, &mut |expr| {
        let Expr::Closure { body, captures, .. } = expr else {
            return;
        };
        let Some(slot) = body.iter().find_map(|stmt| match stmt {
            Stmt::Let {
                id,
                name,
                init: Some(Expr::Undefined),
                ..
            } if name == "__perry_cjs_reload_require" => Some(*id),
            _ => None,
        }) else {
            return;
        };
        factories += 1;
        assert!(
            !captures.contains(&slot),
            "the factory owns the loader slot"
        );
        let reloader_captures = body
            .iter()
            .find_map(|stmt| match stmt {
                Stmt::Let {
                    name,
                    init: Some(Expr::Closure { captures, .. }),
                    ..
                } if name == "__perry_cjs_reloader" => Some(captures),
                _ => None,
            })
            .expect("the hoisted reloader");
        assert!(reloader_captures.contains(&slot));
        assert!(
            body.iter()
                .any(|stmt| matches!(stmt, Stmt::PreallocateBoxes(ids) if ids.contains(&slot))),
            "the lazy loader needs an undefined-seeded cell at factory entry"
        );
        assert!(
            !body
                .iter()
                .any(|stmt| matches!(stmt, Stmt::PreallocateTdzBoxes(ids) if ids.contains(&slot))),
            "a private lazy loader must not impose a TDZ gate on its factory"
        );
    });
    assert_eq!(factories, 1, "the slot belongs to exactly one factory");
    assert!(!module.init.iter().any(
        |stmt| matches!(stmt, Stmt::Let { name, .. } if name == "__perry_cjs_reload_require")
    ));
}

#[test]
fn deferred_require_cells_are_undefined_seeded_inside_the_factory() {
    use perry_hir::{Expr, Stmt};
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("dep.mjs"), "export default 42;").unwrap();
    let path = dir.path().join("entry.cjs");
    let source = wrap_commonjs(
        "module.exports = function () { return require('./dep.mjs'); };",
        &path,
    );
    let path = path.to_str().unwrap();
    let ast = perry_parser::parse_typescript(&source, path).unwrap();
    let module = perry_hir::lower_module(&ast, path, path).unwrap();
    let mut factories = 0;
    perry_hir::tdz_check::for_each_module_expr(&module, &mut |expr| {
        let Expr::Closure { body, captures, .. } = expr else {
            return;
        };
        let slots = body
            .iter()
            .filter_map(|stmt| match stmt {
                Stmt::Let { id, name, .. }
                    if name == "__perry_cjs_c" || name.ends_with("__rec") =>
                {
                    Some(*id)
                }
                _ => None,
            })
            .collect::<std::collections::BTreeSet<_>>();
        if slots.is_empty() {
            return;
        }
        factories += 1;
        assert_eq!(slots.len(), 2, "the cache alias and deferred-record memo");
        let require_captures = body
            .iter()
            .find_map(|stmt| match stmt {
                Stmt::Let {
                    name,
                    init: Some(Expr::Closure { captures, .. }),
                    ..
                } if name == "require" => Some(captures),
                _ => None,
            })
            .expect("the hoisted require function");
        for slot in slots {
            assert!(!captures.contains(&slot), "the factory owns the cell");
            assert!(
                require_captures.contains(&slot),
                "require captures the cell"
            );
            assert!(
                body.iter()
                    .any(|stmt| matches!(stmt, Stmt::PreallocateBoxes(ids) if ids.contains(&slot))),
                "a generated require cell needs an undefined seed at factory entry"
            );
            assert!(!body
                .iter()
                .any(|stmt| matches!(stmt, Stmt::PreallocateTdzBoxes(ids) if ids.contains(&slot))));
        }
    });
    assert_eq!(factories, 1);
}
