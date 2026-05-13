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

use crate::ir::{Expr, Module, Stmt};
use crate::walker::walk_expr_children;

/// Hard cap on the number of paths a single `import()` site can resolve
/// to. Over-cap produces a compile error per D2 (issue #100).
pub const DYNAMIC_IMPORT_PATH_CAP: usize = 64;

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
