//! Locals bound to an empty array literal that a loop entered after the
//! binding pushes into (`const moves = []; for (…) moves.push(m)`).
//!
//! An empty literal is born with four slots, so such a loop reallocates at
//! its fifth push. A reallocation (allocate, copy, forwarding stub) costs
//! about 800 instructions against about 55 for an in-capacity push. Sixteen
//! pushes into `[]` therefore spent most of their time in one regrowth. The
//! literal of a binding in this set is born with
//! [`crate::target_layout::INLINE_PUSHED_ARRAY_SLOTS`] slots instead: the
//! same empty array, with more room behind it.
//!
//! The loop must be entered after the binding: `for (…) { const m = []; m.push(x) }`
//! pushes once per array and keeps the small birth. A push from a nested
//! closure is not counted. Missing a site only loses the larger birth.
//! `PERRY_PUSHED_ARRAY_RESERVE=0` turns the larger birth off (A/B control).
use perry_hir::{Expr, Module, Stmt};
use std::collections::{HashMap, HashSet};

pub(crate) fn loop_pushed_arrays(hir: &Module) -> HashSet<u32> {
    let mut out = HashSet::new();
    if std::env::var("PERRY_PUSHED_ARRAY_RESERVE").as_deref() == Ok("0") {
        return out;
    }
    crate::scope_env::for_each_body(hir, &mut |_, body| {
        let mut walk = Walk {
            declared: HashMap::new(),
            depth: 0,
            out: &mut out,
        };
        walk.stmts(body);
    });
    out
}

struct Walk<'o> {
    /// Loop depth at which each empty-literal binding was declared.
    declared: HashMap<u32, u32>,
    depth: u32,
    out: &'o mut HashSet<u32>,
}

impl Walk<'_> {
    fn stmts(&mut self, stmts: &[Stmt]) {
        for s in stmts {
            self.stmt(s);
        }
    }

    fn looped(&mut self, f: impl FnOnce(&mut Self)) {
        self.depth += 1;
        f(self);
        self.depth -= 1;
    }

    fn stmt(&mut self, s: &Stmt) {
        match s {
            Stmt::Let { id, init, .. } => {
                if let Some(init) = init {
                    self.expr(init);
                    if matches!(init, Expr::Array(elems, _) if elems.is_empty()) {
                        self.declared.insert(*id, self.depth);
                    }
                }
            }
            Stmt::Expr(e) | Stmt::Throw(e) => self.expr(e),
            Stmt::Return(e) => {
                if let Some(e) = e {
                    self.expr(e);
                }
            }
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
            Stmt::While { condition, body } | Stmt::DoWhile { condition, body } => {
                self.looped(|w| {
                    w.expr(condition);
                    w.stmts(body);
                })
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
                self.looped(|w| {
                    if let Some(c) = condition {
                        w.expr(c);
                    }
                    if let Some(u) = update {
                        w.expr(u);
                    }
                    w.stmts(body);
                })
            }
            Stmt::Labeled { body, .. } => self.stmt(body),
            Stmt::Try {
                body,
                catch,
                finally,
            } => {
                self.stmts(body);
                if let Some(c) = catch {
                    self.stmts(&c.body);
                }
                if let Some(f) = finally {
                    self.stmts(f);
                }
            }
            Stmt::Switch {
                discriminant,
                cases,
            } => {
                self.expr(discriminant);
                for c in cases {
                    if let Some(t) = &c.test {
                        self.expr(t);
                    }
                    self.stmts(&c.body);
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

    fn expr(&mut self, e: &Expr) {
        // A closure's pushes run when it is called, not in this loop; the
        // walker would still visit its parameter defaults.
        if matches!(e, Expr::Closure { .. }) {
            return;
        }
        if let Expr::ArrayPush { array_id, .. } | Expr::ArrayPushSpread { array_id, .. } = e {
            if self.declared.get(array_id).is_some_and(|d| *d < self.depth) {
                self.out.insert(*array_id);
            }
        }
        perry_hir::walker::walk_expr_children(e, &mut |c| self.expr(c));
    }
}
