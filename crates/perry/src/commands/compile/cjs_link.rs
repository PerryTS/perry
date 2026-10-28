//! The program's CommonJS closed-world fact (`cjs_closed`, design
//! CJS_LINK 2026-10-26 §2.5) and the one pass that applies it.
//!
//! A program is closed when no module's original source can reach the
//! loader: a loader capability, a write or escape of `require.cache` /
//! `require.extensions`, a computed `require`, `require` or `module` leaving
//! as a value, dynamic code (`cjs_wrap::run_once_syntax`). Then a literal
//! `require` is exact (the target's record binding, no cache read), nothing
//! publishes into the runtime's path registry, and every CommonJS factory
//! runs once. The wrapper, codegen's class families and wpfacts all read
//! this one bit.
//!
//! Each module is wrapped while the walk still discovers the graph, so the
//! bit is not known then. The wrapper writes its escape forms as
//! `if (__perry_cjs_open()) { … } else { … }`; once every module is
//! collected, [`resolve_open_sites`] keeps the arm the bit selects. Codegen
//! and wpfacts never see the condition.

use perry_hir::{Expr, Module, Stmt};
use std::path::PathBuf;

/// The closed-world bit over the collected modules
/// (`perry_hir::program_is_cjs_closed`). `PERRY_LAYOUT_REPORT=2` names the
/// modules that refuse it.
pub(super) fn program_is_closed<'a>(
    modules: impl IntoIterator<Item = (&'a PathBuf, &'a Module)> + Clone,
) -> bool {
    let closed = perry_hir::program_is_cjs_closed(modules.clone().into_iter().map(|(_, m)| m));
    if !closed && std::env::var("PERRY_LAYOUT_REPORT").as_deref() == Ok("2") {
        if perry_hir::has_deferred_dynamic_code_sites() {
            eprintln!("perry: CommonJS not closed: the program has dynamic code sites");
        }
        for (_, module) in modules {
            match module
                .class_family_syntax
                .as_ref()
                .map(|s| s.module_capability_uses)
            {
                Some(0) => {}
                Some(n) => eprintln!(
                    "perry: CommonJS not closed: {}: {n} capability uses",
                    module.name
                ),
                None => eprintln!(
                    "perry: CommonJS not closed: {} has no original-source census",
                    module.name
                ),
            }
        }
    }
    closed
}

fn is_open_call(e: &Expr) -> bool {
    matches!(e, Expr::NativeMethodCall { module, object: None, method, .. }
        if module == "__perry_runtime" && method == "cjsOpen")
}

/// Keep the arm of every `if (__perry_cjs_open())` the bit selects, in every
/// statement list of the module (closure bodies included). Any other use of
/// the call is a malformed wrapper: an error, never a guess.
pub(super) fn resolve_open_sites(module: &mut Module, closed: bool) -> anyhow::Result<()> {
    let open = !closed;
    stmts(&mut module.init, open);
    for f in &mut module.functions {
        stmts(&mut f.body, open);
    }
    for class in &mut module.classes {
        if let Some(ctor) = &mut class.constructor {
            stmts(&mut ctor.body, open);
        }
        for m in class
            .methods
            .iter_mut()
            .chain(class.static_methods.iter_mut())
            .chain(class.getters.iter_mut().map(|(_, f)| f))
            .chain(class.setters.iter_mut().map(|(_, f)| f))
            .chain(class.computed_members.iter_mut().map(|m| &mut m.function))
        {
            stmts(&mut m.body, open);
        }
    }
    if closed {
        drop_unused_loader_preloads(module);
    }
    let mut left = false;
    perry_hir::tdz_check::for_each_module_expr(module, &mut |e| left |= is_open_call(e));
    if left {
        anyhow::bail!(
            "internal compiler error: {}: `__perry_cjs_open()` outside an `if` condition",
            module.name
        );
    }
    Ok(())
}

/// A named builtin import preloads its export cell at module start, which
/// installs that builtin's whole runtime dispatcher (for `node:module`:
/// `createRequire`, the dynamic loader, `vm`, …). In a closed program the
/// wrapper's own `createRequire` alias is no longer called once the escape
/// forms are gone: its preload goes too, so none of that is linked.
fn drop_unused_loader_preloads(module: &mut Module) {
    let builtin = |s: &str| s.strip_prefix("node:").unwrap_or(s) == "module";
    let owned: Vec<(String, String)> = module
        .imports
        .iter()
        .filter(|i| builtin(&i.source))
        .flat_map(|i| i.specifiers.iter())
        .filter_map(|s| match s {
            perry_hir::ImportSpecifier::Named { imported, local }
                if local.starts_with("__perry_cjs") =>
            {
                Some((imported.clone(), local.clone()))
            }
            _ => None,
        })
        .collect();
    let mut used: Vec<String> = Vec::new();
    perry_hir::tdz_check::for_each_module_expr(module, &mut |e| {
        if let Expr::ExternFuncRef { name, .. } = e {
            used.push(name.clone());
        }
    });
    let unused: Vec<&String> = owned
        .iter()
        .filter(|(_, local)| !used.contains(local))
        .map(|(imported, _)| imported)
        .collect();
    module.imports.retain_mut(|import| {
        if builtin(&import.source) {
            import.specifiers.retain(|specifier| {
                !matches!(specifier, perry_hir::ImportSpecifier::Named { imported, local }
                    if local.starts_with("__perry_cjs") && unused.contains(&imported))
            });
            return !import.specifiers.is_empty();
        }
        true
    });
    module.init.retain(|s| {
        !matches!(s, Stmt::Expr(Expr::Call { callee, args, .. })
            if matches!(callee.as_ref(), Expr::ExternFuncRef { name, .. } if name == "js_native_module_named_esm_export_value")
                && matches!(args.as_slice(), [Expr::String(src), Expr::String(imported)]
                    if builtin(src) && unused.contains(&imported)))
    });
}

fn stmts(list: &mut Vec<Stmt>, open: bool) {
    let mut i = 0;
    while i < list.len() {
        if let Stmt::If { condition, .. } = &list[i] {
            if is_open_call(condition) {
                let Stmt::If {
                    then_branch,
                    else_branch,
                    ..
                } = list.remove(i)
                else {
                    unreachable!()
                };
                let live = if open {
                    then_branch
                } else {
                    else_branch.unwrap_or_default()
                };
                // The arm's statements take the `if`'s place; its own sites
                // are visited as part of this list.
                list.splice(i..i, live);
                continue;
            }
        }
        stmt(&mut list[i], open);
        i += 1;
    }
}

fn stmt(s: &mut Stmt, open: bool) {
    match s {
        Stmt::If {
            condition,
            then_branch,
            else_branch,
        } => {
            expr(condition, open);
            stmts(then_branch, open);
            if let Some(b) = else_branch {
                stmts(b, open);
            }
        }
        Stmt::While { condition, body } | Stmt::DoWhile { body, condition } => {
            expr(condition, open);
            stmts(body, open);
        }
        Stmt::For {
            init,
            condition,
            update,
            body,
        } => {
            if let Some(i) = init {
                stmt(i, open);
            }
            if let Some(c) = condition {
                expr(c, open);
            }
            if let Some(u) = update {
                expr(u, open);
            }
            stmts(body, open);
        }
        Stmt::Try {
            body,
            catch,
            finally,
        } => {
            stmts(body, open);
            if let Some(c) = catch {
                stmts(&mut c.body, open);
            }
            if let Some(f) = finally {
                stmts(f, open);
            }
        }
        Stmt::Switch {
            discriminant,
            cases,
        } => {
            expr(discriminant, open);
            for c in cases {
                if let Some(t) = &mut c.test {
                    expr(t, open);
                }
                stmts(&mut c.body, open);
            }
        }
        Stmt::Labeled { body, .. } => stmt(body, open),
        Stmt::Let { init, .. } => {
            if let Some(e) = init {
                expr(e, open);
            }
        }
        Stmt::Expr(e) | Stmt::Throw(e) => expr(e, open),
        Stmt::Return(e) => {
            if let Some(e) = e {
                expr(e, open);
            }
        }
        Stmt::Break
        | Stmt::Continue
        | Stmt::LabeledBreak(_)
        | Stmt::LabeledContinue(_)
        | Stmt::PreallocateBoxes(_)
        | Stmt::PreallocateTdzBoxes(_)
        | Stmt::ReleaseBoxes(_) => {}
    }
}

/// Closure bodies inside an expression.
fn expr(e: &mut Expr, open: bool) {
    if let Expr::Closure { body, .. } = e {
        stmts(body, open);
    }
    perry_hir::walker::walk_expr_children_mut(e, &mut |c| expr(c, open));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn open_if(then: Vec<Stmt>, otherwise: Vec<Stmt>) -> Stmt {
        Stmt::If {
            condition: Expr::NativeMethodCall {
                module: "__perry_runtime".to_string(),
                class_name: None,
                object: None,
                method: "cjsOpen".to_string(),
                args: Vec::new(),
            },
            then_branch: then,
            else_branch: Some(otherwise),
        }
    }

    fn module_with(init: Vec<Stmt>) -> Module {
        let mut m = Module::new("m.js".to_string());
        m.init = init;
        m
    }

    #[test]
    fn the_bit_keeps_one_arm_and_no_condition() {
        let body = || {
            vec![
                Stmt::Expr(Expr::Number(0.0)),
                open_if(
                    vec![Stmt::Expr(Expr::Number(1.0))],
                    vec![Stmt::Expr(Expr::Number(2.0))],
                ),
                Stmt::Expr(Expr::Number(3.0)),
            ]
        };
        for (closed, kept) in [(true, 2.0), (false, 1.0)] {
            let mut m = module_with(body());
            resolve_open_sites(&mut m, closed).unwrap();
            let got: Vec<f64> = m
                .init
                .iter()
                .map(|s| match s {
                    Stmt::Expr(Expr::Number(n)) => *n,
                    other => panic!("{other:?}"),
                })
                .collect();
            assert_eq!(got, vec![0.0, kept, 3.0], "closed={closed}");
        }
    }

    #[test]
    fn closed_wrappers_keep_literal_builtin_bindings_without_loader_imports() {
        for source in [
            "console.log(require('fs').readFileSync);",
            "const fs = require('node:fs'); console.log(fs.readFileSync);",
            "const { readFileSync } = require('fs'); console.log(readFileSync);",
        ] {
            let (wrapped, _) = super::super::cjs_wrap::wrap_commonjs_with_addon_paths(
                source,
                std::path::Path::new("/p/main.cjs"),
                None,
                false,
                None,
                None,
                &mut Vec::new(),
                None,
            );
            let ast = perry_parser::parse_typescript(&wrapped, "main.cjs").unwrap();
            for closed in [false, true] {
                let mut module = perry_hir::lower_module(&ast, "main", "/p/main.cjs").unwrap();
                resolve_open_sites(&mut module, closed).unwrap();
                let has_import = |name| {
                    module
                        .imports
                        .iter()
                        .any(|i| i.source.strip_prefix("node:").unwrap_or(&i.source) == name)
                };
                assert!(
                    has_import("fs"),
                    "literal require lost its static binding: {source}"
                );
                assert_eq!(has_import("module"), !closed, "closed={closed}: {source}");
                assert!(module.is_commonjs_wrap());
                let mut has_loader = false;
                perry_hir::tdz_check::for_each_module_expr(&module, &mut |e| {
                    has_loader |= matches!(e, Expr::NativeMethodCall { module, method, .. }
                        if module == "module" && method == "createRequire");
                });
                assert_eq!(has_loader, !closed, "closed={closed}: {source}");
            }
        }
    }

    #[test]
    fn a_condition_used_as_a_value_is_an_error() {
        let mut m = module_with(vec![Stmt::Expr(Expr::NativeMethodCall {
            module: "__perry_runtime".to_string(),
            class_name: None,
            object: None,
            method: "cjsOpen".to_string(),
            args: Vec::new(),
        })]);
        assert!(resolve_open_sites(&mut m, true).is_err());
    }
}
