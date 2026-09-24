//! Imports synthesized for cross-module method candidates (#11244).
//!
//! A method harvested from an earlier-compiled module may read names that
//! module imports (`Expr::ExternFuncRef`). When such a body is inlined into a
//! destination module, the destination needs its own `Import` for each of
//! those names so codegen can resolve the reference.
//!
//! Every synthesized import is an ordinary static import, and the driver turns
//! each one into a module-init dependency. So the import must exist only when
//! a body that needs it was actually inlined here. Queuing it for every
//! admitted candidate, before any call site had chosen one, gave modules init
//! edges to code they never run: a module reachable only through a lazy
//! `import()` initialized at startup, and inside an import cycle a module was
//! re-entered before its exports were populated (mongodb's
//! `import { MongoClient } from "mongodb"`).
//!
//! Callers queue candidate requirements with [`PendingExternImports::queue`]
//! and, once every inlining phase has run, call
//! [`PendingExternImports::apply`], which keeps only the names the module's
//! bodies now reference.

use perry_hir::walker::walk_expr_children;
use perry_hir::{Expr, Function, Module, Stmt};
use std::collections::{BTreeMap, BTreeSet, HashSet};

/// `(name, resolved_path)` pairs requested by admitted method candidates,
/// grouped per source path.
#[derive(Default)]
pub(crate) struct PendingExternImports {
    by_path: BTreeMap<String, BTreeSet<String>>,
}

impl PendingExternImports {
    pub(crate) fn queue(&mut self, name: &str, path: &str) {
        self.by_path
            .entry(path.to_string())
            .or_default()
            .insert(name.to_string());
    }

    /// Add an import for every queued name that some body in `module` now
    /// references. A queued name nothing references belongs to a candidate
    /// no call site inlined, and adding it would only create an init edge.
    pub(crate) fn apply(self, module: &mut Module) {
        if self.by_path.is_empty() {
            return;
        }
        let referenced = extern_func_ref_names(module);
        for (path, names) in self.by_path {
            let names: Vec<String> = names
                .into_iter()
                .filter(|name| referenced.contains(name))
                .collect();
            if names.is_empty() {
                continue;
            }
            add_named_imports(module, path, names);
        }
    }
}

/// Append `names` (bound under their own names) to the destination's import
/// of `path`, creating that import if the module has none.
fn add_named_imports(module: &mut Module, path: String, names: Vec<String>) {
    let existing_idx = module.imports.iter().position(|imp| {
        imp.resolved_path
            .as_deref()
            .is_some_and(|p| p == path.as_str())
    });
    match existing_idx {
        Some(idx) => {
            for name in names {
                let already = module.imports[idx].specifiers.iter().any(|s| {
                    matches!(s, perry_hir::ImportSpecifier::Named { local, .. } if local == &name)
                });
                if !already {
                    module.imports[idx]
                        .specifiers
                        .push(perry_hir::ImportSpecifier::Named {
                            imported: name.clone(),
                            local: name,
                        });
                }
            }
        }
        None => {
            // Only the resolved path is known on the candidate side, so this
            // is a minimal reconstruction. `is_native = false` because the
            // cross-module-safe filter already excluded native-only patterns;
            // `NativeCompiled` is the only kind codegen consults for
            // `import_function_prefixes`.
            //
            // `is_deferred_require`: the import exists only to bind the name
            // for codegen, so it must not be an init edge either. Every
            // consumer of the flag is init ordering (`init_order.rs`, the
            // per-module `__init` deps in `run_pipeline.rs`); codegen binds
            // the name the same way. No edge is lost: the inlined body runs
            // where the source method would have, on an exact `new C()`
            // receiver, so C's module has already initialized -- and it
            // imports this path itself. Source order is Node's order, and a
            // synthesized edge could only add to it.
            module.imports.push(perry_hir::Import {
                source: path.clone(),
                specifiers: names
                    .into_iter()
                    .map(|name| perry_hir::ImportSpecifier::Named {
                        imported: name.clone(),
                        local: name,
                    })
                    .collect(),
                is_native: false,
                module_kind: perry_hir::ModuleKind::NativeCompiled,
                resolved_path: Some(path),
                type_only: false,
                runtime_erased: false,
                is_dynamic: false,
                is_dynamic_target: false,
                is_deferred_require: true,
                is_adopted_require: false,
            });
        }
    }
}

/// Every `ExternFuncRef` name in the module's executable bodies, including
/// closure bodies. This is a superset of the places the inliner writes
/// (init, functions, class methods), so a name an inlined body introduced is
/// always found.
pub(crate) fn extern_func_ref_names(module: &Module) -> HashSet<String> {
    let mut out = HashSet::new();
    visit_stmts(&module.init, &mut out);
    for function in &module.functions {
        visit_function(function, &mut out);
    }
    for class in &module.classes {
        if let Some(expr) = &class.extends_expr {
            visit_expr(expr, &mut out);
        }
        if let Some(ctor) = &class.constructor {
            visit_function(ctor, &mut out);
        }
        for function in class.methods.iter().chain(&class.static_methods) {
            visit_function(function, &mut out);
        }
        for (_, function) in class.getters.iter().chain(&class.setters) {
            visit_function(function, &mut out);
        }
        for field in class.fields.iter().chain(&class.static_fields) {
            if let Some(expr) = &field.key_expr {
                visit_expr(expr, &mut out);
            }
            if let Some(expr) = &field.init {
                visit_expr(expr, &mut out);
            }
        }
    }
    out
}

fn visit_function(function: &Function, out: &mut HashSet<String>) {
    for param in &function.params {
        if let Some(default) = &param.default {
            visit_expr(default, out);
        }
    }
    visit_stmts(&function.body, out);
}

fn visit_expr(expr: &Expr, out: &mut HashSet<String>) {
    match expr {
        Expr::ExternFuncRef { name, .. } => {
            out.insert(name.clone());
        }
        // `walk_expr_children` visits a closure's param defaults but not its
        // statement body.
        Expr::Closure { body, .. } => visit_stmts(body, out),
        _ => {}
    }
    walk_expr_children(expr, &mut |child| visit_expr(child, out));
}

fn visit_stmts(stmts: &[Stmt], out: &mut HashSet<String>) {
    for stmt in stmts {
        visit_stmt(stmt, out);
    }
}

fn visit_stmt(stmt: &Stmt, out: &mut HashSet<String>) {
    match stmt {
        Stmt::Let { init, .. } => {
            if let Some(expr) = init {
                visit_expr(expr, out);
            }
        }
        Stmt::Expr(expr) | Stmt::Throw(expr) | Stmt::Return(Some(expr)) => visit_expr(expr, out),
        Stmt::Return(None) | Stmt::Break | Stmt::Continue => {}
        Stmt::LabeledBreak(_) | Stmt::LabeledContinue(_) => {}
        Stmt::If {
            condition,
            then_branch,
            else_branch,
        } => {
            visit_expr(condition, out);
            visit_stmts(then_branch, out);
            if let Some(else_branch) = else_branch {
                visit_stmts(else_branch, out);
            }
        }
        Stmt::While { condition, body } | Stmt::DoWhile { body, condition } => {
            visit_expr(condition, out);
            visit_stmts(body, out);
        }
        Stmt::For {
            init,
            condition,
            update,
            body,
        } => {
            if let Some(init) = init {
                visit_stmt(init, out);
            }
            if let Some(condition) = condition {
                visit_expr(condition, out);
            }
            if let Some(update) = update {
                visit_expr(update, out);
            }
            visit_stmts(body, out);
        }
        Stmt::Switch {
            discriminant,
            cases,
        } => {
            visit_expr(discriminant, out);
            for case in cases {
                if let Some(test) = &case.test {
                    visit_expr(test, out);
                }
                visit_stmts(&case.body, out);
            }
        }
        Stmt::Try {
            body,
            catch,
            finally,
        } => {
            visit_stmts(body, out);
            if let Some(catch) = catch {
                visit_stmts(&catch.body, out);
            }
            if let Some(finally) = finally {
                visit_stmts(finally, out);
            }
        }
        Stmt::Labeled { body, .. } => visit_stmt(body, out),
        Stmt::PreallocateBoxes(_) | Stmt::PreallocateTdzBoxes(_) | Stmt::ReleaseBoxes(_) => {}
    }
}
