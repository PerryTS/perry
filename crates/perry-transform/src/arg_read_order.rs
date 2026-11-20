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
//! `a` in time, and `bump() + a` or `a.x + a` do not. An operator whose
//! operands are all primitive converts nothing that could run code: with `a`
//! bound to a local that only ever holds primitives
//! (`WriteCensus::primitive_arg`), `a >= 0 && a < 128 && (a & 0x88) === 0`
//! reads `a` in time three times.
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
use crate::local_occurrences::{count_list_occurrences, primitive_valued};
use perry_hir::types::LocalId;
use perry_hir::walker::walk_expr_children;
use perry_hir::{CompareOp, Expr, Stmt, UnaryOp};
use std::collections::HashSet;

pub(crate) fn enabled() -> bool {
    !matches!(
        std::env::var("PERRY_INLINE_PURE_ARGS").as_deref(),
        Ok("0" | "off" | "false")
    )
}

/// Evaluating `expr` can run code, or stores to `local`.
pub(crate) fn may_run_code(expr: &Expr, local: LocalId) -> bool {
    let mut scan = Scan::new(LocalId::MAX, local, HashSet::new());
    scan.expr(expr);
    scan.ran
}

/// Every read of `param` in `body` happens before anything in `body` that
/// could run code or store to `local`, the caller local passed for it.
/// `primitive`: the parameters whose arguments are primitive values.
pub(crate) fn reads_precede_effects(
    body: &[Stmt],
    param: LocalId,
    local: LocalId,
    primitive: HashSet<LocalId>,
) -> bool {
    let mut scan = Scan::new(param, local, primitive);
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
    /// Locals of the body that hold a primitive: parameters bound to one,
    /// and its `const`s initialized with one.
    primitive: HashSet<LocalId>,
    /// Something that could run code, or a store to `local`, has completed.
    ran: bool,
    /// A read of `param` came after that.
    late: bool,
    /// Plain reads of `param`, each counted once.
    reads: usize,
    /// Inside the second walk of a loop: its reads are already counted.
    replay: bool,
}

impl Scan {
    fn new(param: LocalId, local: LocalId, primitive: HashSet<LocalId>) -> Self {
        Scan {
            param,
            local,
            primitive,
            ran: false,
            late: false,
            reads: 0,
            replay: false,
        }
    }

    /// A loop: what runs on one trip comes before every read of the next.
    fn twice(&mut self, mut trip: impl FnMut(&mut Self)) {
        trip(self);
        let outer = std::mem::replace(&mut self.replay, true);
        trip(self);
        self.replay = outer;
    }

    fn stmts(&mut self, stmts: &[Stmt]) {
        for s in stmts {
            self.stmt(s);
        }
    }

    fn stmt(&mut self, stmt: &Stmt) {
        match stmt {
            Stmt::Let {
                id, init, mutable, ..
            } => {
                if let Some(e) = init {
                    self.expr(e);
                    if !*mutable && self.is_primitive(e) {
                        self.primitive.insert(*id);
                    }
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
            Stmt::While { condition, body } => self.twice(|scan| {
                scan.expr(condition);
                scan.stmts(body);
            }),
            Stmt::DoWhile { body, condition } => self.twice(|scan| {
                scan.stmts(body);
                scan.expr(condition);
            }),
            Stmt::For {
                init,
                condition,
                update,
                body,
            } => {
                if let Some(i) = init {
                    self.stmt(i);
                }
                self.twice(|scan| {
                    if let Some(e) = condition {
                        scan.expr(e);
                    }
                    scan.stmts(body);
                    if let Some(e) = update {
                        scan.expr(e);
                    }
                });
            }
            Stmt::Labeled { body, .. } => self.stmt(body),
            Stmt::Break | Stmt::Continue | Stmt::LabeledBreak(_) | Stmt::LabeledContinue(_) => {}
            // `switch` (every test before any body), `try` and the box
            // statements: not ordered here.
            _ => self.late = true,
        }
    }

    /// The value of `expr` is a primitive: a literal, a primitive local, or
    /// an operator that always yields one.
    fn is_primitive(&self, expr: &Expr) -> bool {
        match expr {
            Expr::LocalGet(id) => self.primitive.contains(id),
            Expr::Logical { left, right, .. } => {
                self.is_primitive(left) && self.is_primitive(right)
            }
            Expr::Conditional {
                then_expr,
                else_expr,
                ..
            } => self.is_primitive(then_expr) && self.is_primitive(else_expr),
            other => primitive_valued(other),
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
                    self.reads += usize::from(!self.replay);
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
            // Left, right, then the operator's conversions (`valueOf`) of
            // an operand that may be an object.
            Expr::Binary { left, right, .. } | Expr::Compare { left, right, .. } => {
                self.expr(left);
                self.expr(right);
                self.ran |= !(self.is_primitive(left) && self.is_primitive(right));
            }
            Expr::Unary { operand, .. } => {
                self.expr(operand);
                self.ran |= !self.is_primitive(operand);
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
                    let mut probe = Scan::new(self.param, self.local, self.primitive.clone());
                    walk_expr_children(other, &mut |child| probe.expr(child));
                    self.ran = probe.ran;
                }
                walk_expr_children(other, &mut |child| self.expr(child));
                self.ran = true;
            }
        }
    }
}
