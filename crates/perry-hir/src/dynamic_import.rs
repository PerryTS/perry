//! Issue #100: helpers for compile-time-resolved dynamic `import()`.
//!
//! Two responsibilities:
//!
//! 1. [`resolve_import_path`] — const-folds the path argument of a
//!    dynamic `import()` to a finite set of module sources. The
//!    supported subset is documented inline; anything outside it
//!    returns [`Resolution::Unresolved`] with a human-readable reason
//!    so the driver can raise a structured compile error.
//!
//! 2. [`detect_top_level_await`] — sets `Module.has_top_level_await`
//!    by scanning `module.init` for any `Expr::Await` outside a
//!    function/closure body. Drives the deferred-import dispatch to
//!    chain the init promise.
//!
//! Neither helper performs filesystem I/O — path resolution to a
//! `resolved_path` is the driver's job (it owns the module resolver).
//! Here we only fold the JS-level path *string*.

use crate::ir::{Expr, Export, Function, Module, Stmt};
use crate::walker::{walk_expr_children, walk_expr_children_mut};
use std::collections::HashSet;

/// Hard cap on the number of paths a single `import()` site can resolve
/// to. Over-cap produces a compile error per D2 (issue #100).
pub const DYNAMIC_IMPORT_PATH_CAP: usize = 64;

/// Walk every expression in `module` (init statements, top-level functions,
/// class constructors/methods/getters/setters, field initializers, etc.)
/// and invoke `f` with each `&mut Expr::DynamicImport` node found.
///
/// Used by the driver to run [`resolve_import_path`] over every dynamic
/// import site in a freshly lowered module so it can register the
/// resolved targets in the import graph and stamp `paths` on each node.
pub fn for_each_dynamic_import_mut<F: FnMut(&mut Expr)>(module: &mut Module, f: &mut F) {
    for stmt in &mut module.init {
        visit_stmt_for_dyn_imports(stmt, f);
    }
    for func in &mut module.functions {
        visit_function_for_dyn_imports(func, f);
    }
    for cls in &mut module.classes {
        if let Some(ctor) = &mut cls.constructor {
            visit_function_for_dyn_imports(ctor, f);
        }
        for m in &mut cls.methods {
            visit_function_for_dyn_imports(m, f);
        }
        for (_, g) in &mut cls.getters {
            visit_function_for_dyn_imports(g, f);
        }
        for (_, s) in &mut cls.setters {
            visit_function_for_dyn_imports(s, f);
        }
        for m in &mut cls.static_methods {
            visit_function_for_dyn_imports(m, f);
        }
        for field in &mut cls.fields {
            if let Some(init) = &mut field.init {
                visit_expr_for_dyn_imports(init, f);
            }
        }
        for field in &mut cls.static_fields {
            if let Some(init) = &mut field.init {
                visit_expr_for_dyn_imports(init, f);
            }
        }
    }
    for global in &mut module.globals {
        if let Some(init) = &mut global.init {
            visit_expr_for_dyn_imports(init, f);
        }
    }
}

fn visit_function_for_dyn_imports<F: FnMut(&mut Expr)>(func: &mut Function, f: &mut F) {
    for stmt in &mut func.body {
        visit_stmt_for_dyn_imports(stmt, f);
    }
    for param in &mut func.params {
        if let Some(default) = &mut param.default {
            visit_expr_for_dyn_imports(default, f);
        }
    }
}

fn visit_stmt_for_dyn_imports<F: FnMut(&mut Expr)>(stmt: &mut Stmt, f: &mut F) {
    match stmt {
        Stmt::Let { init, .. } => {
            if let Some(e) = init {
                visit_expr_for_dyn_imports(e, f);
            }
        }
        Stmt::Expr(e) => visit_expr_for_dyn_imports(e, f),
        Stmt::Return(opt) => {
            if let Some(e) = opt {
                visit_expr_for_dyn_imports(e, f);
            }
        }
        Stmt::If {
            condition,
            then_branch,
            else_branch,
        } => {
            visit_expr_for_dyn_imports(condition, f);
            for s in then_branch {
                visit_stmt_for_dyn_imports(s, f);
            }
            if let Some(eb) = else_branch {
                for s in eb {
                    visit_stmt_for_dyn_imports(s, f);
                }
            }
        }
        Stmt::While { condition, body } => {
            visit_expr_for_dyn_imports(condition, f);
            for s in body {
                visit_stmt_for_dyn_imports(s, f);
            }
        }
        Stmt::DoWhile { body, condition } => {
            for s in body {
                visit_stmt_for_dyn_imports(s, f);
            }
            visit_expr_for_dyn_imports(condition, f);
        }
        Stmt::For {
            init,
            condition,
            update,
            body,
        } => {
            if let Some(i) = init {
                visit_stmt_for_dyn_imports(i, f);
            }
            if let Some(c) = condition {
                visit_expr_for_dyn_imports(c, f);
            }
            if let Some(u) = update {
                visit_expr_for_dyn_imports(u, f);
            }
            for s in body {
                visit_stmt_for_dyn_imports(s, f);
            }
        }
        Stmt::Labeled { body, .. } => visit_stmt_for_dyn_imports(body, f),
        Stmt::Throw(e) => visit_expr_for_dyn_imports(e, f),
        Stmt::Try {
            body,
            catch,
            finally,
        } => {
            for s in body {
                visit_stmt_for_dyn_imports(s, f);
            }
            if let Some(c) = catch {
                for s in &mut c.body {
                    visit_stmt_for_dyn_imports(s, f);
                }
            }
            if let Some(fb) = finally {
                for s in fb {
                    visit_stmt_for_dyn_imports(s, f);
                }
            }
        }
        Stmt::Switch { discriminant, cases } => {
            visit_expr_for_dyn_imports(discriminant, f);
            for c in cases {
                if let Some(t) = &mut c.test {
                    visit_expr_for_dyn_imports(t, f);
                }
                for s in &mut c.body {
                    visit_stmt_for_dyn_imports(s, f);
                }
            }
        }
        Stmt::Break
        | Stmt::Continue
        | Stmt::LabeledBreak(_)
        | Stmt::LabeledContinue(_)
        | Stmt::PreallocateBoxes(_) => {}
    }
}

fn visit_expr_for_dyn_imports<F: FnMut(&mut Expr)>(expr: &mut Expr, f: &mut F) {
    if matches!(expr, Expr::DynamicImport { .. }) {
        f(expr);
        // After f mutates the node, still descend into the (possibly
        // unchanged) `arg` so nested dynamic imports are visited.
        if let Expr::DynamicImport { arg, .. } = expr {
            visit_expr_for_dyn_imports(arg, f);
        }
        return;
    }
    // Closure bodies — descend manually (the walker intentionally
    // doesn't).
    if let Expr::Closure { body, .. } = expr {
        for s in body {
            visit_stmt_for_dyn_imports(s, f);
        }
    }
    walk_expr_children_mut(expr, &mut |child| visit_expr_for_dyn_imports(child, f));
}

/// The result of const-folding a dynamic `import()` path argument.
#[derive(Debug, Clone)]
pub enum Resolution {
    /// The argument resolves to this non-empty, bounded set of module
    /// sources. The driver registers each as an import edge.
    Set(Vec<String>),
    /// The argument cannot be statically resolved. The driver should
    /// raise a compile error citing this reason.
    Unresolved(String),
}

impl Resolution {
    fn merge(self, other: Resolution) -> Resolution {
        match (self, other) {
            (Resolution::Set(mut a), Resolution::Set(b)) => {
                for p in b {
                    if !a.contains(&p) {
                        a.push(p);
                    }
                }
                Resolution::Set(a)
            }
            (Resolution::Unresolved(r), _) | (_, Resolution::Unresolved(r)) => {
                Resolution::Unresolved(r)
            }
        }
    }
}

/// Issue #100: one entry in the flat-export list of a module that may
/// be the target of a dynamic `import()`. Returned by [`flatten_exports`]
/// after resolving `ReExport` / `ExportAll` / `NamespaceReExport` through
/// the module graph.
///
/// The codegen consumes this list to populate the module's
/// `__perry_ns_<prefix>` global at the end of `__perry_init_<prefix>`.
/// Each entry maps one exported name (as the consumer sees it) to the
/// module + local binding that actually holds the value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FlatExport {
    /// The key as seen by the consumer of `await import("...")`.
    pub name: String,
    /// Module that owns the binding holding the value. For local exports
    /// this is the same module passed to `flatten_exports`; for re-
    /// exports it's the upstream module the value transitively came
    /// from.
    pub source_module: String,
    /// The local name in `source_module` that holds the value.
    pub source_local: String,
    /// For `NamespaceReExport` — when `Some(nested_source)`, this entry
    /// represents `name → namespace_of(nested_source)`. Codegen emits a
    /// nested `js_create_namespace` call sourced from that module's own
    /// `__perry_ns_<prefix>`. Otherwise `None` (the typical case).
    pub nested_namespace_of: Option<String>,
}

/// Issue #100: resolve a module's exports — flattening `ExportAll`,
/// `ReExport`, and `NamespaceReExport` through the import graph — into
/// a flat list suitable for namespace materialization.
///
/// `modules` is a lookup of every module by `Module::name` (the same
/// string used in `Import::source` / `Export::*::source` resolution
/// keys). The caller is responsible for resolving module specifiers
/// (e.g. `"./foo.ts"` vs `Module::name`) up-front and keying `modules`
/// consistently — both `Export::ReExport::source` strings and
/// `Module::name` must use the same form.
///
/// Cycle-safe: a `visited` set tracks modules we've already descended
/// into so an `export * from` cycle terminates without infinite
/// recursion. The first encounter wins (depth-first).
///
/// Returns entries in declaration order with later entries overriding
/// earlier ones on duplicate names (matches JS semantics for
/// `export * from`).
pub fn flatten_exports<'a, F>(target_name: &str, lookup: &F) -> Vec<FlatExport>
where
    F: Fn(&str) -> Option<&'a Module>,
{
    let mut out: Vec<FlatExport> = Vec::new();
    let mut visited: HashSet<String> = HashSet::new();
    flatten_into(target_name, lookup, &mut out, &mut visited);
    // Preserve last-writer-wins on duplicate names while keeping insertion order.
    let mut seen: HashSet<String> = HashSet::new();
    let mut dedup: Vec<FlatExport> = Vec::new();
    for entry in out.into_iter().rev() {
        if seen.insert(entry.name.clone()) {
            dedup.push(entry);
        }
    }
    dedup.reverse();
    dedup
}

fn flatten_into<'a, F>(
    module_name: &str,
    lookup: &F,
    out: &mut Vec<FlatExport>,
    visited: &mut HashSet<String>,
) where
    F: Fn(&str) -> Option<&'a Module>,
{
    if !visited.insert(module_name.to_string()) {
        return;
    }
    let module = match lookup(module_name) {
        Some(m) => m,
        None => return,
    };
    for export in &module.exports {
        match export {
            Export::Named { local, exported } => {
                out.push(FlatExport {
                    name: exported.clone(),
                    source_module: module_name.to_string(),
                    source_local: local.clone(),
                    nested_namespace_of: None,
                });
            }
            Export::ReExport {
                source,
                imported,
                exported,
            } => {
                // The value lives in `source`; if `source` re-exports it
                // again, we want to follow that chain so codegen reaches
                // the ULTIMATE owner. But for the MVP we surface one hop
                // — the cross-module access pattern works regardless of
                // how many hops away the value's defining module is, as
                // long as we name the directly-importing source.
                out.push(FlatExport {
                    name: exported.clone(),
                    source_module: source.clone(),
                    source_local: imported.clone(),
                    nested_namespace_of: None,
                });
            }
            Export::ExportAll { source } => {
                // Recursively flatten the source's exports into ours.
                // Cycle-safe via `visited`; depth-first so a closer
                // re-exporter wins on name collision (matches the
                // dedup pass above).
                flatten_into(source, lookup, out, visited);
            }
            Export::NamespaceReExport { source, name } => {
                out.push(FlatExport {
                    name: name.clone(),
                    source_module: source.clone(),
                    source_local: String::new(),
                    nested_namespace_of: Some(source.clone()),
                });
            }
        }
    }
}

/// Const-fold a dynamic `import()` path argument.
///
/// Supported forms (D1, issue #100):
///   - String literal:                    `import('./foo.ts')`
///   - Ternary of two resolvable args:    `import(cond ? a : b)`
///   - Parenthesized / `as` wrapper:      not represented in HIR (already
///     elided during lowering)
///
/// Local SSA-tracking and template-literal expansion are intentionally
/// out of scope for this pass — they need a flow-sensitive analyzer
/// that doesn't exist yet. Those shapes fall through to `Unresolved`
/// with an actionable hint.
pub fn resolve_import_path(arg: &Expr) -> Resolution {
    match arg {
        Expr::String(s) => Resolution::Set(vec![s.clone()]),
        Expr::Conditional {
            then_expr,
            else_expr,
            ..
        } => {
            let a = resolve_import_path(then_expr);
            let b = resolve_import_path(else_expr);
            a.merge(b)
        }
        _ => Resolution::Unresolved(
            "path argument is not statically resolvable (only string literals and \
             ternary expressions of literals are supported); consider enumerating \
             with a ternary or registry object"
                .to_string(),
        ),
    }
}

/// Scan `module.init` for an `await` expression outside any function/
/// closure body and set `module.has_top_level_await` accordingly.
///
/// Idempotent — safe to call multiple times. Closure bodies are NOT
/// descended into because awaits inside them belong to the closure's
/// own async scope, not the module's top level.
pub fn detect_top_level_await(module: &mut Module) {
    let mut found = false;
    for stmt in &module.init {
        if stmt_has_top_level_await(stmt) {
            found = true;
            break;
        }
    }
    module.has_top_level_await = found;
}

fn stmt_has_top_level_await(stmt: &Stmt) -> bool {
    match stmt {
        Stmt::Let { init, .. } => init.as_ref().is_some_and(expr_has_top_level_await),
        Stmt::Expr(e) => expr_has_top_level_await(e),
        Stmt::Return(opt) => opt.as_ref().is_some_and(expr_has_top_level_await),
        Stmt::If {
            condition,
            then_branch,
            else_branch,
        } => {
            expr_has_top_level_await(condition)
                || then_branch.iter().any(stmt_has_top_level_await)
                || else_branch
                    .as_ref()
                    .is_some_and(|b| b.iter().any(stmt_has_top_level_await))
        }
        Stmt::While { condition, body } => {
            expr_has_top_level_await(condition) || body.iter().any(stmt_has_top_level_await)
        }
        Stmt::DoWhile { body, condition } => {
            body.iter().any(stmt_has_top_level_await) || expr_has_top_level_await(condition)
        }
        Stmt::For {
            init,
            condition,
            update,
            body,
        } => {
            init.as_deref().is_some_and(stmt_has_top_level_await)
                || condition.as_ref().is_some_and(expr_has_top_level_await)
                || update.as_ref().is_some_and(expr_has_top_level_await)
                || body.iter().any(stmt_has_top_level_await)
        }
        Stmt::Labeled { body, .. } => stmt_has_top_level_await(body),
        Stmt::Throw(e) => expr_has_top_level_await(e),
        Stmt::Try {
            body,
            catch,
            finally,
        } => {
            body.iter().any(stmt_has_top_level_await)
                || catch
                    .as_ref()
                    .is_some_and(|c| c.body.iter().any(stmt_has_top_level_await))
                || finally
                    .as_ref()
                    .is_some_and(|f| f.iter().any(stmt_has_top_level_await))
        }
        Stmt::Switch { discriminant, cases } => {
            expr_has_top_level_await(discriminant)
                || cases.iter().any(|c| {
                    c.test.as_ref().is_some_and(expr_has_top_level_await)
                        || c.body.iter().any(stmt_has_top_level_await)
                })
        }
        Stmt::Break
        | Stmt::Continue
        | Stmt::LabeledBreak(_)
        | Stmt::LabeledContinue(_)
        | Stmt::PreallocateBoxes(_) => false,
    }
}

fn expr_has_top_level_await(expr: &Expr) -> bool {
    // The walker's `Closure` arm intentionally does NOT descend into the
    // closure body, which is exactly the semantics we need: an `await`
    // inside a nested closure/function belongs to that function's scope,
    // not the module's top level.
    if matches!(expr, Expr::Await(_)) {
        return true;
    }
    let mut found = false;
    walk_expr_children(expr, &mut |child| {
        if !found && expr_has_top_level_await(child) {
            found = true;
        }
    });
    found
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::Module;
    use perry_types::Type;

    #[test]
    fn resolve_string_literal() {
        let r = resolve_import_path(&Expr::String("./foo.ts".into()));
        match r {
            Resolution::Set(v) => assert_eq!(v, vec!["./foo.ts"]),
            _ => panic!("expected Set"),
        }
    }

    #[test]
    fn resolve_ternary_of_literals() {
        let r = resolve_import_path(&Expr::Conditional {
            condition: Box::new(Expr::Bool(true)),
            then_expr: Box::new(Expr::String("./a.ts".into())),
            else_expr: Box::new(Expr::String("./b.ts".into())),
        });
        match r {
            Resolution::Set(v) => {
                assert_eq!(v.len(), 2);
                assert!(v.contains(&"./a.ts".to_string()));
                assert!(v.contains(&"./b.ts".to_string()));
            }
            _ => panic!("expected Set"),
        }
    }

    #[test]
    fn resolve_ternary_dedupes() {
        let r = resolve_import_path(&Expr::Conditional {
            condition: Box::new(Expr::Bool(true)),
            then_expr: Box::new(Expr::String("./a.ts".into())),
            else_expr: Box::new(Expr::String("./a.ts".into())),
        });
        match r {
            Resolution::Set(v) => assert_eq!(v, vec!["./a.ts"]),
            _ => panic!("expected Set"),
        }
    }

    #[test]
    fn resolve_unresolvable_local() {
        let r = resolve_import_path(&Expr::LocalGet(0));
        assert!(matches!(r, Resolution::Unresolved(_)));
    }

    #[test]
    fn tla_detects_module_init_await() {
        let mut m = Module::new("t");
        m.init.push(Stmt::Expr(Expr::Await(Box::new(Expr::Undefined))));
        detect_top_level_await(&mut m);
        assert!(m.has_top_level_await);
    }

    #[test]
    fn flatten_local_named_exports() {
        let mut m = Module::new("foo");
        m.exports.push(Export::Named {
            local: "x".into(),
            exported: "x".into(),
        });
        m.exports.push(Export::Named {
            local: "_g".into(),
            exported: "greet".into(),
        });
        let map = std::collections::HashMap::from([("foo".to_string(), m.clone())]);
        let lookup = |s: &str| map.get(s);
        let flat = flatten_exports("foo", &lookup);
        assert_eq!(flat.len(), 2);
        assert_eq!(flat[0].name, "x");
        assert_eq!(flat[0].source_module, "foo");
        assert_eq!(flat[0].source_local, "x");
        assert_eq!(flat[1].name, "greet");
        assert_eq!(flat[1].source_local, "_g");
    }

    #[test]
    fn flatten_reexport_one_hop() {
        let mut barrel = Module::new("barrel");
        barrel.exports.push(Export::ReExport {
            source: "inner".into(),
            imported: "v".into(),
            exported: "v".into(),
        });
        let map = std::collections::HashMap::from([("barrel".to_string(), barrel.clone())]);
        let lookup = |s: &str| map.get(s);
        let flat = flatten_exports("barrel", &lookup);
        assert_eq!(flat.len(), 1);
        assert_eq!(flat[0].name, "v");
        assert_eq!(flat[0].source_module, "inner");
        assert_eq!(flat[0].source_local, "v");
    }

    #[test]
    fn flatten_export_all_recursive() {
        let mut inner = Module::new("inner");
        inner.exports.push(Export::Named {
            local: "v".into(),
            exported: "v".into(),
        });
        let mut barrel = Module::new("barrel");
        barrel.exports.push(Export::ExportAll {
            source: "inner".into(),
        });
        let map = std::collections::HashMap::from([
            ("inner".to_string(), inner.clone()),
            ("barrel".to_string(), barrel.clone()),
        ]);
        let lookup = |s: &str| map.get(s);
        let flat = flatten_exports("barrel", &lookup);
        assert_eq!(flat.len(), 1);
        assert_eq!(flat[0].name, "v");
        assert_eq!(flat[0].source_module, "inner");
        assert_eq!(flat[0].source_local, "v");
    }

    #[test]
    fn flatten_export_all_cycle_safe() {
        // a -> b -> a — must terminate.
        let mut a = Module::new("a");
        a.exports.push(Export::ExportAll { source: "b".into() });
        a.exports.push(Export::Named {
            local: "fromA".into(),
            exported: "fromA".into(),
        });
        let mut b = Module::new("b");
        b.exports.push(Export::ExportAll { source: "a".into() });
        b.exports.push(Export::Named {
            local: "fromB".into(),
            exported: "fromB".into(),
        });
        let map = std::collections::HashMap::from([
            ("a".to_string(), a.clone()),
            ("b".to_string(), b.clone()),
        ]);
        let lookup = |s: &str| map.get(s);
        let flat = flatten_exports("a", &lookup);
        // Both names appear; recursion terminates at the back-edge.
        let names: Vec<String> = flat.iter().map(|e| e.name.clone()).collect();
        assert!(names.contains(&"fromA".to_string()));
        assert!(names.contains(&"fromB".to_string()));
    }

    #[test]
    fn flatten_namespace_re_export() {
        let mut m = Module::new("m");
        m.exports.push(Export::NamespaceReExport {
            source: "sub".into(),
            name: "Sub".into(),
        });
        let map = std::collections::HashMap::from([("m".to_string(), m.clone())]);
        let lookup = |s: &str| map.get(s);
        let flat = flatten_exports("m", &lookup);
        assert_eq!(flat.len(), 1);
        assert_eq!(flat[0].name, "Sub");
        assert_eq!(flat[0].nested_namespace_of, Some("sub".to_string()));
    }

    #[test]
    fn tla_skips_await_inside_closure() {
        let mut m = Module::new("t");
        // Build a closure body containing an Await — the module-level
        // detector must NOT descend into the closure.
        let closure = Expr::Closure {
            func_id: 0,
            params: vec![],
            return_type: Type::Any,
            body: vec![Stmt::Expr(Expr::Await(Box::new(Expr::Undefined)))],
            captures: vec![],
            mutable_captures: vec![],
            captures_this: false,
            enclosing_class: None,
            is_async: true,
        };
        m.init.push(Stmt::Expr(closure));
        detect_top_level_await(&mut m);
        assert!(!m.has_top_level_await);
    }
}
