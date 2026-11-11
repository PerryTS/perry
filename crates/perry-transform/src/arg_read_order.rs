//! When may a read of a caller local be moved from the call into the body?
//!
//! `f(m)` reads `m` at the call. Substituting `m` for the parameter reads it
//! where the body uses the parameter. The two reads see the same value when
//! nothing can run between them that could write `m`: no code at all
//! (a call, a `new`, a getter or setter, a `valueOf` / `toString` conversion,
//! an `await` or `yield`), and no direct store to `m`.
//!
//! The scan walks the body in evaluation order and remembers whether
//! anything that could run code, or a store to `m`, has completed. A read of
//! the parameter after that point is late. An operator's own conversion runs
//! after its operands are evaluated, so `(a & 0x88) === 0` and `a * a` read
//! `a` in time, and `bump() + a` or `a.x + a` do not.
//!
//! Only a short list of nodes is known to be inert or to evaluate left to
//! right; every other node is taken to run code both before and after its
//! operands as soon as one of them can. Loops are walked twice, so a read in
//! a loop that runs code anywhere is late. A read inside a closure literal
//! happens whenever the closure is called: late.
//!
//! `PERRY_INLINE_PURE_ARGS=0` (`off`, `false`) turns the rule off; such an
//! argument is then bound to a `let` before the body, or the call is left
//! alone where a `let` has no place (a loop condition).
use crate::local_occurrences::count_list_occurrences;
use perry_hir::types::LocalId;
use perry_hir::walker::walk_expr_children;
use perry_hir::{CompareOp, Expr, Stmt, UnaryOp};

pub(crate) fn enabled() -> bool {
    !matches!(
        std::env::var("PERRY_INLINE_PURE_ARGS").as_deref(),
        Ok("0" | "off" | "false")
    )
}

/// Evaluating `expr` can run code, or stores to `local`.
pub(crate) fn may_run_code(expr: &Expr, local: LocalId) -> bool {
    let mut scan = Scan::new(LocalId::MAX, local);
    scan.expr(expr);
    scan.ran
}

/// Every read of `param` in `body` happens before anything in `body` that
/// could run code or store to `local`, the caller local passed for it.
pub(crate) fn reads_precede_effects(body: &[Stmt], param: LocalId, local: LocalId) -> bool {
    let mut scan = Scan::new(param, local);
    scan.stmts(body);
    // A reference to the parameter that is not a plain read (an intrinsic
    // that names its local, a capture list) is one the scan did not place.
    !scan.late
        && count_list_occurrences(body)
            .get(&param)
            .copied()
            .unwrap_or(0)
            == scan.reads
}

struct Scan {
    param: LocalId,
    local: LocalId,
    /// Something that could run code, or a store to `local`, has completed.
    ran: bool,
    /// A read of `param` came after that.
    late: bool,
    reads: usize,
}

impl Scan {
    fn new(param: LocalId, local: LocalId) -> Self {
        Scan {
            param,
            local,
            ran: false,
            late: false,
            reads: 0,
        }
    }

    fn stmts(&mut self, stmts: &[Stmt]) {
        for s in stmts {
            self.stmt(s);
        }
    }

    fn stmt(&mut self, stmt: &Stmt) {
        match stmt {
            Stmt::Let { init, .. } => {
                if let Some(e) = init {
                    self.expr(e);
                }
            }
            Stmt::Expr(e) | Stmt::Throw(e) => self.expr(e),
            Stmt::Return(e) => {
                if let Some(e) = e {
                    self.expr(e);
                }
            }
            // Walking both arms in a row only adds order, never drops any.
            Stmt::If {
                condition,
                then_branch,
                else_branch,
            } => {
                self.expr(condition);
                self.stmts(then_branch);
                if let Some(b) = else_branch {
                    self.stmts(b);
                }
            }
            Stmt::While { condition, body } => {
                for _ in 0..2 {
                    self.expr(condition);
                    self.stmts(body);
                }
            }
            Stmt::DoWhile { body, condition } => {
                for _ in 0..2 {
                    self.stmts(body);
                    self.expr(condition);
                }
            }
            Stmt::For {
                init,
                condition,
                update,
                body,
            } => {
                if let Some(i) = init {
                    self.stmt(i);
                }
                for _ in 0..2 {
                    if let Some(e) = condition {
                        self.expr(e);
                    }
                    self.stmts(body);
                    if let Some(e) = update {
                        self.expr(e);
                    }
                }
            }
            Stmt::Labeled { body, .. } => self.stmt(body),
            Stmt::Break | Stmt::Continue | Stmt::LabeledBreak(_) | Stmt::LabeledContinue(_) => {}
            // `switch` (every test before any body), `try` and the box
            // statements: not ordered here.
            _ => self.late = true,
        }
    }

    fn expr(&mut self, expr: &Expr) {
        match expr {
            Expr::Integer(_)
            | Expr::Number(_)
            | Expr::Bool(_)
            | Expr::String(_)
            | Expr::WtfString(_)
            | Expr::Null
            | Expr::Undefined => {}
            Expr::LocalGet(id) => {
                if *id == self.param {
                    self.reads += 1;
                    self.late |= self.ran;
                }
            }
            // The store itself runs nothing; only a store to `local` counts.
            Expr::LocalSet(id, value) => {
                self.expr(value);
                self.ran |= *id == self.local;
            }
            // ToBoolean and the nullish test run no code.
            Expr::Logical { left, right, .. } => {
                self.expr(left);
                self.expr(right);
            }
            Expr::Conditional {
                condition,
                then_expr,
                else_expr,
            } => {
                self.expr(condition);
                self.expr(then_expr);
                self.expr(else_expr);
            }
            Expr::Unary {
                op: UnaryOp::Not,
                operand,
            } => self.expr(operand),
            // Strict equality converts nothing.
            Expr::Compare {
                op: CompareOp::Eq | CompareOp::Ne,
                left,
                right,
            } => {
                self.expr(left);
                self.expr(right);
            }
            // Left, right, then the operator's conversions (`valueOf`).
            Expr::Binary { left, right, .. } | Expr::Compare { left, right, .. } => {
                self.expr(left);
                self.expr(right);
                self.ran = true;
            }
            Expr::Unary { operand, .. } => {
                self.expr(operand);
                self.ran = true;
            }
            Expr::Sequence(items) => {
                for item in items {
                    self.expr(item);
                }
            }
            // Creating a closure runs nothing; its body runs at any later
            // time, so a read in it is late.
            Expr::Closure { body, .. } => {
                let ran = std::mem::replace(&mut self.ran, true);
                walk_expr_children(expr, &mut |child| self.expr(child));
                self.stmts(body);
                self.ran = ran;
            }
            // Operand order unknown: if any operand can run code, take it to
            // run before every read in the others. The node itself may run
            // code once its operands are evaluated.
            other => {
                if !self.ran {
                    let mut probe = Scan::new(self.param, self.local);
                    walk_expr_children(other, &mut |child| probe.expr(child));
                    self.ran = probe.ran;
                }
                walk_expr_children(other, &mut |child| self.expr(child));
                self.ran = true;
            }
        }
    }
}
