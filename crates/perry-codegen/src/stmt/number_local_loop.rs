//! #10511: a loop whose bitwise operators read locals the compiler cannot
//! prove are Numbers, versioned on ONE entry test per local.
//!
//! The shape is noble's SHA-2 round and every hash like it:
//!
//! ```ts
//! let { A, B, C, D } = this;            // or `let A: number = st.A`
//! for (let i = 0; i < n; i++) {
//!   const t = (rotr(A, 7) ^ ((B & C) ^ (~B & D))) + i | 0;
//!   D = C; C = B; B = A; A = t;
//! }
//! ```
//!
//! Nothing proves `A..D` are Numbers at the function scope: their first write
//! is a property read, and a declared `number` is a hint, not a proof. So
//! every bitwise operand pays the inline guard (`|v| < 2^63`, a cold helper
//! arm), every copy pays the boxed-write protocol, and the values round-trip
//! through doubles between operators.
//!
//! The rule (charter step 5L — Number is a dataflow fact, not a per-site
//! admission): a local is Number at every read inside the loop when
//!
//! 1. it holds a Number when the loop is entered — tested once, here; and
//! 2. every write to it that can execute inside the loop — in the body, the
//!    update clause AND the condition, at any nesting depth — produces a
//!    Number whenever the candidates read by that write are Numbers
//!    (a greatest fixed point, so `D = C; C = B; B = A; A = t` proves all
//!    four together); and
//! 3. nothing else can write it: it is not captured by any closure, not a
//!    boxed / mapped-`arguments` cell, not a module global, and no closure,
//!    `await` or `yield` appears in the loop.
//!
//! (1) is the induction base, (2) the step, (3) closes the set of writes. The
//! condition and update clauses are walked exactly like the body: a write in
//! `for (…; …; i++, s = "a")` withdraws `s`. When the entry test passes, the
//! loop runs in a clone whose 5L Number scope
//! (`ReceiverDescriptorTable::materialize_number_locals`) holds the admitted
//! locals; `local_is_number` answers from it, so their bitwise operators lower
//! natively and their writes carry no pointer protocol; each lives in a plain
//! F64 alloca for the clone's duration and is written back to its slot on
//! every exit that can observe it. When the test fails, the ordinary loop
//! runs. A Number's representation is its raw double, so neither clone ever
//! puts a non-Number bit pattern where the other expects a JS value.
//!
//! Scope of the tier: loops that touch no receiver (no property or element
//! access) — those belong to the region tier that follows — and that use an
//! admitted local as a direct bitwise operand, the only shape where the clone
//! pays for its code size.

use std::collections::{BTreeSet, HashSet};

use anyhow::Result;
use perry_hir::{BinaryOp, Expr, Stmt, UnaryOp};

use super::loops::{emit_js_value_is_number, lower_for_after_init};
use crate::expr::{lower_expr, FnCtx};
use crate::types::I1;

/// Expression nodes beyond which the clone's code size is not worth it.
const MAX_LOOP_NODES: usize = 4000;

/// `PERRY_NUMBER_LOCAL_LOOP=0` keeps every loop on the ordinary lowering.
fn enabled() -> bool {
    !matches!(
        std::env::var("PERRY_NUMBER_LOCAL_LOOP").as_deref(),
        Ok("0") | Ok("off") | Ok("false")
    )
}

fn trace(what: &str) {
    use std::sync::OnceLock;
    static ON: OnceLock<bool> = OnceLock::new();
    if *ON.get_or_init(|| std::env::var("PERRY_PACKED_LOOP_TRACE").as_deref() == Ok("1")) {
        eprintln!("[number-local-loop] {what}");
    }
}

/// Everything the matcher learned about one loop's statements and clauses.
#[derive(Default)]
struct LoopFacts<'a> {
    /// Every write that can execute inside the loop, by local:
    /// `Some(rhs)` for an assignment, `None` for `++`/`--`.
    writes: std::collections::BTreeMap<u32, Vec<Option<&'a Expr>>>,
    /// Locals declared inside the loop (including the `for` init's): their
    /// value at entry is not the value the clone reads, so they are never
    /// entry-tested candidates.
    declared: HashSet<u32>,
    /// Locals that appear directly as an operand of a bitwise operator.
    bitwise_operands: HashSet<u32>,
    nodes: usize,
}

impl<'a> LoopFacts<'a> {
    /// `false` when the loop contains a construct this tier does not reason
    /// about; the caller then declines the whole loop.
    fn walk_stmts(&mut self, stmts: &'a [Stmt]) -> bool {
        stmts.iter().all(|stmt| self.walk_stmt(stmt))
    }

    fn walk_stmt(&mut self, stmt: &'a Stmt) -> bool {
        match stmt {
            Stmt::Let { id, init, .. } => {
                self.declared.insert(*id);
                init.as_ref().is_none_or(|init| self.walk_expr(init))
            }
            Stmt::Expr(expr) | Stmt::Throw(expr) => self.walk_expr(expr),
            Stmt::Return(value) => value.as_ref().is_none_or(|value| self.walk_expr(value)),
            Stmt::Break | Stmt::Continue => true,
            Stmt::If {
                condition,
                then_branch,
                else_branch,
            } => {
                self.walk_expr(condition)
                    && self.walk_stmts(then_branch)
                    && else_branch
                        .as_deref()
                        .is_none_or(|branch| self.walk_stmts(branch))
            }
            Stmt::While { condition, body } | Stmt::DoWhile { body, condition } => {
                self.walk_expr(condition) && self.walk_stmts(body)
            }
            Stmt::For {
                init,
                condition,
                update,
                body,
            } => {
                init.as_deref().is_none_or(|init| self.walk_stmt(init))
                    && condition.as_ref().is_none_or(|c| self.walk_expr(c))
                    && update.as_ref().is_none_or(|u| self.walk_expr(u))
                    && self.walk_stmts(body)
            }
            // try/switch/labels and the box-management statements carry
            // control flow or storage protocols this tier does not model.
            _ => false,
        }
    }

    fn walk_expr(&mut self, expr: &'a Expr) -> bool {
        self.nodes += 1;
        if self.nodes > MAX_LOOP_NODES {
            return false;
        }
        match expr {
            // A closure could capture, an `await`/`yield` suspends with the
            // frame's storage protocol, and a receiver access belongs to the
            // region tier that runs after this one.
            Expr::Closure { .. }
            | Expr::Await(_)
            | Expr::Yield { .. }
            | Expr::PropertyGet { .. }
            | Expr::PropertySet { .. }
            | Expr::IndexGet { .. }
            | Expr::IndexSet { .. }
            | Expr::PutValueSet { .. } => return false,
            Expr::LocalSet(id, value) => {
                self.writes.entry(*id).or_default().push(Some(value));
            }
            Expr::Update { id, .. } => {
                self.writes.entry(*id).or_default().push(None);
            }
            Expr::Binary { op, left, right } if is_bitwise(*op) => {
                for operand in [left.as_ref(), right.as_ref()] {
                    if let Expr::LocalGet(id) = operand {
                        self.bitwise_operands.insert(*id);
                    }
                }
            }
            Expr::Unary {
                op: UnaryOp::BitNot,
                operand,
            } => {
                if let Expr::LocalGet(id) = operand.as_ref() {
                    self.bitwise_operands.insert(*id);
                }
            }
            _ => {}
        }
        let mut ok = true;
        perry_hir::walker::walk_expr_children(expr, &mut |child| {
            ok = ok && self.walk_expr(child);
        });
        ok
    }
}

fn is_bitwise(op: BinaryOp) -> bool {
    matches!(
        op,
        BinaryOp::BitAnd
            | BinaryOp::BitOr
            | BinaryOp::BitXor
            | BinaryOp::Shl
            | BinaryOp::Shr
            | BinaryOp::UShr
    )
}

/// Does evaluating `expr` produce a Number (or throw) whenever every local in
/// `numbers` holds a Number? The rule's step, per operator:
///
/// * `+` needs both operands Number (otherwise it may concatenate);
/// * the other arithmetic and bitwise operators need ONE: a Number operand
///   paired with a BigInt throws, so the result is a Number or nothing;
/// * `>>>`, unary `+` and `Number(x)` produce a Number or throw for any input;
/// * `-x`, `~x` and `x++` keep a BigInt a BigInt, so they need `x` Number.
///
/// Anything else defers to the function-scope proof (`is_numeric_expr`).
fn produces_number(ctx: &FnCtx<'_>, expr: &Expr, numbers: &BTreeSet<u32>) -> bool {
    match expr {
        Expr::Integer(_) | Expr::Number(_) => true,
        Expr::LocalGet(id) => {
            numbers.contains(id) || crate::type_analysis::is_numeric_expr(ctx, expr)
        }
        Expr::LocalSet(_, value) => produces_number(ctx, value, numbers),
        Expr::Update { id, .. } => {
            numbers.contains(id) || crate::type_analysis::is_numeric_expr(ctx, &Expr::LocalGet(*id))
        }
        Expr::Binary { op, left, right } => match op {
            BinaryOp::Add => {
                produces_number(ctx, left, numbers) && produces_number(ctx, right, numbers)
            }
            BinaryOp::UShr => true,
            _ => produces_number(ctx, left, numbers) || produces_number(ctx, right, numbers),
        },
        Expr::Unary { op, operand } => match op {
            UnaryOp::Pos => true,
            UnaryOp::Neg | UnaryOp::BitNot => produces_number(ctx, operand, numbers),
            UnaryOp::Not => false,
        },
        Expr::NumberCoerce(_) => true,
        Expr::Conditional {
            then_expr,
            else_expr,
            ..
        } => produces_number(ctx, then_expr, numbers) && produces_number(ctx, else_expr, numbers),
        _ => crate::type_analysis::is_numeric_expr(ctx, expr),
    }
}

/// The admitted locals, or `None` when the loop is not this tier's.
fn match_number_locals(
    ctx: &FnCtx<'_>,
    init: Option<&Stmt>,
    condition: Option<&Expr>,
    update: Option<&Expr>,
    body: &[Stmt],
) -> Option<Vec<u32>> {
    if !ctx.pending_labels.is_empty() || ctx.try_depth != 0 || ctx.is_async_fn {
        return None;
    }
    let mut facts = LoopFacts::default();
    // The init runs once, before the entry test: its declarations are loop
    // locals, and its writes are covered by the test itself.
    if let Some(Stmt::Let { id, .. }) = init {
        facts.declared.insert(*id);
    }
    let walked = condition.is_none_or(|c| facts.walk_expr(c))
        && update.is_none_or(|u| facts.walk_expr(u))
        && facts.walk_stmts(body);
    if !walked {
        return None;
    }
    let mut numbers: BTreeSet<u32> = facts
        .writes
        .keys()
        .copied()
        .filter(|id| {
            !facts.declared.contains(id)
                && ctx.locals.contains_key(id)
                && !ctx.boxed_vars.contains(id)
                && !ctx.closure_captures.contains_key(id)
                && !ctx.repsel_closure_ref_locals.contains(id)
                && !ctx.module_globals.contains_key(id)
                && !ctx.i32_counter_slots.contains_key(id)
                && !ctx.local_slot_reps.contains_key(id)
                && !crate::type_analysis::local_is_number(ctx, *id)
                && !ctx.numeric_accumulator_f64_slots.contains_key(id)
        })
        .collect();
    loop {
        let rejected: Vec<u32> = numbers
            .iter()
            .copied()
            .filter(|id| {
                !facts.writes[id].iter().all(|write| match write {
                    Some(rhs) => produces_number(ctx, rhs, &numbers),
                    // `x++` on a Number is a Number.
                    None => true,
                })
            })
            .collect();
        if rejected.is_empty() {
            break;
        }
        for id in rejected {
            numbers.remove(&id);
        }
    }
    if !numbers.iter().any(|id| facts.bitwise_operands.contains(id)) {
        return None;
    }
    Some(numbers.into_iter().collect())
}

/// Try the tier. `init` has already been lowered by the caller.
pub(super) fn lower(
    ctx: &mut FnCtx<'_>,
    init: Option<&Stmt>,
    condition: Option<&Expr>,
    update: Option<&Expr>,
    body: &[Stmt],
) -> Result<bool> {
    if !enabled() {
        return Ok(false);
    }
    let Some(numbers) = match_number_locals(ctx, init, condition, update, body) else {
        return Ok(false);
    };
    trace(&format!("admitted {} local(s)", numbers.len()));

    // The induction base: one Number test per admitted local, read through
    // the ordinary `LocalGet` lowering so every storage protocol is honoured.
    let mut all_numbers: Option<String> = None;
    let mut entry_values: Vec<(u32, String)> = Vec::with_capacity(numbers.len());
    for id in &numbers {
        let value = lower_expr(ctx, &Expr::LocalGet(*id))?;
        entry_values.push((*id, value.clone()));
        let is_number = emit_js_value_is_number(ctx, &value);
        all_numbers = Some(match all_numbers {
            Some(prev) => ctx.block().and(I1, &prev, &is_number),
            None => is_number,
        });
    }
    let all_numbers = all_numbers.expect("the matcher admits at least one local");

    let fast_idx = ctx.new_block("for.number_locals.fast.preheader");
    let slow_idx = ctx.new_block("for.number_locals.slow.preheader");
    let merge_idx = ctx.new_block("for.number_locals.merge");
    let fast_label = ctx.block_label(fast_idx);
    let slow_label = ctx.block_label(slow_idx);
    let merge_label = ctx.block_label(merge_idx);
    ctx.block().cond_br(&all_numbers, &fast_label, &slow_label);

    ctx.current_block = fast_idx;
    let scope_id = ctx.next_loop_proof_scope_id();
    ctx.receiver_descriptors
        .materialize_number_locals(scope_id, &numbers);
    // Inside the clone each admitted local lives in a plain F64 alloca — the
    // packed tiers' unboxed-accumulator redirect (`LocalGet`/`LocalSet` read
    // and write `numeric_accumulator_f64_slots`), so LLVM keeps the value in
    // a register instead of reloading it through the root slot's protocol
    // at every use. A Number carries no heap edge, so the alloca needs no
    // root. The real slot keeps the entry value (a Number) for the clone's
    // duration and is brought up to date on every way out that can observe
    // it: the loop's fall-through/`break` exit below, and every `throw`
    // site (`flush_packed_accumulator_locals`). A `return` leaves the
    // function, and no closure can read these locals (admission rule 3).
    let mut redirected: Vec<(u32, String, String)> = Vec::with_capacity(entry_values.len());
    for (id, value) in &entry_values {
        let Some(real_slot) = ctx.locals.get(id).cloned() else {
            continue;
        };
        let alloca = ctx.func.alloca_entry(crate::types::DOUBLE);
        ctx.block().store(crate::types::DOUBLE, value, &alloca);
        ctx.numeric_accumulator_f64_slots
            .insert(*id, alloca.clone());
        redirected.push((*id, alloca, real_slot));
    }
    let fast = lower_for_after_init(ctx, init, condition, update, body, "for.number_locals_fast");
    if fast.is_ok() && !ctx.block().is_terminated() {
        for (_, alloca, real_slot) in &redirected {
            // Same argument as the packed tiers' `finish`: a Number's bits
            // are its NaN-box and carry no heap edge, so no barrier.
            let value = ctx.block().load(crate::types::DOUBLE, alloca);
            ctx.block().store(crate::types::DOUBLE, &value, real_slot);
        }
    }
    for (id, _, _) in &redirected {
        ctx.numeric_accumulator_f64_slots.remove(id);
    }
    ctx.receiver_descriptors.dematerialize_scope(scope_id);
    fast?;
    if !ctx.block().is_terminated() {
        ctx.block().br(&merge_label);
    }

    ctx.current_block = slow_idx;
    lower_for_after_init(ctx, init, condition, update, body, "for.number_locals_slow")?;
    if !ctx.block().is_terminated() {
        ctx.block().br(&merge_label);
    }

    ctx.current_block = merge_idx;
    Ok(true)
}
