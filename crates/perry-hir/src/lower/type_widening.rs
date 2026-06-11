//! Post-lowering static-type widening (#3576/#3575 family).
//!
//! A `var x = 2` infers `Type::Number` from its initializer, but a later
//! assignment — often from inside a closure, e.g. a sloppy accessor body
//! doing `x = this` (test262 10.4.3-1-56gs…61gs) — can store a value that
//! is certainly NOT a number. Codegen trusts the declared type and lowers
//! `x === o` as a float compare; NaN-boxed pointers are NaNs, so identity
//! comparisons go permanently false. Widen the declared type to `Any` for
//! every local that is assigned a certainly-non-numeric value anywhere in
//! the module, INCLUDING inside nested closure bodies (the expr walker
//! does not descend into `Expr::Closure` bodies, so this pass recurses
//! manually).
//!
//! Deliberately conservative: only RHS shapes that are statically known to
//! be non-numeric trigger widening, so number-typed fast paths for actual
//! numeric code are untouched (zero-regression requirement).

use crate::ir::*;
use perry_types::{LocalId, Type};
use std::collections::HashSet;

/// RHS shapes that can never evaluate to a JS number.
fn rhs_certainly_object_like(expr: &Expr) -> bool {
    matches!(
        expr,
        Expr::This
            | Expr::Object(_)
            | Expr::ObjectSpread { .. }
            | Expr::ObjectAssign { .. }
            | Expr::Array(_)
            | Expr::ArraySpread(_)
            | Expr::Closure { .. }
            | Expr::New { .. }
            | Expr::Null
            | Expr::Undefined
    )
}

/// RHS shapes that are primitives but not numbers (still wrong for a
/// `Type::Number`-declared slot).
fn rhs_certainly_string_or_bool(expr: &Expr) -> bool {
    matches!(expr, Expr::String(_) | Expr::Bool(_))
}

#[derive(Default)]
struct WidenSets {
    /// Assigned an object-like value → widen any primitive declared type.
    object_like: HashSet<LocalId>,
    /// Assigned a string/bool literal → widen a numeric declared type.
    string_or_bool: HashSet<LocalId>,
}

fn visit_expr(expr: &Expr, out: &mut WidenSets) {
    if let Expr::LocalSet(id, rhs) = expr {
        if rhs_certainly_object_like(rhs) {
            out.object_like.insert(*id);
        } else if rhs_certainly_string_or_bool(rhs) {
            out.string_or_bool.insert(*id);
        }
    }
    if let Expr::Closure { params, body, .. } = expr {
        for p in params {
            if let Some(d) = &p.default {
                visit_expr(d, out);
            }
        }
        for s in body {
            visit_stmt(s, out);
        }
        return;
    }
    crate::walker::walk_expr_children(expr, &mut |child| visit_expr(child, out));
}

fn visit_stmt(stmt: &Stmt, out: &mut WidenSets) {
    match stmt {
        Stmt::Let { init: Some(e), .. }
        | Stmt::Expr(e)
        | Stmt::Return(Some(e))
        | Stmt::Throw(e) => visit_expr(e, out),
        Stmt::If {
            condition,
            then_branch,
            else_branch,
        } => {
            visit_expr(condition, out);
            for s in then_branch {
                visit_stmt(s, out);
            }
            if let Some(b) = else_branch {
                for s in b {
                    visit_stmt(s, out);
                }
            }
        }
        Stmt::While { condition, body } | Stmt::DoWhile { body, condition } => {
            visit_expr(condition, out);
            for s in body {
                visit_stmt(s, out);
            }
        }
        Stmt::For {
            init,
            condition,
            update,
            body,
        } => {
            if let Some(s) = init {
                visit_stmt(s, out);
            }
            if let Some(e) = condition {
                visit_expr(e, out);
            }
            if let Some(e) = update {
                visit_expr(e, out);
            }
            for s in body {
                visit_stmt(s, out);
            }
        }
        Stmt::Labeled { body, .. } => visit_stmt(body, out),
        Stmt::Try {
            body,
            catch,
            finally,
        } => {
            for s in body {
                visit_stmt(s, out);
            }
            if let Some(c) = catch {
                for s in &c.body {
                    visit_stmt(s, out);
                }
            }
            if let Some(f) = finally {
                for s in f {
                    visit_stmt(s, out);
                }
            }
        }
        Stmt::Switch {
            discriminant,
            cases,
        } => {
            visit_expr(discriminant, out);
            for c in cases {
                if let Some(t) = &c.test {
                    visit_expr(t, out);
                }
                for s in &c.body {
                    visit_stmt(s, out);
                }
            }
        }
        _ => {}
    }
}

fn widen_lets_stmt(stmt: &mut Stmt, sets: &WidenSets) {
    match stmt {
        Stmt::Let { id, ty, .. } => {
            let widen = match ty {
                Type::Number | Type::Int32 => {
                    sets.object_like.contains(id) || sets.string_or_bool.contains(id)
                }
                Type::String | Type::Boolean => sets.object_like.contains(id),
                _ => false,
            };
            if widen {
                *ty = Type::Any;
            }
        }
        Stmt::If {
            then_branch,
            else_branch,
            ..
        } => {
            for s in then_branch {
                widen_lets_stmt(s, sets);
            }
            if let Some(b) = else_branch {
                for s in b {
                    widen_lets_stmt(s, sets);
                }
            }
        }
        Stmt::While { body, .. } | Stmt::DoWhile { body, .. } => {
            for s in body {
                widen_lets_stmt(s, sets);
            }
        }
        Stmt::For { init, body, .. } => {
            if let Some(s) = init {
                widen_lets_stmt(s, sets);
            }
            for s in body {
                widen_lets_stmt(s, sets);
            }
        }
        Stmt::Labeled { body, .. } => widen_lets_stmt(body, sets),
        Stmt::Try {
            body,
            catch,
            finally,
        } => {
            for s in body {
                widen_lets_stmt(s, sets);
            }
            if let Some(c) = catch {
                for s in &mut c.body {
                    widen_lets_stmt(s, sets);
                }
            }
            if let Some(f) = finally {
                for s in f {
                    widen_lets_stmt(s, sets);
                }
            }
        }
        Stmt::Switch { cases, .. } => {
            for c in cases {
                for s in &mut c.body {
                    widen_lets_stmt(s, sets);
                }
            }
        }
        _ => {}
    }
}

/// Run the pass over one body: collect non-numeric assignments (recursing
/// into closure bodies), then widen the matching `Stmt::Let` declared types.
/// Collection and rewriting both stay within the given body slice — HIR
/// LocalIds are unique module-wide, so cross-body collection is handled by
/// the caller passing every body through `collect` first.
pub(crate) struct TypeWidening {
    sets: WidenSets,
}

impl TypeWidening {
    pub(crate) fn new() -> Self {
        Self {
            sets: WidenSets::default(),
        }
    }

    pub(crate) fn collect(&mut self, stmts: &[Stmt]) {
        for s in stmts {
            visit_stmt(s, &mut self.sets);
        }
    }

    pub(crate) fn apply(&self, stmts: &mut [Stmt]) {
        if self.sets.object_like.is_empty() && self.sets.string_or_bool.is_empty() {
            return;
        }
        for s in stmts {
            widen_lets_stmt(s, &self.sets);
        }
    }
}
