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
//! natively and their writes carry no pointer protocol. They use raw Number
//! storage: normally F64, or the existing canonical i32 slot when a stable
//! bound and integral entry prove the entire increment budget cannot wrap.
//! Each is written back to its ordinary slot on
//! every exit that can observe it. When the test fails, the ordinary loop
//! runs. A Number's representation is its raw double, so neither clone ever
//! puts a non-Number bit pattern where the other expects a JS value.
//!
//! Scope of the tier: bitwise loops and typed byte-scanning loops. Byte reads
//! keep the existing checked element lowering and B4 owner proof; this tier
//! proves the loop-carried Number; byte-result numeric facts additionally require
//! B4 receiver admission.
//! Other receiver accesses belong to the region tier that follows.

mod bounded_counter;

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
    /// Locals used directly by bitwise operations or as typed-byte indices.
    numeric_consumers: HashSet<u32>,
    byte_receivers: BTreeSet<u32>,
    nodes: usize,
}

impl<'a> LoopFacts<'a> {
    /// `false` when the loop contains a construct this tier does not reason
    /// about; the caller then declines the whole loop.
    fn walk_stmts(&mut self, ctx: &FnCtx<'_>, stmts: &'a [Stmt]) -> bool {
        stmts.iter().all(|stmt| self.walk_stmt(ctx, stmt))
    }

    fn walk_stmt(&mut self, ctx: &FnCtx<'_>, stmt: &'a Stmt) -> bool {
        match stmt {
            Stmt::Let { id, init, .. } => {
                self.declared.insert(*id);
                init.as_ref().is_none_or(|init| self.walk_expr(ctx, init))
            }
            Stmt::Expr(expr) | Stmt::Throw(expr) => self.walk_expr(ctx, expr),
            Stmt::Return(value) => value
                .as_ref()
                .is_none_or(|value| self.walk_expr(ctx, value)),
            Stmt::Break | Stmt::Continue => true,
            Stmt::If {
                condition,
                then_branch,
                else_branch,
            } => {
                self.walk_expr(ctx, condition)
                    && self.walk_stmts(ctx, then_branch)
                    && else_branch
                        .as_deref()
                        .is_none_or(|branch| self.walk_stmts(ctx, branch))
            }
            Stmt::While { condition, body } | Stmt::DoWhile { body, condition } => {
                self.walk_expr(ctx, condition) && self.walk_stmts(ctx, body)
            }
            Stmt::For {
                init,
                condition,
                update,
                body,
            } => {
                init.as_deref().is_none_or(|init| self.walk_stmt(ctx, init))
                    && condition.as_ref().is_none_or(|c| self.walk_expr(ctx, c))
                    && update.as_ref().is_none_or(|u| self.walk_expr(ctx, u))
                    && self.walk_stmts(ctx, body)
            }
            // try/switch/labels and the box-management statements carry
            // control flow or storage protocols this tier does not model.
            _ => false,
        }
    }

    fn walk_expr(&mut self, ctx: &FnCtx<'_>, expr: &'a Expr) -> bool {
        self.nodes += 1;
        if self.nodes > MAX_LOOP_NODES {
            return false;
        }
        match expr {
            // A closure could capture, an `await`/`yield` suspends with the
            // frame's storage protocol, and other receiver accesses belong to the
            // region tier that runs after this one.
            Expr::Closure { .. }
            | Expr::Await(_)
            | Expr::Yield { .. }
            | Expr::PropertySet { .. }
            | Expr::IndexSet { .. }
            | Expr::PutValueSet { .. } => return false,
            Expr::PropertyGet {
                object, property, ..
            } => {
                // Admission is only for the local index. Keep full property
                // semantics for live length, including overrides/getters.
                if property != "length"
                    || crate::expr::ta_element_read::receiver_kind(ctx, object) != Some(1)
                {
                    return false;
                }
            }
            Expr::IndexGet { object, index } => {
                if crate::expr::ta_element_read::receiver_kind(ctx, object) != Some(1) {
                    return false;
                }
                if let Expr::LocalGet(id) = index.as_ref() {
                    self.numeric_consumers.insert(*id);
                }
            }
            Expr::Uint8ArrayGet {
                array: object,
                index,
            }
            | Expr::BufferIndexGet {
                buffer: object,
                index,
            } => {
                let Expr::LocalGet(id) = object.as_ref() else {
                    return false;
                };
                if ctx.reassigned_locals.contains(id) || ctx.boxed_vars.contains(id) {
                    return false;
                }
                self.byte_receivers.insert(*id);
                if let Expr::LocalGet(id) = index.as_ref() {
                    self.numeric_consumers.insert(*id);
                }
            }
            Expr::LocalSet(id, value) => {
                self.writes.entry(*id).or_default().push(Some(value));
            }
            Expr::Update { id, .. } => {
                self.writes.entry(*id).or_default().push(None);
            }
            Expr::Binary { op, left, right } if is_bitwise(*op) => {
                for operand in [left.as_ref(), right.as_ref()] {
                    if let Expr::LocalGet(id) = operand {
                        self.numeric_consumers.insert(*id);
                    }
                }
            }
            Expr::Unary {
                op: UnaryOp::BitNot,
                operand,
            } => {
                if let Expr::LocalGet(id) = operand.as_ref() {
                    self.numeric_consumers.insert(*id);
                }
            }
            _ => {}
        }
        let mut ok = true;
        perry_hir::walker::walk_expr_children(expr, &mut |child| {
            ok = ok && self.walk_expr(ctx, child);
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
        Expr::Uint8ArrayGet { .. } | Expr::BufferIndexGet { .. } => false,
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
) -> Option<(Vec<u32>, Vec<u32>)> {
    if !ctx.pending_labels.is_empty() || ctx.try_depth != 0 || ctx.is_async_fn {
        return None;
    }
    let mut facts = LoopFacts::default();
    // The init runs once, before the entry test: its declarations are loop
    // locals, and its writes are covered by the test itself.
    if let Some(Stmt::Let { id, .. }) = init {
        facts.declared.insert(*id);
    }
    let walked = condition.is_none_or(|c| facts.walk_expr(ctx, c))
        && update.is_none_or(|u| facts.walk_expr(ctx, u))
        && facts.walk_stmts(ctx, body);
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
    if !numbers
        .iter()
        .any(|id| facts.numeric_consumers.contains(id))
    {
        return None;
    }
    Some((
        numbers.into_iter().collect(),
        facts.byte_receivers.into_iter().collect(),
    ))
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
    let Some((numbers, byte_receivers)) = match_number_locals(ctx, init, condition, update, body)
    else {
        return Ok(false);
    };
    trace(&format!("admitted {} local(s)", numbers.len()));
    let int_plan = bounded_counter::match_plan(ctx, &numbers, condition, update, body);

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
    let mut all_numbers = all_numbers.expect("the matcher admits at least one local");
    // Reuse B4 admission before publishing numeric byte-result facts. A lying
    // receiver runs the original loop; detach/resize still invalidate storage.
    let mut receiver_proofs = Vec::new();
    for id in byte_receivers {
        if crate::expr::ta_element_read::byte_receiver_is_proven(ctx, &Expr::LocalGet(id)) {
            continue;
        }
        let boxed = lower_expr(ctx, &Expr::LocalGet(id))?;
        let Some(access) = crate::expr::u8_buffer_read::byte_view_param_for(
            ctx,
            &Expr::LocalGet(id),
            &boxed,
            &crate::expr::u8_buffer_read::U8_BRANDS,
        ) else {
            return Ok(false);
        };
        all_numbers = ctx.block().and(I1, &all_numbers, &access.valid_i1);
        receiver_proofs.push((id, ctx.snapshot_guarded_proof(&id)));
    }

    let int_guard = if let Some(plan) = &int_plan {
        let guard = super::loops::emit_guarded_i32_bound(
            ctx,
            plan.counter,
            plan.bound,
            perry_hir::CompareOp::Lt,
            update,
            body,
            "for.number_locals",
            Some(plan.extra_increments),
        )
        .expect("bounded Number counter has plain storage");
        let integral = ctx.block().load(I1, &guard.flag_slot);
        all_numbers = ctx.block().and(I1, &all_numbers, &integral);
        Some(guard)
    } else {
        None
    };

    let fast_idx = ctx.new_block("for.number_locals.fast.preheader");
    let slow_idx = ctx.new_block("for.number_locals.slow.preheader");
    let merge_idx = ctx.new_block("for.number_locals.merge");
    let fast_label = ctx.block_label(fast_idx);
    let slow_label = ctx.block_label(slow_idx);
    let merge_label = ctx.block_label(merge_idx);
    ctx.block().cond_br(&all_numbers, &fast_label, &slow_label);

    ctx.current_block = fast_idx;
    for (id, _) in &receiver_proofs {
        ctx.proven_local_types.insert(
            *id,
            perry_hir::types::Type::Union(vec![
                perry_hir::types::Type::Named("Uint8Array".into()),
                perry_hir::types::Type::Named("Buffer".into()),
            ]),
        );
    }
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
    let int_active = int_plan
        .as_ref()
        .zip(int_guard.as_ref())
        .map(|(plan, guard)| {
            bounded_counter::Active::begin(ctx, plan, guard.counter_i32_slot.clone())
        });
    for (id, value) in &entry_values {
        if int_plan.as_ref().is_some_and(|plan| plan.counter == *id) {
            continue;
        }
        let Some(real_slot) = ctx.locals.get(id).cloned() else {
            continue;
        };
        let alloca = ctx.func.alloca_entry(crate::types::DOUBLE);
        ctx.block().store(crate::types::DOUBLE, value, &alloca);
        ctx.numeric_accumulator_f64_slots
            .insert(*id, alloca.clone());
        redirected.push((*id, alloca, real_slot));
    }
    let int_bound = int_plan
        .as_ref()
        .zip(int_guard.as_ref())
        .map(|(plan, guard)| {
            let bound = ctx.block().load(crate::types::I32, &guard.bound_i32_slot);
            (plan.counter, bound)
        });
    let fast = super::loops::lower_for_after_init_with_i32_bound(
        ctx,
        init,
        condition,
        update,
        body,
        "for.number_locals_fast",
        int_bound,
    );
    if fast.is_ok() && !ctx.block().is_terminated() {
        if let Some(active) = &int_active {
            active.sync(ctx);
        }
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
    if let Some(active) = int_active {
        active.finish(ctx);
    }
    ctx.receiver_descriptors.dematerialize_scope(scope_id);
    for (id, previous) in receiver_proofs {
        if let Some(ty) = previous {
            ctx.proven_local_types.insert(id, ty);
        } else {
            ctx.proven_local_types.remove(&id);
        }
    }
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

/// Number admission proves the index, not the receiver. Checked byte reads
/// can still enter a collecting property fallback, so the scanner clone keeps
/// its ordinary armed poll on every back edge.
pub(super) fn has_guarded_byte_index(ctx: &FnCtx<'_>, body: &[Stmt], controls: &[&Expr]) -> bool {
    fn guarded(ctx: &FnCtx<'_>, expr: &Expr) -> bool {
        let index = match expr {
            Expr::Uint8ArrayGet { index, .. } | Expr::BufferIndexGet { index, .. } => index,
            Expr::IndexGet { object, index }
                if crate::expr::ta_element_read::receiver_kind(ctx, object) == Some(1) =>
            {
                index
            }
            _ => return false,
        };
        fn uses_guarded_local(ctx: &FnCtx<'_>, expr: &Expr) -> bool {
            let id = match expr {
                Expr::LocalGet(id) | Expr::Update { id, .. } | Expr::LocalSet(id, _) => Some(id),
                _ => None,
            };
            let mut found = id.is_some_and(|id| {
                ctx.numeric_accumulator_f64_slots.contains_key(id)
                    || (ctx.i32_counter_slots.contains_key(id)
                        && ctx.receiver_descriptors.local_is_number_in_scope(*id))
            });
            perry_hir::walker::walk_expr_children(expr, &mut |child| {
                found |= uses_guarded_local(ctx, child);
            });
            found
        }
        uses_guarded_local(ctx, index)
    }
    let mut found = false;
    crate::collectors::for_each_expr_in_stmts(body, &mut |e| found |= guarded(ctx, e));
    fn walk(ctx: &FnCtx<'_>, expr: &Expr) -> bool {
        let mut found = guarded(ctx, expr);
        perry_hir::walker::walk_expr_children(expr, &mut |e| found |= walk(ctx, e));
        found
    }
    found || controls.iter().any(|e| walk(ctx, e))
}
