//! Which closures are an iterating builtin's callback (#6952, #7034 §8).
//!
//! Loop-nesting depth alone is a **wrong** hotness proxy for object-heavy
//! TypeScript. #7034 measured `batch.ts`: of 247 PIC/guard blocks, only 39
//! sit inside an explicit loop region and **208 are in closure bodies** —
//! `map`/`sort`/`reduce` callbacks that have no loop of their own and are
//! invoked once per element. Ranking those at depth 0 buries exactly the
//! sites that matter.
//!
//! So the report carries the two facts in separate columns. This module
//! supplies the second one: a per-module scan that records which closure
//! `FuncId`s are passed directly to an iterating builtin, and which builtin.
//! It is a *static, syntactic* attribution — an indirection
//! (`const f = x => …; arr.map(f)`) is not resolved, so absence of the mark
//! is not evidence the body is cold. The report says so.

use perry_hir::{Expr, Module, Stmt};

/// Scan a module and register every closure that is syntactically the
/// callback of an iterating builtin. Runs once per module, only under
/// `--opt-report`.
pub(crate) fn scan_module(hir: &Module) {
    if !super::enabled() {
        return;
    }
    scan_stmts(&hir.init);
    for f in &hir.functions {
        scan_stmts(&f.body);
    }
    for class in &hir.classes {
        for m in &class.methods {
            scan_stmts(&m.body);
        }
        if let Some(ctor) = &class.constructor {
            scan_stmts(&ctor.body);
        }
    }
}

fn scan_stmts(stmts: &[Stmt]) {
    for s in stmts {
        match s {
            Stmt::Let { init, .. } => {
                if let Some(e) = init {
                    scan_expr(e);
                }
            }
            Stmt::Expr(e) | Stmt::Throw(e) => scan_expr(e),
            Stmt::Return(opt) => {
                if let Some(e) = opt {
                    scan_expr(e);
                }
            }
            Stmt::If {
                condition,
                then_branch,
                else_branch,
            } => {
                scan_expr(condition);
                scan_stmts(then_branch);
                if let Some(eb) = else_branch {
                    scan_stmts(eb);
                }
            }
            Stmt::While { condition, body } | Stmt::DoWhile { body, condition } => {
                scan_expr(condition);
                scan_stmts(body);
            }
            Stmt::For {
                init,
                condition,
                update,
                body,
            } => {
                if let Some(init) = init {
                    scan_stmts(std::slice::from_ref(init.as_ref()));
                }
                if let Some(c) = condition {
                    scan_expr(c);
                }
                if let Some(u) = update {
                    scan_expr(u);
                }
                scan_stmts(body);
            }
            Stmt::Try {
                body,
                catch,
                finally,
            } => {
                scan_stmts(body);
                if let Some(c) = catch {
                    scan_stmts(&c.body);
                }
                if let Some(f) = finally {
                    scan_stmts(f);
                }
            }
            Stmt::Switch {
                discriminant,
                cases,
            } => {
                scan_expr(discriminant);
                for c in cases {
                    if let Some(t) = &c.test {
                        scan_expr(t);
                    }
                    scan_stmts(&c.body);
                }
            }
            Stmt::Labeled { body, .. } => scan_stmts(std::slice::from_ref(body.as_ref())),
            _ => {}
        }
    }
}

/// The iterating builtins whose callback runs once per element. `sort`'s
/// comparator runs O(n log n) times, which is even more per-element than the
/// rest; it is labelled the same way.
fn scan_expr(e: &Expr) {
    scan_children(e);
    let (callback, builtin) = match e {
        Expr::ArrayForEach { callback, .. } => (callback, "Array.prototype.forEach"),
        Expr::ArrayMap { callback, .. } => (callback, "Array.prototype.map"),
        Expr::ArrayFilter { callback, .. } => (callback, "Array.prototype.filter"),
        Expr::ArrayFind { callback, .. } => (callback, "Array.prototype.find"),
        Expr::ArrayFindIndex { callback, .. } => (callback, "Array.prototype.findIndex"),
        Expr::ArrayFindLast { callback, .. } => (callback, "Array.prototype.findLast"),
        Expr::ArrayFindLastIndex { callback, .. } => (callback, "Array.prototype.findLastIndex"),
        Expr::ArraySome { callback, .. } => (callback, "Array.prototype.some"),
        Expr::ArrayEvery { callback, .. } => (callback, "Array.prototype.every"),
        Expr::ArrayFlatMap { callback, .. } => (callback, "Array.prototype.flatMap"),
        Expr::ArrayReduce { callback, .. } => (callback, "Array.prototype.reduce"),
        Expr::ArrayReduceRight { callback, .. } => (callback, "Array.prototype.reduceRight"),
        Expr::ArraySort { comparator, .. } => (comparator, "Array.prototype.sort"),
        _ => return,
    };
    if let Expr::Closure { func_id, .. } = callback.as_ref() {
        super::note_per_element_callback(*func_id, builtin);
    }
}

/// Recurse into every sub-expression, including closure BODIES (which are
/// `Vec<Stmt>` and therefore invisible to `walk_expr_children`). A `map`
/// callback nested inside another callback must be marked too.
fn scan_children(e: &Expr) {
    if let Expr::Closure { body, .. } = e {
        scan_stmts(body);
    }
    perry_hir::walker::walk_expr_children(e, &mut |c| scan_expr(c));
}
