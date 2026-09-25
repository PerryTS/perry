//! #11250: keep class-capture refreshes from reading a `for (let …)` head
//! binding outside the iteration that owns it.
//!
//! A refresh (`RegisterClassCaptures` / `RefreshClassExprCaptures`) re-reads
//! every capture of its class. A class that closed over a per-iteration head
//! binding `i` may only re-read `i` inside the loop body: the head's condition
//! and update already run against the NEXT iteration's binding, and after the
//! loop the single HIR slot for `i` holds the post-increment value. The refresh
//! passes in `expr_function.rs` place refreshes by assignment site and before
//! returns, whatever capture triggered them, so a write to another capture
//! (`i++, x++`), a later `return`, or the loop head itself would otherwise
//! overwrite the last class's `i` with the value that stopped the loop.
//!
//! The refresh itself must still run: it also carries the other captures,
//! e.g. a `const` declared after the loop that the class read while still in
//! its TDZ. So a per-object refresh keeps every in-scope capture and re-reads
//! an expired head from the class object's own capture slot instead. The
//! name-keyed `RegisterClassCaptures` snapshot has no per-evaluation slot to
//! re-read and is dropped; the per-object refresh is authoritative over it.

use std::collections::HashSet;

use crate::ir::{Expr, Stmt};
use crate::types::LocalId;

/// Rewrite every refresh in `stmts` that captures a `for`-head lexical binding
/// outside that loop's body (see the module docs). Closures are not descended:
/// each owns its own refresh region.
pub(crate) fn prune_out_of_scope_capture_refreshes(stmts: &mut Vec<Stmt>) {
    let mut heads = HashSet::new();
    collect_for_heads(stmts, &mut heads);
    if heads.is_empty() {
        return;
    }
    let mut pruner = Pruner {
        heads,
        enclosing: Vec::new(),
    };
    pruner.stmts(stmts);
}

/// The lexical binding a `for` head declares. A `var` head is hoisted out of
/// `For::init` during lowering, so a `Let` here is always a per-iteration
/// `let`/`const` binding.
fn for_head(init: &Option<Box<Stmt>>) -> Option<LocalId> {
    match init.as_deref() {
        Some(Stmt::Let { id, .. }) => Some(*id),
        _ => None,
    }
}

fn collect_for_heads(stmts: &[Stmt], heads: &mut HashSet<LocalId>) {
    for stmt in stmts {
        match stmt {
            Stmt::If {
                then_branch,
                else_branch,
                ..
            } => {
                collect_for_heads(then_branch, heads);
                if let Some(branch) = else_branch {
                    collect_for_heads(branch, heads);
                }
            }
            Stmt::While { body, .. } | Stmt::DoWhile { body, .. } => {
                collect_for_heads(body, heads);
            }
            Stmt::For { init, body, .. } => {
                heads.extend(for_head(init));
                collect_for_heads(body, heads);
            }
            Stmt::Labeled { body, .. } => {
                collect_for_heads(std::slice::from_ref(body.as_ref()), heads)
            }
            Stmt::Try {
                body,
                catch,
                finally,
            } => {
                collect_for_heads(body, heads);
                if let Some(catch) = catch {
                    collect_for_heads(&catch.body, heads);
                }
                if let Some(finally) = finally {
                    collect_for_heads(finally, heads);
                }
            }
            Stmt::Switch { cases, .. } => {
                for case in cases {
                    collect_for_heads(&case.body, heads);
                }
            }
            _ => {}
        }
    }
}

struct Pruner {
    heads: HashSet<LocalId>,
    /// Heads whose loop body encloses the current position.
    enclosing: Vec<LocalId>,
}

impl Pruner {
    fn expired(&self, capture: &Expr) -> bool {
        matches!(capture, Expr::LocalGet(id)
            if self.heads.contains(id) && !self.enclosing.contains(id))
    }

    /// Whether `expr` is a refresh to drop outright. A per-object refresh
    /// reading an expired head is rewritten in place and kept.
    fn rewrite_refresh(&self, expr: &mut Expr) -> bool {
        match expr {
            Expr::RegisterClassCaptures { captures, .. } => {
                captures.iter().any(|capture| self.expired(capture))
            }
            Expr::RefreshClassExprCaptures {
                class_value,
                captures,
            } => {
                for (index, capture) in captures.iter_mut().enumerate() {
                    if self.expired(capture) {
                        *capture = current_capture_slot(class_value, index);
                    }
                }
                false
            }
            _ => false,
        }
    }

    fn stmts(&mut self, stmts: &mut Vec<Stmt>) {
        stmts.retain_mut(|stmt| match stmt {
            Stmt::Expr(expr) => !self.rewrite_refresh(expr),
            _ => true,
        });
        for stmt in stmts.iter_mut() {
            self.stmt(stmt);
        }
    }

    fn stmt(&mut self, stmt: &mut Stmt) {
        match stmt {
            Stmt::Let {
                init: Some(expr), ..
            }
            | Stmt::Expr(expr)
            | Stmt::Return(Some(expr))
            | Stmt::Throw(expr) => self.expr(expr),
            Stmt::If {
                condition,
                then_branch,
                else_branch,
            } => {
                self.expr(condition);
                self.stmts(then_branch);
                if let Some(branch) = else_branch {
                    self.stmts(branch);
                }
            }
            Stmt::While { condition, body } | Stmt::DoWhile { body, condition } => {
                self.expr(condition);
                self.stmts(body);
            }
            Stmt::For {
                init,
                condition,
                update,
                body,
            } => {
                if let Some(init) = init {
                    self.stmt(init);
                }
                // The head's condition and update belong to the next
                // iteration, so the head binding is in scope only in the body.
                if let Some(condition) = condition {
                    self.expr(condition);
                }
                if let Some(update) = update {
                    self.expr(update);
                }
                let head = for_head(init);
                self.enclosing.extend(head);
                self.stmts(body);
                if head.is_some() {
                    self.enclosing.pop();
                }
            }
            Stmt::Labeled { body, .. } => self.stmt(body),
            Stmt::Try {
                body,
                catch,
                finally,
            } => {
                self.stmts(body);
                if let Some(catch) = catch {
                    self.stmts(&mut catch.body);
                }
                if let Some(finally) = finally {
                    self.stmts(finally);
                }
            }
            Stmt::Switch {
                discriminant,
                cases,
            } => {
                self.expr(discriminant);
                for case in cases {
                    if let Some(test) = &mut case.test {
                        self.expr(test);
                    }
                    self.stmts(&mut case.body);
                }
            }
            _ => {}
        }
    }

    /// Inline refreshes live inside `(x = v, refresh…, x)` sequences.
    fn expr(&mut self, expr: &mut Expr) {
        if matches!(expr, Expr::Closure { .. }) {
            return;
        }
        if let Expr::Sequence(items) = expr {
            items.retain_mut(|item| !self.rewrite_refresh(item));
        }
        crate::walker::walk_expr_children_mut(expr, &mut |child| self.expr(child));
    }
}

/// `class_value.__perry_ctor_caps[index]` — the value the class object already
/// holds for that capture — or `undefined` when the class was never evaluated
/// (the refresh is then a no-op, but its captures are still evaluated).
fn current_capture_slot(class_value: &Expr, index: usize) -> Expr {
    Expr::Conditional {
        condition: Box::new(class_value.clone()),
        then_expr: Box::new(Expr::IndexGet {
            object: Box::new(Expr::PropertyGet {
                byte_offset: 0,
                object: Box::new(class_value.clone()),
                property: "__perry_ctor_caps".to_string(),
            }),
            index: Box::new(Expr::Integer(index as i64)),
        }),
        else_expr: Box::new(Expr::Undefined),
    }
}
