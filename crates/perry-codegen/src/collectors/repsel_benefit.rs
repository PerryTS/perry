//! Repsel profitability model (#7128): refuse a canonical-i32 promotion whose
//! every hot consumer wants a double.
//!
//! ## The measurement this exists for
//!
//! #7128 measured the #7106/#7034 coverage campaign on a quiet Raspberry Pi 5
//! with `perf stat`, at a 0.02% noise floor. canonical-i32 went from 2/18 to
//! 17/18 real workloads and bought: one −1.05% (`11_prime_sieve`), fifteen
//! exact zeros, and **one +14.87% regression** (`15_mandelbrot`), bisected by
//! binary hash to #7121.
//!
//! The mechanism is visible in the emitted AArch64, not inferred. In
//! `15_mandelbrot` the innermost loop is
//!
//! ```text
//! while (x * x + y * y <= 4.0 && iter < MAX_ITER) { …; iter = iter + 1; }
//! …
//! totalIter = totalIter + iter;
//! ```
//!
//! With `iter` a double the whole loop is **one basic block of 12
//! instructions**: LLVM fuses the two exit tests into `fcmp` + `fccmp` and
//! branches once. With `iter` in a canonical i32 slot the integer test cannot
//! fuse with the FP test, the loop splits into **two blocks of 14 instructions
//! total** (an extra compare-and-branch plus a register copy for the induction
//! phi), and the accumulator join needs a `ucvtf` it did not need before. Two
//! instructions × 8,011,148 innermost iterations ≈ 16.0M, against a measured
//! +15.63M. That is the whole regression.
//!
//! ## Why this is a profitability rule and not a soundness rule
//!
//! Nothing about the promotion is *wrong*. #7122's monotone loop-induction
//! interval proves `iter ∈ [0, 100]`, which is true. The defect is that the
//! promotion was chosen on **provability** rather than on **benefit**: the
//! Let-site gate is a conjunction of "may we?" terms with no "should we?" term
//! anywhere in it, so any widening of the proof automatically widens the
//! emission.
//!
//! The benefit argument is short. For values inside i32 range, `double` is a
//! *lossless and equal-cost* representation of `+`, `-` and comparison — one
//! instruction either way on every target Perry ships. The i32 representation
//! only *buys* something where the consumer cannot take a double at all
//! without a conversion:
//!
//! * an array / typed-array **index** (`a[v]`),
//! * a **bitwise** operand (`&`, `|`, `^`, `<<`, `>>`, `>>>`, `~`),
//! * `Math.imul`.
//!
//! Everywhere else it is at best neutral — and it becomes a *cost* the moment
//! a consumer needs the double back, because that consumer now emits a
//! `sitofp`/`uitofp` the boxed representation never needed.
//!
//! So the rule is: **a local that is written after its declaration, has no
//! i32-consuming read anywhere, and has at least one double-consuming read
//! inside a loop, does not select canonical i32.**
//!
//! Each of the three conjuncts earns its place:
//!
//! * *written after declaration* — a single-assignment local is loop-invariant
//!   at every read, so any conversion is hoisted out of the loop by LICM and
//!   costs O(1), not O(iterations). `const WIDTH = 800` in `15_mandelbrot` is
//!   read as a double twice per inner iteration and still costs nothing,
//!   because the value is a constant. Judging it would be pure census churn.
//! * *no i32-consuming read* — one array index is enough to make the
//!   representation pay for itself; `11_prime_sieve`'s counters are all
//!   index-used, which is why its −1.05% survives this rule untouched.
//! * *a double-consuming read inside a loop* — a conversion outside every loop
//!   runs once. `return iter` after the loop is not a reason to refuse.
//!
//! ## Deliberate under-approximation
//!
//! This analysis can only ever **remove** selections, so every uncertainty is
//! resolved toward "not a cost": an expression form this module does not model
//! contributes `Neutral`, never `Double`. Two consequences worth naming:
//!
//! * **Comparison is always neutral**, on both sides, even against a
//!   non-integer operand. `for (let i = 0; i < n; i++)` with a `number`
//!   parameter `n` is the single most common loop in JavaScript; classifying
//!   its guard as a double consumer would refuse nearly every counter in the
//!   corpus to buy nothing measurable.
//! * **A write of a local into its own slot never costs** (`iter = iter + 1`),
//!   because the value never leaves the representation it arrived in — but it
//!   still *counts as a benefit* when it lands in an i32-consuming position.
//!   `seed = (Math.imul(seed, K) + C) & 0x7fffffff` is a local whose only read
//!   is inside its own assignment and whose whole reason to want an i32 slot
//!   is that read.
//!
//! A missed refusal is today's behaviour. A spurious refusal is a lost
//! promotion, so the model is built to err in the first direction.
//!
//! ## Scope
//!
//! Consumed only by the canonical-i32 gate (`canonical_i32_value_eligible` in
//! `stmt/let_stmt.rs`), never by the parallel-shadow `needs_i32_slot` gate — so
//! `PERRY_CANONICAL_I32_LOCALS=0` still reproduces the pre-phase model
//! bit-for-bit, the same containment `int_valued_ta_locals` and
//! `loop_bounded_i32_locals` use. canonical `Str` and `Ptr<*>` are untouched;
//! they have the same disease (#7128 findings C and D) and are the next
//! consumers this model is shaped for, but widening it to them is a separate
//! measurement.

use std::collections::HashSet;

use perry_hir::{BinaryOp, Expr, Stmt, UnaryOp};

/// What the *consumer* of a read wants the value to be.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum UseCtx {
    /// The consumer takes an i32 operand directly: an index, a bitwise
    /// operand, `Math.imul`, or a write into another local that already has
    /// i32 storage. This is what canonical i32 buys.
    Int,
    /// The consumer needs an f64: a call/`new` argument (Perry's ABI passes
    /// NaN-boxed doubles), a `/` operand, a `return`, or a write into a local
    /// whose storage is a boxed double. This is what canonical i32 costs.
    Double,
    /// Equal cost in both representations, or a form this model does not
    /// claim to understand. Contributes to neither side.
    Neutral,
}

/// The locals that already hold i32 storage under the *parallel-shadow* model,
/// i.e. independently of any canonical selection. Writing into one of these is
/// an i32-consuming use; writing into anything else is a double-consuming use.
pub(crate) struct I32StorageFacts<'a> {
    pub index_used: &'a HashSet<u32>,
    pub strictly_bounded: &'a HashSet<u32>,
    pub unsigned: &'a HashSet<u32>,
    pub int_valued_ta: &'a HashSet<u32>,
}

impl I32StorageFacts<'_> {
    fn holds_i32(&self, id: u32) -> bool {
        self.index_used.contains(&id)
            || self.strictly_bounded.contains(&id)
            || self.unsigned.contains(&id)
            || self.int_valued_ta.contains(&id)
    }
}

#[derive(Default, Clone, Copy)]
struct Tally {
    declared: bool,
    writes_after_decl: u32,
    int_reads: u32,
    /// Double-consuming reads at loop depth ≥ 1 only. A conversion outside
    /// every loop runs once and is not a reason to refuse a representation.
    hot_double_reads: u32,
}

impl Tally {
    fn unprofitable(&self) -> bool {
        self.declared
            && self.writes_after_decl >= 1
            && self.int_reads == 0
            && self.hot_double_reads >= 1
    }
}

struct Model<'a> {
    storage: &'a I32StorageFacts<'a>,
    tallies: std::collections::HashMap<u32, Tally>,
    /// The local currently being written, if the walk is inside the RHS of
    /// `LocalSet(t, …)`. A read of `t` there is representation-preserving.
    self_target: Option<u32>,
}

impl<'a> Model<'a> {
    fn entry(&mut self, id: u32) -> &mut Tally {
        self.tallies.entry(id).or_default()
    }

    fn read(&mut self, id: u32, ctx: UseCtx, depth: u32) {
        // A read of a local inside its own assignment is representation-
        // PRESERVING, so it is never a cost — `iter = iter + 1` costs one
        // instruction whichever slot `iter` lives in. It is still a benefit
        // when it lands in an i32-consuming position, and suppressing that
        // half was wrong: `seed = (Math.imul(seed, K) + C) & 0x7fffffff` is a
        // local whose ONLY read is inside its own assignment and whose whole
        // reason to want an i32 slot is that read
        // (`benchmarks/suite/17_loop_data_dependent.ts`, and every FNV /
        // xorshift mixer in the corpus).
        let is_self = self.self_target == Some(id);
        match ctx {
            UseCtx::Int => self.entry(id).int_reads += 1,
            UseCtx::Double if depth >= 1 && !is_self => self.entry(id).hot_double_reads += 1,
            _ => {}
        }
    }

    /// The context a value acquires by being stored into `target`.
    fn target_ctx(&self, target: u32) -> UseCtx {
        if self.storage.holds_i32(target) {
            UseCtx::Int
        } else {
            UseCtx::Double
        }
    }

    fn expr(&mut self, e: &Expr, ctx: UseCtx, depth: u32) {
        match e {
            Expr::LocalGet(id) => self.read(*id, ctx, depth),
            Expr::LocalSet(id, value) => {
                self.entry(*id).writes_after_decl += 1;
                let target_ctx = self.target_ctx(*id);
                let saved = self.self_target.replace(*id);
                self.expr(value, target_ctx, depth);
                self.self_target = saved;
            }
            Expr::Update { id, .. } => {
                self.entry(*id).writes_after_decl += 1;
            }
            // An index is the canonical i32-consuming position: with a boxed
            // local this is exactly the `fptosi` per access the representation
            // exists to delete.
            Expr::IndexGet { object, index } => {
                self.expr(object, UseCtx::Neutral, depth);
                self.expr(index, UseCtx::Int, depth);
            }
            Expr::IndexSet {
                object,
                index,
                value,
            } => {
                self.expr(object, UseCtx::Neutral, depth);
                self.expr(index, UseCtx::Int, depth);
                // The element's own representation depends on the array kind,
                // which this model does not track: neutral, not a cost.
                self.expr(value, UseCtx::Neutral, depth);
            }
            Expr::IndexUpdate { object, index, .. } => {
                self.expr(object, UseCtx::Neutral, depth);
                self.expr(index, UseCtx::Int, depth);
            }
            Expr::Binary { op, left, right } => {
                let child = match op {
                    // Bitwise operands are ToInt32-coerced by the language, so
                    // an i32 slot feeds them with no conversion at all.
                    BinaryOp::BitAnd
                    | BinaryOp::BitOr
                    | BinaryOp::BitXor
                    | BinaryOp::Shl
                    | BinaryOp::Shr
                    | BinaryOp::UShr => UseCtx::Int,
                    // `/` and `**` are floating-point in JS regardless of the
                    // operands, so an i32 operand must be converted.
                    BinaryOp::Div | BinaryOp::Pow => UseCtx::Double,
                    // Additive / multiplicative chains inherit their root:
                    // `a[i * 4 + k]` keeps `i` and `k` integral, `sum + i * 2`
                    // makes them doubles.
                    BinaryOp::Add | BinaryOp::Sub | BinaryOp::Mul | BinaryOp::Mod => ctx,
                };
                self.expr(left, child, depth);
                self.expr(right, child, depth);
            }
            Expr::Unary { op, operand } => {
                let child = match op {
                    UnaryOp::BitNot => UseCtx::Int,
                    UnaryOp::Neg | UnaryOp::Pos => ctx,
                    UnaryOp::Not => UseCtx::Neutral,
                };
                self.expr(operand, child, depth);
            }
            // A comparison costs one instruction in either representation; see
            // the module header on why this is neutral rather than a cost.
            Expr::Compare { left, right, .. } => {
                self.expr(left, UseCtx::Neutral, depth);
                self.expr(right, UseCtx::Neutral, depth);
            }
            Expr::MathImul(a, b) => {
                self.expr(a, UseCtx::Int, depth);
                self.expr(b, UseCtx::Int, depth);
            }
            // Perry's calling convention passes NaN-boxed doubles, so every
            // argument position converts an i32 operand back.
            Expr::Call { callee, args, .. } => {
                self.expr(callee, UseCtx::Neutral, depth);
                for a in args {
                    self.expr(a, UseCtx::Double, depth);
                }
            }
            Expr::New { args, .. } => {
                for a in args {
                    self.expr(a, UseCtx::Double, depth);
                }
            }
            Expr::Conditional {
                condition,
                then_expr,
                else_expr,
            } => {
                self.expr(condition, UseCtx::Neutral, depth);
                self.expr(then_expr, ctx, depth);
                self.expr(else_expr, ctx, depth);
            }
            // Closure bodies are not walked: closure-referenced locals are
            // excluded from canonical selection wholesale by
            // `collect_closure_referenced_locals`, so nothing inside one can
            // change a verdict.
            Expr::Closure { .. } => {}
            // Everything else: recurse neutrally. Under-approximating the cost
            // is the safe direction for a rule that can only refuse.
            _ => {
                let saved = self.self_target.take();
                perry_hir::walker::walk_expr_children(e, &mut |child| {
                    self.expr(child, UseCtx::Neutral, depth)
                });
                self.self_target = saved;
            }
        }
    }

    fn stmts(&mut self, stmts: &[Stmt], depth: u32) {
        for s in stmts {
            self.stmt(s, depth);
        }
    }

    fn stmt(&mut self, s: &Stmt, depth: u32) {
        match s {
            Stmt::Let { id, init, .. } => {
                self.entry(*id).declared = true;
                if let Some(e) = init {
                    let ctx = self.target_ctx(*id);
                    self.expr(e, ctx, depth);
                }
            }
            Stmt::Expr(e) => self.expr(e, UseCtx::Neutral, depth),
            Stmt::Return(Some(e)) | Stmt::Throw(e) => self.expr(e, UseCtx::Double, depth),
            Stmt::If {
                condition,
                then_branch,
                else_branch,
            } => {
                self.expr(condition, UseCtx::Neutral, depth);
                self.stmts(then_branch, depth);
                if let Some(eb) = else_branch {
                    self.stmts(eb, depth);
                }
            }
            Stmt::While { condition, body } | Stmt::DoWhile { body, condition } => {
                self.expr(condition, UseCtx::Neutral, depth + 1);
                self.stmts(body, depth + 1);
            }
            Stmt::For {
                init,
                condition,
                update,
                body,
            } => {
                if let Some(i) = init {
                    self.stmt(i, depth);
                }
                if let Some(c) = condition {
                    self.expr(c, UseCtx::Neutral, depth + 1);
                }
                if let Some(u) = update {
                    self.expr(u, UseCtx::Neutral, depth + 1);
                }
                self.stmts(body, depth + 1);
            }
            Stmt::Labeled { body, .. } => self.stmt(body, depth),
            Stmt::Try {
                body,
                catch,
                finally,
            } => {
                self.stmts(body, depth);
                if let Some(c) = catch {
                    self.stmts(&c.body, depth);
                }
                if let Some(f) = finally {
                    self.stmts(f, depth);
                }
            }
            Stmt::Switch {
                discriminant,
                cases,
            } => {
                self.expr(discriminant, UseCtx::Neutral, depth);
                for c in cases {
                    if let Some(t) = &c.test {
                        self.expr(t, UseCtx::Neutral, depth);
                    }
                    self.stmts(&c.body, depth);
                }
            }
            Stmt::Return(None)
            | Stmt::Break
            | Stmt::Continue
            | Stmt::LabeledBreak(_)
            | Stmt::LabeledContinue(_)
            | Stmt::PreallocateBoxes(_)
            | Stmt::PreallocateTdzBoxes(_) => {}
        }
    }
}

/// Locals whose canonical-i32 promotion would emit strictly more work than the
/// boxed representation: written after declaration, no i32-consuming read
/// anywhere in the body, and at least one double-consuming read inside a loop.
///
/// The result is a **refusal** set — subtracted from the Let-site eligibility
/// conjunction, never added to it.
pub(crate) fn collect_unprofitable_canonical_i32_locals(
    stmts: &[Stmt],
    storage: &I32StorageFacts<'_>,
) -> HashSet<u32> {
    let mut model = Model {
        storage,
        tallies: std::collections::HashMap::new(),
        self_target: None,
    };
    model.stmts(stmts, 0);
    model
        .tallies
        .into_iter()
        .filter(|(_, t)| t.unprofitable())
        .map(|(id, _)| id)
        .collect()
}

#[cfg(test)]
#[path = "repsel_benefit/tests.rs"]
mod tests;
