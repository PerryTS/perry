//! The multi-point re-read scope and the accumulator combinators (#7615
//! slice 6), split out of `rooting/mod.rs` to stay under the 2000-line cap.
//!
//! Pure move: every item keeps its name, visibility and doc, and
//! `rooting/mod.rs` re-exports the `pub(crate)` surface by name, so callers
//! still say `crate::rooting::…`.

use anyhow::Result;
use perry_hir::Expr;

use crate::expr::FnCtx;
use crate::types::{LlvmType, DOUBLE, I64};

use super::temp_root;
use super::{call_void_with_roots, call_with_roots, read_slot, Arg, Repr, RootedSlot};

// ---------------------------------------------------------------------------
// The multi-point re-read scope (#7615 slice 6).
//
// WHY THE `with_operands_rooted*` FAMILY COULD NOT TAKE THE `lower_call/`
// MODULES, stated as a property of the API rather than of those files.
//
// Every form above re-reads at exactly ONE point: the end of the operand list,
// after an optional `across` step. That is the whole shape for `m.set(k, v)`
// and `u8[i]` — lower, protect, re-read once, consume once. Three lowerings in
// `lower_call/` are not that shape, and each is a different way of not being
// it:
//
//   * `lower_dynamic_closure_call` consumes the group in TWO instructions with
//     an allocating step between them. The receiver and callee feed
//     `js_closure_unbox_callee_checked_rebind`, which CLONES a `this`-capturing
//     closure and therefore allocates; the arguments feed `js_closure_callN`
//     BELOW it. One re-read point can serve one of those two and must strand
//     the other (#7154's own reasoning, `RootedOperands::reread_one`).
//   * `lower_rest_call_args_rooted` re-reads element `i` between the pushes
//     that materialise the rest array — `js_array_alloc` plus one
//     `js_array_push_f64` per element, all of which allocate — so its re-read
//     points are a LOOP, not a point.
//   * `try_lower_func_ref_call` releases ~450 lines below the lowering, in the
//     merge block of four block-splitting specialized-ABI dispatch diamonds. A
//     closure form can express that only by swallowing the dispatch chain.
//
// So the missing combinator is not "the variadic/rest shape" (slice 5's
// hypothesis, and the shape that made it visible) but the thing all three want:
// ONE temp-root scope whose contents may be re-read at ANY number of
// caller-chosen points. The rest/variadic case then falls out as a group that
// happens to hold an accumulator array as well as operands, which is why
// [`RootedGroup`] carries both rather than there being a second type for it.
//
// TWO ENTRY POINTS, AND THE ASYMMETRY IS DELIBERATE.
//
// [`with_rooted_group`] owns the release, like every other combinator here.
// [`open_rooted_group`] hands the group back, which every other combinator in
// this file deliberately refuses to do — so it needs an argument.
//
// The argument is that the two halves of a mis-managed guard are not equally
// dangerous. A release that is EARLY or MIS-ORDERED is a use-after-free: the
// slot is cut while the consumer still reads it. A release that never happens
// is over-retention — the slot stays bound for the rest of the function, the
// object stays live, and the emitted code is merely conservative (in the FFI
// fallback the runtime stack also grows, which is #7462's symptom and a real
// bug, but still not a dangling pointer).
//
// [`RootedGroup`] removes the dangerous half BY CONSTRUCTION and for both
// entry points: it is not `Clone`, `release` consumes it, and there is no way
// to obtain the slot index — so a caller cannot truncate at the wrong slot, in
// the wrong order, or twice. What escaping leaves writable is exactly the safe
// half. That is strictly better than the raw API it replaces, where the caller
// holds an `Option<String>` slot index it can truncate anywhere.
//
// Prefer [`with_rooted_group`]. Reach for [`open_rooted_group`] only where the
// release must post-dominate blocks the lowering does not lexically contain.
// ---------------------------------------------------------------------------

/// One temp-root scope: an ordered stack of rooted values — already-lowered
/// **operands** and mutable **accumulator arrays** — re-readable at any number
/// of caller-chosen points and released once, for the whole stack.
///
/// Two things it does NOT do, both on purpose:
///
///  * it never hands out a slot index, so the release cannot be mis-ordered.
///    `temp_root_truncate` is a stack CUT — truncating the wrong slot drops
///    every slot above it, which is how a receiver save becomes the number `0`
///    (`func_ref.rs`'s note on release ordering);
///  * it never lowers an operand it was not asked to. [`RootedGroup::lower`]
///    lowers, [`RootedGroup::adopt`] takes a value the caller emitted itself —
///    which the generic dynamic call needs, because its callee operand is a
///    hand-emitted by-name property read rather than `lower_expr(callee)`.
pub(crate) struct RootedGroup<'a> {
    operands: temp_root::RootedOperands,
    exprs: Vec<&'a Expr>,
    accs: Vec<String>,
    emitted: Vec<EmittedRoot>,
    /// The LOWEST slot this group pushed, of either kind. One truncate at it
    /// drops the whole scope, because a truncate is a stack cut.
    first_slot: Option<String>,
}

/// What [`RootedGroup::adopt_emitted`] recorded for one emitted value.
///
/// The `Reused` arm is the `protect == false` answer, and it exists for the
/// same reason [`RootedAcc`]'s `value` field does: a site whose window
/// provably cannot collect must keep the IR it had before it was rooted at
/// all, register numbering included. It is NOT a third protection strategy —
/// `operand_protection`'s `Reload` is still unavailable here (re-emitting the
/// producing call would call it twice) and `Reuse`-across-a-real-window is
/// still the bug. It only records that there was no window.
enum EmittedRoot {
    Rooted(RootedSlot),
    Reused(String),
}

/// A handle on one **emitted** value inside a [`RootedGroup`] —
/// see [`RootedGroup::adopt_emitted`].
///
/// Opaque and `Copy`, for the same reason [`AccArray`] is: it is not a slot
/// index, so it cannot be truncated, mis-ordered or released. The same
/// not-branded-per-group caveat applies.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct EmittedValue(usize);

/// A handle on one accumulator array inside a [`RootedGroup`].
///
/// Opaque and `Copy`. What it buys is the half that matters: it is not a slot
/// index, so it cannot be truncated, mis-ordered or released — only handed back
/// to the group, which owns every emission that touches the slot.
///
/// It is **not branded per group**. It is a bare index into the group's own
/// list, so passing a handle to a *different* group selects that group's
/// accumulator at the same position (or panics if it has fewer). Pass a handle
/// only to the group that returned it. Branding it would need a group identity
/// this file has nowhere to get without global state, and the mistake is not
/// one any caller is positioned to make: a group is always a local, and the two
/// entry points hand it out one at a time.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct AccArray(usize);

impl<'a> RootedGroup<'a> {
    fn new(capacity: usize) -> Self {
        RootedGroup {
            operands: temp_root::root_operands_begin(capacity),
            exprs: Vec::with_capacity(capacity),
            accs: Vec::new(),
            emitted: Vec::new(),
            first_slot: None,
        }
    }

    /// Record the lowest slot the group holds. Slots are handed out in
    /// increasing watermark order, so the first one recorded is the lowest and
    /// later ones must not displace it.
    fn note_slot(&mut self, slot: Option<String>) {
        if self.first_slot.is_none() {
            self.first_slot = slot;
        }
    }

    /// Lower `expr` and root it across a window the caller states.
    ///
    /// Returns the operand's index for [`RootedGroup::reread`] — and returns
    /// *only* that. The lowered register is deliberately not handed back: a
    /// caller holding it is the second half of every bug in this family, and
    /// the group is now the only way to name the value.
    pub(crate) fn lower(
        &mut self,
        ctx: &mut FnCtx<'_>,
        expr: &'a Expr,
        collects: bool,
    ) -> Result<usize> {
        let value = crate::expr::lower_expr(ctx, expr)?;
        Ok(self.adopt(ctx, expr, &value, collects))
    }

    /// Root a value the caller emitted itself.
    ///
    /// `expr` still decides the protection, so the group answers "root,
    /// re-derive, or reuse?" through `operand_protection` exactly as a lowered
    /// operand does.
    ///
    /// **Precondition.** `value` must be what lowering `expr` produces, or
    /// `expr` must not be `operand_is_reloadable` — because a `Reload` operand
    /// is re-read by *re-lowering* `expr`, and re-lowering something the caller
    /// did not lower would answer with a different value. Only `Expr::String`
    /// is reloadable, so every current caller (a `PropertyGet` callee, a
    /// receiver) satisfies this trivially; the note is here for the next one.
    pub(crate) fn adopt(
        &mut self,
        ctx: &mut FnCtx<'_>,
        expr: &'a Expr,
        value: &str,
        collects: bool,
    ) -> usize {
        self.operands.push(ctx, expr, value, collects);
        let pushed = self.operands.guard();
        self.note_slot(pushed);
        self.exprs.push(expr);
        self.exprs.len() - 1
    }

    /// Re-read operand `i` **here**, below whatever has collected since it was
    /// rooted.
    ///
    /// Mandatory rather than defensive, and it is the reason this type exists:
    /// the slot is a MUTABLE root that an evacuating cycle rewrites in place,
    /// so a register read before the cycle names from-space.
    pub(crate) fn reread(&self, ctx: &mut FnCtx<'_>, i: usize) -> Result<String> {
        self.operands.reread_one(ctx, &self.exprs, i)
    }

    /// Re-read every operand at this point, in order.
    pub(crate) fn reread_all(&self, ctx: &mut FnCtx<'_>) -> Result<Vec<String>> {
        self.operands.reread(ctx, &self.exprs)
    }

    /// How many operands the group holds.
    pub(crate) fn len(&self) -> usize {
        self.exprs.len()
    }

    /// True when this group actually pushed a slot.
    ///
    /// The signal a caller uses to keep an eager unbox — and therefore its
    /// exact register numbering — on the unprotected path, exactly as
    /// `RootedOperands::is_rooted` served `math_simple.rs`'s `MapSet` before
    /// the migration. It reports whether a slot exists, never which one, so it
    /// cannot be turned into a release.
    pub(crate) fn is_rooted(&self) -> bool {
        self.first_slot.is_some()
    }

    /// Root a value that an **emitted step** produced, rather than one lowered
    /// from an expression.
    ///
    /// # Why this exists, and why it took until slice 7
    ///
    /// This file deleted a `root_i64(ctx, reg) -> RootedSlot` combinator unused
    /// and recorded the terms on which a replacement could return: "it should
    /// arrive with its caller and with a written argument for why
    /// [`call_rooted`] cannot serve". Slice 7 found two callers, and they are
    /// the same shape:
    ///
    ///  * `expr/proxy_reflect.rs` — `process.env[k] = v` must coerce the key
    ///    (`js_to_property_key`, which runs a user `Symbol.toPrimitive`) ABOVE
    ///    the value's evaluation, because ES2022 moved `ToPropertyKey` before
    ///    the RHS. The **coerced** key is what has to survive that evaluation,
    ///    and it is a fresh heap string with no other root;
    ///  * `expr/fs_await.rs` — the await loop polls the promise that
    ///    `js_assimilate_thenable` + `js_await_any_promise` produced, which for
    ///    a thenable is a **wrapper** the assimilation allocated, not the
    ///    operand.
    ///
    /// **Why [`call_rooted`] cannot serve.** It fuses the root store to a call
    /// it emits itself and hardcodes [`Repr::Ptr`], so it can only root the
    /// direct `i64` result of one call. Neither value is that: the property key
    /// is a `double` whose raw pointer must be taken BELOW the window, and the
    /// promise is used boxed (`js_value_is_promise`) and unboxed
    /// (`js_promise_state`) in six different basic blocks.
    ///
    /// **Why there is no protection decision to make.** Every other entry point
    /// asks `operand_protection`; this value has no `Expr` to ask about, and
    /// two of the three answers are unavailable on principle rather than by
    /// choice. `Reload` cannot re-derive it — re-emitting the producing call
    /// would call it a *second* time, and both producers are observable. And
    /// `Reuse` is the bug. So the answer is always `Root`, there is no flag,
    /// and a caller cannot pick the wrong one.
    ///
    /// **What it does weaken, stated plainly.** `value` is a register the
    /// caller produced, so "produce it, let something collect, THEN root it" —
    /// #7192 — is writable here, exactly as it is in
    /// [`with_rooted_accumulator`], which has taken a caller-produced `initial`
    /// since slice 3. Call this on the line below the emission that produced
    /// the value.
    ///
    /// # `protect` states the WINDOW, not the strategy (slice 8)
    ///
    /// Slice 7 shipped this without a flag and said so: "there is no flag, and
    /// a caller cannot pick the wrong one". That claim was about the
    /// *strategy* — `Reload` and `Reuse` are unavailable for an emitted value
    /// on principle — and it still holds. `protect` answers the other
    /// question, the one every combinator in this file already takes from its
    /// caller in some form: **does anything between here and the last use
    /// collect?** [`with_rooted_accumulator`] has taken it as `protect` since
    /// slice 3, `RootedGroup::lower`/`adopt` take it as `collects`, and
    /// `with_operands_rooted_across_call` hardcodes it to `true`.
    ///
    /// Slice 8 needed it for `expr/static_field_meta.rs`: a `ClassExprFresh`
    /// with no statics, no captures, no symbol statics and no `static { … }`
    /// block emits nothing at all between the class object's allocation and
    /// the `nanbox_pointer_inline` that returns it, and that shape is
    /// reachable (`lower_decl/body_stmt.rs`'s `fresh_binding` arm builds one
    /// with three empty vectors). `protect == false` emits no push, no
    /// re-reads and no truncate, so it keeps the pre-rooting IR byte for byte
    /// — the same contract `rooted_handle_begin(ctx, h, false)` had.
    pub(crate) fn adopt_emitted(
        &mut self,
        ctx: &mut FnCtx<'_>,
        repr: Repr,
        value: &str,
        protect: bool,
    ) -> EmittedValue {
        let root = if protect {
            let idx = match repr {
                Repr::Ptr => temp_root::temp_root_push_i64(ctx, value),
                Repr::Boxed => temp_root::temp_root_push_double(ctx, value),
            };
            self.note_slot(Some(idx.clone()));
            EmittedRoot::Rooted(RootedSlot { idx, repr })
        } else {
            EmittedRoot::Reused(value.to_string())
        };
        self.emitted.push(root);
        EmittedValue(self.emitted.len() - 1)
    }

    /// Re-read an [`adopt_emitted`](RootedGroup::adopt_emitted) value **here**,
    /// in the representation it was pushed with.
    ///
    /// An unprotected value hands its original register back and emits
    /// nothing, exactly as `RootedOperands::reread_one`'s `Reuse` arm does.
    pub(crate) fn reread_emitted(&self, ctx: &mut FnCtx<'_>, value: EmittedValue) -> String {
        match &self.emitted[value.0] {
            EmittedRoot::Rooted(slot) => read_slot(ctx, slot),
            EmittedRoot::Reused(reg) => reg.clone(),
        }
    }

    /// Allocate an argument-accumulator array of capacity `cap` and root it in
    /// this scope.
    ///
    /// This is the variadic / spread / rest shape: `js_array_alloc(n)`, then one
    /// push per argument. The accumulator holds the ONLY reference to everything
    /// pushed so far while the next argument is lowered, and every push
    /// allocates — so it is an accumulator in exactly [`RootedAcc`]'s sense, and
    /// it lives in the group so that ONE release drops the operands and the
    /// arrays together.
    pub(crate) fn begin_array(&mut self, ctx: &mut FnCtx<'_>, cap: &str) -> AccArray {
        let slot = temp_root::rooted_array_begin(ctx, cap);
        self.note_slot(Some(slot.clone()));
        self.accs.push(slot);
        AccArray(self.accs.len() - 1)
    }

    /// Root an array this group did not allocate — the inline-constructed
    /// rest bundle — so one release still drops operands and arrays together.
    pub(crate) fn adopt_array(&mut self, ctx: &mut FnCtx<'_>, arr: &str) -> AccArray {
        let slot = temp_root::rooted_array_adopt(ctx, arr);
        self.note_slot(Some(slot.clone()));
        self.accs.push(slot);
        AccArray(self.accs.len() - 1)
    }

    /// Push one element, re-reading the array from its slot and publishing the
    /// possibly-reallocated pointer back into it.
    pub(crate) fn push_array(&mut self, ctx: &mut FnCtx<'_>, acc: AccArray, value: &str) {
        let slot = self.accs[acc.0].clone();
        temp_root::temp_rooted_array_push(ctx, &slot, value);
    }

    /// Re-read the finished array as a raw `i64` pointer. Does not release: the
    /// consuming call allocates while it reads the array.
    pub(crate) fn read_array(&self, ctx: &mut FnCtx<'_>, acc: AccArray) -> String {
        let slot = self.accs[acc.0].clone();
        temp_root::rooted_array_read(ctx, &slot)
    }

    /// Drop the whole scope. Call it *after* the consuming call: the consumer
    /// allocates while reading these values.
    pub(crate) fn release(self, ctx: &mut FnCtx<'_>) {
        temp_root::temp_root_release(ctx, self.first_slot);
    }
}

/// Open a [`RootedGroup`] for the duration of `body` and release it on every
/// path out, including `body`'s `?`.
pub(crate) fn with_rooted_group<'f, 'a, R>(
    ctx: &mut FnCtx<'f>,
    capacity: usize,
    body: impl FnOnce(&mut FnCtx<'f>, &mut RootedGroup<'a>) -> Result<R>,
) -> Result<R> {
    let mut group = RootedGroup::new(capacity);
    let out = body(ctx, &mut group);
    group.release(ctx);
    out
}

/// Open a [`RootedGroup`] whose release the CALLER performs, because it must
/// post-dominate blocks this lowering does not lexically contain.
///
/// One shape needs this and it is named rather than left general:
/// `func_ref.rs`'s direct call lowers its arguments, then dispatches through up
/// to four specialized-ABI diamonds that split the block, and the release has
/// to sit in the merge. Releasing inside either side of a diamond would leave
/// the other side's call reading dropped slots.
///
/// `#[must_use]` because dropping the group silently is the one mistake left
/// writable — see the block comment above for why that mistake is
/// over-retention rather than a dangling pointer, and why the dangerous half
/// (an early or mis-ordered truncate) is not writable at all.
#[must_use = "a RootedGroup must be released with `release`, below the call that reads it"]
pub(crate) fn open_rooted_group<'a>(capacity: usize) -> RootedGroup<'a> {
    RootedGroup::new(capacity)
}

/// A saved implicit `this`, held in a rooted slot for the duration of a
/// dispatch (#7211).
///
/// **Moved here in slice 6 rather than re-exported.** It was already a paired
/// combinator with this file's contract — root before the window, re-read
/// after it, never hand out the register — but it lived in the raw API, so six
/// `lower_call/` modules had to name `expr::temp_root` while making no ordering
/// decision at all. Leaving a copy behind would have given the pair two
/// spellings, which is the drift that produced #7114; there is one.
///
/// `js_implicit_this_set` swaps the `IMPLICIT_THIS` cell and returns what was
/// there, read straight out of a cell `scan_implicit_this_roots_mut`
/// (`object/this_binding.rs:176`) registers as a scanned MUTABLE root. The swap
/// has already overwritten the cell, so the returned value is now held ONLY in
/// an SSA register, across the whole call the bind exists to scope.
///
/// Two ways that hurts, and the second is what makes it worse than an ordinary
/// stale read:
///
///  * the enclosing frame still roots the same object, so an evacuating minor
///    inside the callee MOVES it and rewrites that root — leaving this register
///    naming from-space. The restore then publishes a pre-move address back
///    INTO a root the collector scans, so the corruption outlives the call that
///    caused it and surfaces in whatever reads `this` next;
///  * where no other root holds it, the object is simply collected.
///
/// Seven lowerings emit this pair. They had seven copies of the same three
/// lines and therefore seven copies of the same bug, which is why it is a
/// combinator rather than seven edits.
pub(crate) struct ImplicitThisSave {
    slot: RootedSlot,
}

/// Bind `new_this` as the implicit `this` and root the value it displaced.
///
/// Unconditional, unlike an operand group: the window is a user or native call,
/// so `operand_protection`'s "can this window collect?" test has exactly one
/// answer here and there is nothing to gate on.
pub(crate) fn implicit_this_save(ctx: &mut FnCtx<'_>, new_this: &str) -> ImplicitThisSave {
    let prev = implicit_this_swap(ctx, new_this, "implicit_this.save");
    let idx = temp_root::temp_root_push_double(ctx, &prev);
    ImplicitThisSave {
        slot: RootedSlot {
            idx,
            repr: Repr::Boxed,
        },
    }
}

/// Restore the saved implicit `this`, re-read from its root.
///
/// Reading the slot rather than the register is the fix, not a precaution: the
/// slot is a mutable root, so an evacuating cycle inside the dispatch rewrote
/// it and the register pushed beforehand names from-space.
///
/// The release is emitted BEFORE the restore call so that nested saves — an
/// override arm inside an outer bind — release inner to outer. A release is a
/// stack cut, so a caller holding a LOWER group may release it afterwards and
/// drop this slot a second time harmlessly.
pub(crate) fn implicit_this_restore(ctx: &mut FnCtx<'_>, save: ImplicitThisSave) {
    let prev = read_slot(ctx, &save.slot);
    save.slot.release(ctx);
    implicit_this_swap(ctx, &prev, "implicit_this.restore");
}

/// `js_implicit_this_set(value)`: bind `value` as the implicit `this` and
/// return the previous binding. On Apple aarch64 the cell is read and
/// written inline through the hot-cache lookup (`expr::hot_tls`), with the
/// runtime call as the fallback for every miss — the pair around a
/// dynamically-dispatched call was two runtime calls whose whole body was
/// that lookup plus a `replace`.
fn implicit_this_swap(ctx: &mut FnCtx<'_>, value: &str, stem: &str) -> String {
    if !crate::expr::hot_tls::inline_hot_tls_enabled(ctx) {
        return ctx
            .block()
            .call(DOUBLE, "js_implicit_this_set", &[(DOUBLE, value)]);
    }
    let lookup = crate::expr::hot_tls::emit_hot_tls_lookup(ctx, stem);
    let merge_idx = ctx.new_block(&format!("{stem}.hot_tls.merge"));
    let merge_label = ctx.block_label(merge_idx);
    let cell = crate::expr::hot_tls::hot_tls_field(
        ctx,
        &lookup.hot,
        crate::expr::hot_tls::HOT_TLS_IMPLICIT_THIS_OFFSET,
    );
    let (fast_prev, fast_pred) = {
        let blk = ctx.block();
        let prev_bits = blk.load(I64, &cell);
        let value_bits = blk.bitcast_double_to_i64(value);
        blk.store(I64, &value_bits, &cell);
        let prev = blk.bitcast_i64_to_double(&prev_bits);
        let pred = blk.label.clone();
        blk.br(&merge_label);
        (prev, pred)
    };
    ctx.current_block = lookup.slow_idx;
    let (slow_prev, slow_pred) = {
        let blk = ctx.block();
        let prev = blk.call(DOUBLE, "js_implicit_this_set", &[(DOUBLE, value)]);
        let pred = blk.label.clone();
        blk.br(&merge_label);
        (prev, pred)
    };
    ctx.current_block = merge_idx;
    ctx.block().phi(
        DOUBLE,
        &[(&fast_prev, &fast_pred), (&slow_prev, &slow_pred)],
    )
}

/// The `new.target` cell's saved previous value (#7664).
///
/// Structurally [`ImplicitThisSave`] for a different cell, and it is a separate
/// type rather than a parameter so the two cannot be crossed at a restore.
///
/// The cell is a registered mutable root — `scan_current_new_target_root_mut`,
/// `gc/mod.rs` — so an evacuating cycle inside the constructor rewrites it and
/// a register saved beforehand names from-space. The RUNTIME's own construct
/// paths have always rooted their `prev_new_target`
/// (`object/class_registry/construct.rs`, `scope.root_nanbox_f64`); the
/// generated `new` path saved it into a bare SSA register across the whole
/// constructor body, which is #7226's `prev_this` bug for `new.target`.
///
/// Re-reading the cell instead of rooting it would be the wrong repair, and for
/// the reason `operand_is_reloadable` states: `js_new_target_set` has already
/// overwritten it with THIS class's ref, so a re-read returns the new value,
/// not the saved one. Only a root gives both a rewritten location and the value
/// the save observed.
pub(crate) struct NewTargetSave {
    slot: RootedSlot,
}

/// Set `new.target` to `new_target` and root the value it displaced.
pub(crate) fn new_target_save(ctx: &mut FnCtx<'_>, new_target: &str) -> NewTargetSave {
    let prev = ctx.block().call(DOUBLE, "js_new_target_get", &[]);
    let idx = temp_root::temp_root_push_double(ctx, &prev);
    ctx.block()
        .call(DOUBLE, "js_new_target_set", &[(DOUBLE, new_target)]);
    NewTargetSave {
        slot: RootedSlot {
            idx,
            repr: Repr::Boxed,
        },
    }
}

/// Restore the saved `new.target`, re-read from its root.
///
/// Takes the save by REFERENCE, and does not release — which is the difference
/// from [`implicit_this_restore`] and is forced by the caller. `new.rs` emits
/// this restore on several exits from one save, and its slot is cut by the
/// enclosing expression scope (`temp_root_scope_begin`/`temp_root_scope_end`,
/// which that module already opens precisely because its ~20 return paths make
/// per-path releases the thing that gets missed, #6969). A release here would
/// be a stack cut on one of those paths only.
pub(crate) fn new_target_restore(ctx: &mut FnCtx<'_>, save: &NewTargetSave) {
    let prev = read_slot(ctx, &save.slot);
    ctx.block()
        .call(DOUBLE, "js_new_target_set", &[(DOUBLE, &prev)]);
}

/// A GC-managed value that generated code keeps **updating** while it lowers
/// further expressions: an object literal's half-built handle, `Object.assign`'s
/// threaded target, `Math.min(...)`'s growing argument array.
///
/// It is the operand group's mirror image. An operand is lowered once and read
/// once; an accumulator is written, read, rewritten and read again, with
/// arbitrary user code lowered between the writes. #7154's `ObjectSpread` bug is
/// the canonical failure: the half-built object sat in a raw SSA register while
/// 269 spread values were lowered, an evacuating minor relocated it, and every
/// later field store wrote into abandoned from-space memory — silently, because
/// the fields simply did not appear on the copy the program kept.
///
/// The invariant it enforces is the one a raw handle cannot: **the accumulator
/// never exists as a register the lowering holds across an emission.** Every
/// consuming call re-reads it as part of being emitted ([`RootedAcc::call`],
/// [`RootedAcc::call_void`]), and a helper that returns a fresh address
/// publishes it straight back into the slot ([`RootedAcc::advance`]) rather than
/// handing it out. The single point where a register does escape is the final
/// read, and [`with_rooted_accumulator`]'s `finish` closure owns it: it runs
/// below the last collection point and above the release, so there is no
/// program in which the escaped register outlives its root.
pub(crate) struct RootedAcc {
    slot: Option<RootedSlot>,
    repr: Repr,
    /// The register as first produced. The answer when `protect` was false, in
    /// which case nothing is emitted and the IR matches the un-rooted form byte
    /// for byte.
    value: String,
}

impl RootedAcc {
    /// The accumulator as a call argument.
    ///
    /// `pub(crate)` for the one shape that needs it: a call whose argument 0 is
    /// one accumulator and whose later argument is *another* (an object-literal
    /// method closure being installed into the half-built object it belongs to,
    /// #8809). It hands out an [`Arg`], never a register — `materialize` still
    /// performs the re-read at the instant the call is emitted — so the "load
    /// early, use late" sequence stays unwritable through this door too.
    pub(crate) fn as_arg(&self) -> Arg<'_> {
        match &self.slot {
            Some(slot) => Arg::Root(slot),
            None => Arg::Plain(self.repr.llvm_ty(), &self.value),
        }
    }

    fn args_with_self<'a>(&'a self, rest: &[Arg<'a>]) -> Vec<Arg<'a>> {
        let mut all = Vec::with_capacity(rest.len() + 1);
        all.push(self.as_arg());
        all.extend_from_slice(rest);
        all
    }

    /// Emit `callee(<accumulator>, ...rest)` and return its result register.
    ///
    /// The accumulator is argument **0**, positionally and deliberately. It is
    /// argument 0 at every site this exists for — `js_object_set_field_by_name`,
    /// `js_object_copy_own_fields`, `js_array_push_f64`, `js_object_assign_one`
    /// — and keeping it positional is what lets the re-read be fused to the
    /// emission instead of handed back to the caller as a register to place.
    pub(crate) fn call(
        &self,
        ctx: &mut FnCtx<'_>,
        ret_ty: LlvmType,
        callee: &str,
        rest: &[Arg<'_>],
    ) -> String {
        call_with_roots(ctx, ret_ty, callee, &self.args_with_self(rest))
    }

    /// [`RootedAcc::call`] for a `void` helper.
    pub(crate) fn call_void(&self, ctx: &mut FnCtx<'_>, callee: &str, rest: &[Arg<'_>]) {
        call_void_with_roots(ctx, callee, &self.args_with_self(rest));
    }

    /// Emit `callee(<accumulator>, ...rest)` and make its result the new
    /// accumulator value.
    ///
    /// For helpers that may **relocate** what they are handed and return the
    /// current address: `js_array_push_f64` reallocs the element storage,
    /// `js_object_assign_one` returns the post-collection target. Keeping the
    /// pre-call register instead is how `Object.assign(t, a, b)` threaded a
    /// stale `t` into `b`'s link before #7200.
    pub(crate) fn advance(&mut self, ctx: &mut FnCtx<'_>, callee: &str, rest: &[Arg<'_>]) {
        let next = self.call(ctx, self.repr.llvm_ty(), callee, rest);
        match (&self.slot, self.repr) {
            (Some(slot), Repr::Ptr) => temp_root::temp_root_set_i64(ctx, &slot.idx, &next),
            (Some(slot), Repr::Boxed) => temp_root::temp_root_set_double(ctx, &slot.idx, &next),
            (None, _) => self.value = next,
        }
    }
}

/// Root a mutable accumulator for the duration of `build`, then hand its final
/// value to `finish` and release it.
///
/// `protect == false` emits nothing at all — no push, no re-reads, no truncate —
/// so a site whose initializers provably cannot collect keeps the IR it had
/// before it was rooted at all.
///
/// The split into two closures is what makes the release unmissable while still
/// letting the final value be *used*. `build` may not hold the accumulator in a
/// register across anything; `finish` receives one, but it runs below the last
/// collection point and above the release, and it is the only place the value
/// escapes. Both paths — `build`'s `?` and `finish`'s — release.
pub(crate) fn with_rooted_accumulator<'f, R>(
    ctx: &mut FnCtx<'f>,
    repr: Repr,
    initial: &str,
    protect: bool,
    build: impl FnOnce(&mut FnCtx<'f>, &mut RootedAcc) -> Result<()>,
    finish: impl FnOnce(&mut FnCtx<'f>, &str) -> Result<R>,
) -> Result<R> {
    let slot = protect.then(|| {
        let idx = match repr {
            Repr::Ptr => temp_root::temp_root_push_i64(ctx, initial),
            Repr::Boxed => temp_root::temp_root_push_double(ctx, initial),
        };
        RootedSlot { idx, repr }
    });
    let mut acc = RootedAcc {
        slot,
        repr,
        value: initial.to_string(),
    };
    let out = (|| {
        build(ctx, &mut acc)?;
        let final_value = match &acc.slot {
            Some(slot) => read_slot(ctx, slot),
            None => acc.value.clone(),
        };
        finish(ctx, &final_value)
    })();
    if let Some(slot) = acc.slot {
        slot.release(ctx);
    }
    out
}
