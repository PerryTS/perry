//! Bound code growth before cloning a loop body. Count HIR statements and
//! expressions, not just top-level statements: one string-building expression
//! or branch can lower to substantial native code. This is a conservative size
//! heuristic, not an estimate of machine-code bytes.

use perry_hir::walker::walk_expr_children;
use perry_hir::{Expr, Stmt};

/// Keep small constant-index kernels eligible while bounding each expansion.
/// Inner loops have already been processed, so their copies are charged again
/// when considering an outer loop. The bound therefore applies to the whole
/// expanded nest, rather than independently to each original loop body.
const MAX_EXPANDED_NODES: usize = 512;

pub(super) fn fits_expansion_budget(body: &[Stmt], trips: usize) -> bool {
    // A single iteration removes loop overhead without duplicating its body.
    if trips == 1 {
        return true;
    }
    let mut budget = Budget(MAX_EXPANDED_NODES / trips);
    budget.stmts(body)
}

struct Budget(usize);

impl Budget {
    fn spend(&mut self) -> bool {
        if self.0 == 0 {
            return false;
        }
        self.0 -= 1;
        true
    }

    fn expr(&mut self, expr: &Expr) -> bool {
        if !self.spend() {
            return false;
        }
        // The safety check has already rejected function literals. Their
        // statement bodies need not be traversed here.
        let mut fits = true;
        walk_expr_children(expr, &mut |child| {
            fits = fits && self.expr(child);
        });
        fits
    }

    fn stmts(&mut self, stmts: &[Stmt]) -> bool {
        stmts.iter().all(|stmt| self.stmt(stmt))
    }

    fn stmt(&mut self, stmt: &Stmt) -> bool {
        if !self.spend() {
            return false;
        }
        // Exhaustive so new statement shapes cannot silently evade the cap.
        match stmt {
            Stmt::Let { init, .. } | Stmt::Return(init) => {
                init.as_ref().is_none_or(|expr| self.expr(expr))
            }
            Stmt::Expr(expr) | Stmt::Throw(expr) => self.expr(expr),
            Stmt::If {
                condition,
                then_branch,
                else_branch,
            } => {
                self.expr(condition)
                    && self.stmts(then_branch)
                    && else_branch.as_ref().is_none_or(|body| self.stmts(body))
            }
            Stmt::While { condition, body } | Stmt::DoWhile { condition, body } => {
                self.expr(condition) && self.stmts(body)
            }
            Stmt::For {
                init,
                condition,
                update,
                body,
            } => {
                init.as_ref().is_none_or(|stmt| self.stmt(stmt))
                    && condition.as_ref().is_none_or(|expr| self.expr(expr))
                    && update.as_ref().is_none_or(|expr| self.expr(expr))
                    && self.stmts(body)
            }
            Stmt::Switch {
                discriminant,
                cases,
            } => {
                self.expr(discriminant)
                    && cases.iter().all(|case| {
                        case.test.as_ref().is_none_or(|expr| self.expr(expr))
                            && self.stmts(&case.body)
                    })
            }
            Stmt::Labeled { body, .. } => self.stmt(body),
            Stmt::Try {
                body,
                catch,
                finally,
            } => {
                self.stmts(body)
                    && catch.as_ref().is_none_or(|catch| self.stmts(&catch.body))
                    && finally.as_ref().is_none_or(|body| self.stmts(body))
            }
            Stmt::PreallocateBoxes(ids)
            | Stmt::PreallocateTdzBoxes(ids)
            | Stmt::ReleaseBoxes(ids) => ids.iter().all(|_| self.spend()),
            Stmt::Break | Stmt::Continue | Stmt::LabeledBreak(_) | Stmt::LabeledContinue(_) => true,
        }
    }
}
