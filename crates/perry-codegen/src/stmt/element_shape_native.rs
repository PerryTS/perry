//! The element-shape fast clone's loop-carried scalars in their native
//! machine domains: the accumulator as an `f64`, the counter as an `i32`.
//!
//! ## What this removes
//!
//! Before this module the `repeat` clone (`sum += rows[7].id`) spent ~13 CPU
//! cycles per iteration on 16 instructions. Not instruction count: two
//! loop-carried chains crossed the integer/float register boundary every
//! iteration.
//!
//! * **The accumulator.** An `any`-typed `sum` lives in a precise GC root slot,
//!   and every reload of a root slot passes through the RS4GC launder
//!   (`function/precise_roots.rs`'s `ROOT_RELOAD_LAUNDER`, an `asm` identity
//!   LLVM cannot see through). mem2reg therefore promoted the slot as NaN-box
//!   `i64` bits, and the chain was `x25 → fmov → fadd → fmov → x25`.
//! * **The counter.** A counter the clone never uses as an index (the
//!   constant-index and carried forms) has no i32 slot, so it stayed a double
//!   (`fadd d8, d8, #1.0`) compared against `count` — itself a root slot,
//!   reloaded through the same launder and re-transferred to an FP register on
//!   every iteration, although the preheader had already materialized it as an
//!   i32.
//!
//! ## What replaces it
//!
//! Both scalars move into plain promotable allocas for the fast clone's
//! lowering, exactly the redirects the packed clones already use
//! (`FnCtx::numeric_accumulator_f64_slots`,
//! `FnCtx::deferred_integer_update_accumulators`):
//!
//! * the accumulator's live value is an `alloca double` seeded in the fast
//!   preheader with the value the deref block just tag-tested as a Number, so
//!   every in-clone read and write is a `double` and mem2reg makes it an FP phi;
//! * the counter gets an i32 slot (reused if it already owns a parallel one,
//!   clone-private otherwise) that the `Update` lowering advances ALONE, so the
//!   precomputed i32 trip count turns the condition into `icmp slt i32`.
//!
//! The real slots are written back at the two places the clone can leave:
//! a side-exit trampoline every residual check branches to instead of the slow
//! preheader, and a block on the fall-through exit.
//!
//! ## Why this changes no observable behaviour
//!
//! * **Commit ordering (#10185) is untouched.** The redirected store happens at
//!   exactly the point the root-slot store used to, and the matcher already
//!   put that point after every side exit of the iteration (the K-statement
//!   fold, the carried commit last). At any side exit the f64 alloca therefore
//!   holds the value the iteration was entered with, the i32 counter holds the
//!   iteration's own index (the `Update` runs after the body), and the
//!   trampoline publishes exactly the state the slow clone must re-run the
//!   iteration from.
//! * **Call-freeness is untouched.** The seed, the trampoline and the
//!   write-back are plain loads, stores and one `sitofp`; the trampoline is
//!   created inside the scanned block range, so the post-emission
//!   `contains_gc_unsafe_call` scan still sees it. No poll is emitted in these
//!   clones (`emit_gc_loop_safepoint`), so no collection observes the stale
//!   root slot, and a stale slot holds a Number, which a scan treats as data.
//! * **JS `+`.** The redirect admits no write the clone did not already lower
//!   as a bare `fadd`: the accumulator is the fact's `numeric_accumulator`,
//!   which `is_numeric_expr` already trusted as a raw double inside this clone.
//!   Keeping that same double in a register instead of a stack slot is the
//!   same IEEE arithmetic on the same operands in the same order, so `-0`, NaN
//!   and overflow to Infinity are bit-identical.
//! * **The i32 counter.** The trip count is always an i32 in the fast clone —
//!   a literal in `0..=i32::MAX`, `arr.length`, or `materialize_loop_i32`'s
//!   integral `0..=i32::MAX` — and the start is an integer literal in the same
//!   range, so `i < bound <= i32::MAX` before every `add i32 1`: no wrap. A
//!   fractional, NaN, negative or out-of-range `count` never reaches the clone.
//!
//! Every exclusion below declines ONE redirect and leaves that scalar exactly
//! as it was, never the clone.

use crate::expr::FnCtx;
use crate::types::{DOUBLE, I32};

/// The scalars one fast clone moved into native storage, and where the clone
/// must publish them back.
pub(super) struct NativeLoopDomains {
    /// `(local, f64 alloca, real slot)`.
    accumulator: Option<(u32, String, String)>,
    /// The counter's deferred double storage.
    counter: Option<DeferredCounter>,
    /// The label residual checks branch to: the write-back trampoline, or the
    /// slow preheader itself when nothing was redirected.
    side_exit: String,
}

struct DeferredCounter {
    id: u32,
    i32_slot: String,
    double_slot: String,
    /// The slot was minted for this clone and must be unregistered after it,
    /// so the slow clone and the code after the loop see the counter exactly
    /// as they did before.
    private: bool,
}

/// Can `id` carry the unboxed accumulator redirect? Mirrors the packed clones'
/// admission (`stable_packed_accumulator::collect_numeric_accumulators`) minus
/// the numeric fixpoint, which the element-shape matcher already discharged.
fn accumulator_is_redirectable(ctx: &FnCtx<'_>, id: u32) -> bool {
    ctx.locals.contains_key(&id)
        && !ctx.boxed_vars.contains(&id)
        && !ctx.closure_captures.contains_key(&id)
        && !ctx.module_globals.contains_key(&id)
        // An i32 (parallel or canonical) or Str representation has storage the
        // redirect would leave stale; a POD or scalar-replaced local is not a
        // single slot at all.
        && !ctx.i32_counter_slots.contains_key(&id)
        && !ctx.local_slot_reps.contains_key(&id)
        && !ctx.pod_records.contains_key(&id)
        && !ctx.scalar_replaced.contains_key(&id)
        // An enclosing clone already redirected it; its scope owns the alloca
        // and the write-back, and overwriting the entry would strand both.
        && !ctx.numeric_accumulator_f64_slots.contains_key(&id)
}

impl NativeLoopDomains {
    /// Emit the seeds into the current block (the fast preheader) and register
    /// both redirects for the fast clone's lowering.
    ///
    /// `accumulator_value` is the value the deref block tag-tested as a Number;
    /// the deref block dominates the fast preheader and nothing is stored in
    /// between, so it is the slot's current value.
    pub(super) fn enter(
        ctx: &mut FnCtx<'_>,
        counter_id: u32,
        counter_start: i64,
        accumulator_id: u32,
        accumulator_value: &str,
        slow_pre_label: &str,
    ) -> Self {
        let accumulator = accumulator_is_redirectable(ctx, accumulator_id).then(|| {
            let real_slot = ctx.locals[&accumulator_id].clone();
            let alloca = ctx.func.alloca_entry(DOUBLE);
            ctx.block().store(DOUBLE, accumulator_value, &alloca);
            ctx.numeric_accumulator_f64_slots
                .insert(accumulator_id, alloca.clone());
            (accumulator_id, alloca, real_slot)
        });

        let counter = Self::defer_counter(ctx, counter_id, counter_start);

        let side_exit = if accumulator.is_none() && counter.is_none() {
            slow_pre_label.to_string()
        } else {
            let tramp_idx = ctx.new_block("element_shape.loop.side_exit");
            let saved = ctx.current_block;
            ctx.current_block = tramp_idx;
            emit_write_back(ctx, accumulator.as_ref(), counter.as_ref());
            ctx.block().br(slow_pre_label);
            ctx.current_block = saved;
            ctx.block_label(tramp_idx)
        };

        Self {
            accumulator,
            counter,
            side_exit,
        }
    }

    fn defer_counter(ctx: &mut FnCtx<'_>, id: u32, start: i64) -> Option<DeferredCounter> {
        // A canonical-i32 counter has no double storage to defer — its `Update`
        // is already one `add i32`.
        if crate::expr::canonical_local_i32_slot(ctx, id).is_some()
            || ctx.boxed_vars.contains(&id)
            || ctx.closure_captures.contains_key(&id)
            || ctx.module_globals.contains_key(&id)
            || ctx.unsigned_i32_locals.contains(&id)
            || ctx.deferred_integer_update_accumulators.contains(&id)
            || !(0..=i64::from(i32::MAX)).contains(&start)
        {
            return None;
        }
        let double_slot = ctx.locals.get(&id)?.clone();
        let (i32_slot, private) = match ctx.i32_counter_slots.get(&id) {
            // A Let-site parallel slot: every write mirrors it, so it already
            // holds the counter's value.
            Some(slot) => (slot.clone(), false),
            None => {
                let slot = ctx.func.alloca_entry(I32);
                // The init was lowered before the clone was chosen and nothing
                // wrote the counter since, so its value is the literal start.
                ctx.block().store(I32, &start.to_string(), &slot);
                ctx.i32_counter_slots.insert(id, slot.clone());
                (slot, true)
            }
        };
        ctx.deferred_integer_update_accumulators.insert(id);
        Some(DeferredCounter {
            id,
            i32_slot,
            double_slot,
            private,
        })
    }

    /// The label every residual check of the fast clone must branch to.
    pub(super) fn side_exit_label(&self) -> &str {
        &self.side_exit
    }

    /// Publish the live values on the fall-through exit and end both
    /// redirects. Must run right after the fast clone is lowered and BEFORE
    /// the slow clone is, which reads and writes the real slots.
    pub(super) fn finish(self, ctx: &mut FnCtx<'_>, merge_label: &str) {
        let redirected = self.accumulator.is_some() || self.counter.is_some();
        if redirected && !ctx.block().is_terminated() {
            let commit_idx = ctx.new_block("element_shape.loop.fast.write_back");
            let commit_label = ctx.block_label(commit_idx);
            ctx.block().br(&commit_label);
            ctx.current_block = commit_idx;
            emit_write_back(ctx, self.accumulator.as_ref(), self.counter.as_ref());
            ctx.block().br(merge_label);
        }
        if let Some((id, _, _)) = &self.accumulator {
            ctx.numeric_accumulator_f64_slots.remove(id);
        }
        if let Some(counter) = &self.counter {
            ctx.deferred_integer_update_accumulators.remove(&counter.id);
            if counter.private {
                ctx.i32_counter_slots.remove(&counter.id);
            }
        }
    }
}

/// The write-back both exits share. A genuine double's bits are its NaN-box, so
/// the accumulator needs no conversion and — carrying no heap edge — no
/// barrier; the counter is an exact integer in `0..=i32::MAX`.
fn emit_write_back(
    ctx: &mut FnCtx<'_>,
    accumulator: Option<&(u32, String, String)>,
    counter: Option<&DeferredCounter>,
) {
    let blk = ctx.block();
    if let Some((_, alloca, real_slot)) = accumulator {
        let value = blk.load(DOUBLE, alloca);
        blk.store(DOUBLE, &value, real_slot);
    }
    if let Some(counter) = counter {
        let value = blk.load(I32, &counter.i32_slot);
        let as_double = blk.sitofp(I32, &value, DOUBLE);
        blk.store(DOUBLE, &as_double, &counter.double_slot);
    }
}
