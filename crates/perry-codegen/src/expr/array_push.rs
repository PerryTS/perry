//! ArrayPush / ArrayPushSpread.
//!
//! Extracted from `expr/mod.rs` to keep that file under the 2000-line cap.
//! Pure mechanical move — match arm bodies are verbatim copies, called from
//! `lower_expr`'s outer dispatch.
//!
//! # Layer 1 migrated module (#7615, slice 3)
//!
//! Nothing here names `expr::temp_root`, and nothing here did before the
//! migration either — so the ledger line is **vacuous on the committed source**
//! and only means something because the slice ran the sabotage arm (a real
//! `temp_root_push_i64` / `temp_root_truncate` pair injected here turns
//! `migrated_modules_do_not_reach_past_the_rooting_api` red). Slices 1a and 1b
//! carried the same caveat. `ArrayPushSpread`'s operand pair goes through
//! [`crate::rooting::with_operands_rooted`], which is a documentation change
//! rather than a repair: the group's window is empty, so it emits nothing.
//!
//! ## Two orders, and the as-if test that picks between them (#7634)
//!
//! The historical arms lower the **pushed value first and the receiver
//! second**, and the receiver is `Expr::LocalGet`, i.e. a load from the local's
//! alloca / box / module global. A load taken *after* the value's arbitrary
//! user code observes whatever an evacuating cycle wrote back into that
//! storage, so there is no stale register to repair and `operand_protection`
//! answers `Reuse`. Everything the arms emit below that point —
//! `js_array_push_f64`, `js_array_concat`, the header probes,
//! `js_gc_note_slot_layout`, `js_write_barrier_slot`, `js_array_length` —
//! either consumes the pointer it is handed or cannot re-enter user code, so no
//! value crosses a moving window.
//!
//! That safety was a **consequence of an evaluation order the spec does not
//! permit**: ES2024 evaluates the `MemberExpression` `a.push` to a Reference
//! before the argument list, so `a.push(f())` must push onto the array `a`
//! named *before* `f` ran. Perry pushed onto whatever `a` named afterwards.
//!
//! The repair is not a blanket reorder, because a blanket reorder makes the
//! receiver live across the argument on **every** push and buys an observable
//! difference on almost none of them. [`push_receiver_is_rebindable`] is the
//! as-if test: unless the argument can rebind the receiver's binding — it
//! assigns the id itself, or the binding is boxed (captured *and* mutated) or a
//! module global and the argument can reach a collection point — the two orders
//! name the same array and the historical lowering is kept, tiers, register
//! numbering and all. `an_unreachable_binding_keeps_the_historical_order_and_
//! roots_nothing` pins that.
//!
//! When the test does fire, the spec fix and the rooting fix are one change
//! (#7634's own framing) and [`lower_array_push_spec_order`] is both: the
//! receiver is an operand of `with_operands_rooted_across`, rooted before the
//! argument and re-read after it. That arm also drops the inline fast tiers on
//! purpose — they publish the reallocated head back into the binding
//! unconditionally, and once the argument may have rebound it that store lands
//! on the wrong array.

use anyhow::{anyhow, Result};
use perry_hir::Expr;

use crate::block::LlBlock;
use crate::nanbox::double_literal;
use crate::native_value::{
    BoundsState, BufferAccessMode, ExpectedNativeRep, LoweredValue, MaterializationReason,
    NativeRep, SemanticKind,
};
use crate::rooting;
use crate::type_analysis::is_numeric_expr;
use crate::types::{DOUBLE, I1, I16, I32, I64, I8};

use super::{
    array_store_needs_layout_note, array_store_needs_write_barrier,
    emit_array_numeric_write_note_on_block, emit_jsvalue_slot_store_with_flags_on_block,
    emit_jsvalue_slot_store_with_value_bits_on_block, emit_layout_note_slot_on_block,
    emit_may_carry_heap_pointer_check, emit_root_nanbox_store_on_block,
    emit_typed_feedback_register_site, emit_write_barrier,
    emit_write_barrier_slot_generation_tested, expr_has_numeric_pointer_free_array_layout,
    lower_expr, lower_expr_native, nanbox_pointer_inline, raw_f64_layout_fact, unbox_to_i64, FnCtx,
    TypedFeedbackContract, TypedFeedbackKind,
};

/// Metadata may admit the #7839 append tier because that tier tests the live
/// result bits before doing GC bookkeeping. A lying numeric operand makes JS
/// `+` produce a heap value; the pointer test then takes the note/addref/
/// barrier arm, so the annotation selects a checked lowering, not an answer.
fn guarded_numeric_add_push_candidate(ctx: &FnCtx<'_>, value: &Expr) -> bool {
    if is_numeric_expr(ctx, value) {
        return true;
    }
    match value {
        Expr::LocalGet(id) => matches!(
            ctx.local_type_hint(id),
            Some(perry_hir::types::Type::Number | perry_hir::types::Type::Int32)
        ),
        Expr::Binary {
            op: perry_hir::BinaryOp::Add,
            left,
            right,
        } => {
            guarded_numeric_add_push_candidate(ctx, left)
                && guarded_numeric_add_push_candidate(ctx, right)
        }
        _ => false,
    }
}

/// The expression's result: the new length per ES2024 `Array.prototype.push`.
///
/// `js_array_length` is NOT a field read — it resolves Proxy arrays through
/// the `get` trap and probes the registered-Set/Map side tables — and a
/// statement-position `arr.push(x);` discards its result, so on push-heavy
/// workloads it was 8–13% of the run computing a number nobody reads.
/// `value_discarded` is the `mem::take`n per-expression signal from
/// `dispatch::lower_expr` (#7590: it reaches exactly the statement's own
/// expression, never an operand — a consumed `n = arr.push(x)` always
/// computes the real length). When set, the placeholder constant is returned
/// without emitting the call.
/// The `nofwd` admission test for a #7839 numeric push: the historical
/// integrity mask `0x0407` PLUS the three `_reserved` states in which
/// `js_gc_note_slot_layout` does real work for a **non-pointer** value stored
/// into a `GC_TYPE_ARRAY`. Every other state that function can be in is a
/// provable no-op for such a value (see
/// [`emit_numeric_push_store_pointer_tested`]).
///
/// * `0x0407` `FROZEN|SEALED|NO_EXTEND|ARRAY_DESCRIPTORS` — the historical
///   integrity bits, unchanged in meaning and in destination.
/// * `0x0800` `GC_ARRAY_ELEMENT_SHAPE` — a live element-shape proof (#7480).
///   `note_element_store` must CLEAR it when a non-object lands in the array,
///   and that call sits ahead of every early return in `layout_note_slot`.
/// * `0x1000` — `GC_OBJ_TYPED_LAYOUT_INTACT` as `layout_note_slot` reads it
///   (`GC_ARRAY_RAW_F64_HOLES` as `gc::types` writes it for an array; the two
///   share the bit and are disjoint by `obj_type`). Set, it routes into the
///   typed-descriptor probe, whose `slot_index >= slot_count` arm downgrades.
/// * `0x2000` `GC_LAYOUT_ALL_POINTERS` — a non-pointer store into an
///   all-pointer array calls `layout_mark_unknown`, which is a real state
///   change, not a no-op.
///
/// `GC_LAYOUT_SIDE_MASK` is deliberately absent. Skipping the note there leaves
/// a stale set bit over a non-pointer, and `mark_field_into_worklist`
/// re-validates every slot word, so the cost is one rejected visit and never a
/// stranded child — the identical argument `class_field_store_needs_layout_note`
/// already ships.
///
/// Failing this test costs the push its inline store: it takes
/// `js_array_push_f64`, which notes the slot exactly as it always did. So a
/// widening here can only ever be slower, never wrong — the same direction of
/// approximation `emit_may_carry_heap_pointer_check` documents.
///
/// `0x0407 | 0x3800` == `0x3C07` == 15367.
const ARRAY_PUSH_NUMERIC_CLEAN_I16: &str = "15367";

/// Header admission mask for the dynamic pointer-append bookkeeping fast arm.
///
/// A generic value cannot use #7469's static all-pointer-array tier, but its
/// live bits can still prove `POINTER_TAG` at the store. When the receiver also
/// carries `SIDE_MASK | ALL_POINTERS`, with both raw-f64 flags and the optional
/// homogeneous element-shape proof clear, the three generic calls are dead:
///
/// * a `POINTER_TAG` value is not a heap string, so no string addref is needed;
/// * appending it preserves the all-pointer GC layout;
/// * both raw-f64 flags are already clear, so the numeric-layout note is a
///   no-op.
///
/// `0xF880` is `LAYOUT_STATE_MASK | ALL_POINTERS | RAW_F64_HOLES |
/// ELEMENT_SHAPE | RAW_F64_LAYOUT`; the admitted value is exactly
/// `SIDE_MASK | ALL_POINTERS` (`0xA000`). Integrity and prototype cleanliness
/// are checked by the enclosing `apush.nofwd` block as before.
const ARRAY_PUSH_POINTER_LAYOUT_MASK_I16: &str = "63616";
const ARRAY_PUSH_POINTER_LAYOUT_EXPECT_I16: &str = "40960";

/// Store a dynamically typed append value and bypass redundant bookkeeping
/// when the value and receiver's live header jointly prove the pointer-only
/// case. The write barrier is intentionally not handled here: an all-pointer
/// layout says which slots the collector scans, not that an old parent cannot
/// receive a young child, so the caller retains its generation-tested barrier.
#[allow(clippy::too_many_arguments)]
fn emit_dynamic_pointer_push_store(
    ctx: &mut FnCtx<'_>,
    arr_handle: &str,
    value_double: &str,
    value_bits_override: Option<&str>,
    object_flags: &str,
    string_addref_needed: bool,
    layout_note_needed: bool,
) -> (String, String, String) {
    let (length, element_addr, value_bits) = {
        let blk = ctx.block();
        let length = blk.safe_load_i32_from_ptr(arr_handle);
        let length_i64 = blk.zext(I32, &length, I64);
        let byte_offset = blk.shl(I64, &length_i64, "3");
        let with_header = blk.add(I64, &byte_offset, "8");
        let element_addr = blk.add(I64, arr_handle, &with_header);
        let element_ptr = blk.inttoptr(I64, &element_addr);
        // GC_STORE_AUDIT(BARRIERED): the common store remains unconditional;
        // only proven no-op bookkeeping is bypassed below, and the caller
        // still emits the generation-tested write barrier.
        blk.store(DOUBLE, value_double, &element_ptr);
        let value_bits = value_bits_override
            .map(ToOwned::to_owned)
            .unwrap_or_else(|| blk.bitcast_double_to_i64(value_double));
        (length, element_addr, value_bits)
    };

    let bookkeeping_idx = ctx.new_block("apush.pointer_layout.bookkeeping");
    let done_idx = ctx.new_block("apush.pointer_layout.done");
    let bookkeeping_label = ctx.block_label(bookkeeping_idx);
    let done_label = ctx.block_label(done_idx);
    {
        let blk = ctx.block();
        let top16 = blk.lshr(I64, &value_bits, "48");
        let is_object_pointer = blk.icmp_eq(I64, &top16, crate::nanbox::POINTER_TAG_TOP16_I64);
        let proof_bits = blk.and(I16, object_flags, ARRAY_PUSH_POINTER_LAYOUT_MASK_I16);
        let pointer_layout = blk.icmp_eq(I16, &proof_bits, ARRAY_PUSH_POINTER_LAYOUT_EXPECT_I16);
        let fast = blk.and(I1, &is_object_pointer, &pointer_layout);
        blk.cond_br(&fast, &done_label, &bookkeeping_label);
    }

    ctx.current_block = bookkeeping_idx;
    {
        let blk = ctx.block();
        if string_addref_needed {
            blk.call_void("js_string_addref_if_heap_string", &[(DOUBLE, value_double)]);
        }
        if layout_note_needed {
            emit_layout_note_slot_on_block(blk, arr_handle, &length, &value_bits);
        }
        // This helper is selected only for a value whose static construction
        // cannot prove numeric. The generic path therefore carried this note
        // before, and still does whenever the joint live proof fails.
        emit_array_numeric_write_note_on_block(blk, arr_handle, &value_bits);
        blk.br(&done_label);
    }
    ctx.current_block = done_idx;
    (length, element_addr, value_bits)
}

/// #7839 — the inline array append's GC bookkeeping behind ONE live test.
///
/// The `apush.inbounds` store used to pay `js_string_addref_if_heap_string` +
/// `js_gc_note_slot_layout` unconditionally and then an `ldar` on
/// `PERRY_INCREMENTAL_MARK_BARRIER_ACTIVE_COUNT` for the barrier gate — three
/// cross-crate obligations on *every* element of a `number[]` push loop, where
/// all three are dead. `bench/push_num.ts` is 20M such pushes.
///
/// The static proof that would retire them (`array_store_needs_layout_note` →
/// `expr_produces_non_pointer_bits_by_construction`) cannot be made for the
/// shape that matters: `keep.push(base + j)` is an `Expr::Binary { Add }`, and
/// that arm answers `false` unconditionally because `+` is string concatenation
/// for non-numeric operands. It fires only for a bare canonical-i32 local
/// (`keep.push(j)`), which is why the same loop is ~1.7x faster written that
/// way. This is #7511's answer to the identical problem on class fields: ask
/// the question ONCE inline, on the live bits, and branch over all three.
///
/// Why each obligation is dead when the test says no:
///
/// * `js_string_addref_if_heap_string` is tag-checked and a no-op for every
///   non-`STRING_TAG` value — `emit_may_carry_heap_pointer_check` admits
///   `STRING_TAG`, so a string always takes the guarded arm.
/// * `js_write_barrier_slot` opens with `barrier_child_prologue`, which returns
///   immediately when `decode_heap_addr(child) == 0`. The predicate is a
///   superset of every address that decoder resolves.
/// * `js_gc_note_slot_layout` for a non-pointer value is a no-op in every
///   layout state EXCEPT three, and reaching this block already PROVES all
///   three clear: [`ARRAY_PUSH_NUMERIC_CLEAN_I16`] widens the `nofwd`
///   integrity mask to cover them, so the array's half of the proof costs a
///   wider constant on an `and` that was being emitted anyway, and this block
///   has only the value left to test.
///
/// Gated on `value_is_numeric` at the call site, so a pointer-pushing loop
/// (`churn`, `tree`, `push_cls`) emits byte-identical IR to before rather than
/// paying the predicate for a test it always fails. That also keeps
/// `js_array_note_numeric_write` out of the picture: it is already statically
/// elided for exactly this class of value.
#[allow(clippy::too_many_arguments)]
fn emit_numeric_push_store_pointer_tested(
    ctx: &mut FnCtx<'_>,
    arr_handle: &str,
    value_double: &str,
    value_bits_override: Option<&str>,
    string_addref_needed: bool,
    layout_note_needed: bool,
    write_barrier_needed: bool,
) -> (String, String, Option<String>) {
    let (length, element_addr, value_bits) = {
        let blk = ctx.block();
        let length = blk.safe_load_i32_from_ptr(arr_handle);
        let length_i64 = blk.zext(I32, &length, I64);
        let byte_offset = blk.shl(I64, &length_i64, "3");
        let with_header = blk.add(I64, &byte_offset, "8");
        let element_addr = blk.add(I64, arr_handle, &with_header);
        let element_ptr = blk.inttoptr(I64, &element_addr);
        // GC_STORE_AUDIT(BARRIERED): the slot write itself is unconditional;
        // only the bookkeeping moves behind the live test below, and the
        // barrier's own first test is a subset of that predicate.
        blk.store(DOUBLE, value_double, &element_ptr);
        let value_bits = value_bits_override
            .map(ToOwned::to_owned)
            .unwrap_or_else(|| blk.bitcast_double_to_i64(value_double));
        (length, element_addr, value_bits)
    };
    let bookkeeping_idx = ctx.new_block("apush.gc_bookkeeping");
    let done_idx = ctx.new_block("apush.gc_bookkeeping.done");
    let bookkeeping_label = ctx.block_label(bookkeeping_idx);
    let done_label = ctx.block_label(done_idx);
    {
        let blk = ctx.block();
        let may_carry_pointer = emit_may_carry_heap_pointer_check(blk, &value_bits);
        blk.cond_br(&may_carry_pointer, &bookkeeping_label, &done_label);
    }
    ctx.current_block = bookkeeping_idx;
    {
        let blk = ctx.block();
        if string_addref_needed {
            blk.call_void("js_string_addref_if_heap_string", &[(DOUBLE, value_double)]);
        }
        if layout_note_needed {
            emit_layout_note_slot_on_block(blk, arr_handle, &length, &value_bits);
        }
    }
    if write_barrier_needed {
        // `arr_handle` reached here through the `nofwd` header test, so it is a
        // live, non-forwarded GC array user pointer — the precondition for
        // reading its header byte. The generation test stays: this arm is
        // reached for real pointer children too.
        emit_write_barrier_slot_generation_tested(
            ctx,
            arr_handle,
            arr_handle,
            &element_addr,
            &value_bits,
            "apush",
        );
    }
    ctx.block().br(&done_label);
    ctx.current_block = done_idx;
    (length, element_addr, None)
}

fn emit_array_handle_length(
    ctx: &mut FnCtx<'_>,
    array_handle: &str,
    value_discarded: bool,
) -> String {
    if value_discarded {
        return double_literal(0.0);
    }
    let blk = ctx.block();
    let len_i32 = blk.call(I32, "js_array_length", &[(I64, array_handle)]);
    blk.uitofp(I32, &len_i32, DOUBLE)
}

fn emit_array_box_length(ctx: &mut FnCtx<'_>, array_box: &str, value_discarded: bool) -> String {
    if value_discarded {
        return double_literal(0.0);
    }
    let blk = ctx.block();
    let array_handle = unbox_to_i64(blk, array_box);
    emit_array_handle_length(ctx, &array_handle, false)
}

/// Publish a (possibly reallocated) array head back into whichever storage
/// backs `array_id`.
///
/// Extracted verbatim from `Expr::ArrayPush`'s generic tail in #7634 so that
/// the spread arm and the spec-order arm share one copy: three sites emitting
/// the same five-way storage chain is three places for the #5459 fall-through
/// to be got wrong. The two early `return`s are the boxed cases — they must NOT
/// also take the capture-slot store below, which would clobber the box pointer
/// in the capture slot with the array pointer, so the next push would treat the
/// array as the box and silently lose the realloc write-back.
///
/// `what` names the caller for the "local not in scope" diagnostic.
fn emit_push_writeback(
    ctx: &mut FnCtx<'_>,
    array_id: u32,
    new_box: &str,
    what: &str,
) -> Result<()> {
    // Boxed var takes priority: write through the box so every closure sharing
    // the box sees the new pointer.
    if ctx.boxed_vars.contains(&array_id) {
        // Captured-through-closure boxed var.
        if let Some(&capture_idx) = ctx.closure_captures.get(&array_id) {
            let closure_ptr =
                super::current_closure_ptr_value(ctx, &format!("{what} boxed captured"))?;
            let idx_str = capture_idx.to_string();
            let blk = ctx.block();
            let box_ptr = blk.call(
                I64,
                "js_closure_get_capture_bits",
                &[(I64, &closure_ptr), (I32, &idx_str)],
            );
            let new_bits = blk.bitcast_double_to_i64(new_box);
            blk.call_void("js_box_set_bits", &[(I64, &box_ptr), (I64, &new_bits)]);
            // Gen-GC Phase C2: the realloc'd array head is a (possibly young)
            // heap pointer stored into an existing box — barrier the box parent
            // so a minor GC can't miss it.
            emit_write_barrier(ctx, &box_ptr, &new_bits);
            return Ok(());
        } else if let Some(slot) = ctx.locals.get(&array_id).cloned() {
            let blk = ctx.block();
            let box_ptr = blk.load(I64, &slot);
            let new_bits = blk.bitcast_double_to_i64(new_box);
            blk.call_void("js_box_set_bits", &[(I64, &box_ptr), (I64, &new_bits)]);
            // Gen-GC Phase C2: barrier the box parent (see capture path).
            emit_write_barrier(ctx, &box_ptr, &new_bits);
            return Ok(());
        }
        // #5459: `array_id` is in `boxed_vars` but has no box location in THIS
        // context — it's a module-level global accessed directly from a nested
        // function (the load path read `@global`, not a box-get). Returning here
        // would skip the realloc write-back entirely, so the relocated array
        // header is never stored to the registered GC-root global slot: the old
        // head is freed on the next GC and the global dangles (use-after-free /
        // corrupted length). Fall through to the module-global store-back below
        // instead of returning.
    }
    if let Some(&capture_idx) = ctx.closure_captures.get(&array_id) {
        let closure_ptr = super::current_closure_ptr_value(ctx, &format!("{what} captured"))?;
        let idx_str = capture_idx.to_string();
        let new_bits = ctx.block().bitcast_double_to_i64(new_box);
        ctx.block().call_void(
            "js_closure_set_capture_bits",
            &[(I64, &closure_ptr), (I32, &idx_str), (I64, &new_bits)],
        );
        // Gen-GC Phase C2: the realloc'd array head stored into the closure
        // capture is a (possibly young) heap pointer — barrier the closure
        // parent.
        emit_write_barrier(ctx, &closure_ptr, &new_bits);
    } else if let Some(slot) = ctx.locals.get(&array_id).cloned() {
        ctx.block().store(DOUBLE, new_box, &slot);
    } else if let Some(global_name) = ctx.module_globals.get(&array_id).cloned() {
        let g_ref = format!("@{}", global_name);
        // GC_STORE_AUDIT(ROOT): module global array slot is a registered mutable GC root.
        emit_root_nanbox_store_on_block(ctx.block(), new_box, &g_ref);
    } else {
        return Err(anyhow!("{}({}): local not in scope", what, array_id));
    }
    Ok(())
}

/// Where an inline push tier's receiver binding lives: a stack slot or a
/// module-global root cell. Both inline tiers below need exactly two binding
/// operations — a head write-back after a slow/realloc arm, and a head reload
/// at the merge for the returned `length` — and both were hard-coded to
/// `ctx.locals`, which silently excluded module-global receivers from BOTH
/// tiers: a global `out.push(v)` fell to a bare `js_array_push_f64_spec` call
/// per push (~26 ns vs 2.5 on the isolated append). The write-back twin
/// (`emit_push_writeback`) has handled globals all along; this mirrors its
/// two arms for the tiers. #8617 precedent: extending an inline lane's
/// admission from slot locals to module-global bindings.
enum PushReceiverHome {
    Slot(String),
    Global(String),
}

impl PushReceiverHome {
    fn resolve(ctx: &FnCtx<'_>, array_id: u32) -> Option<Self> {
        if ctx.boxed_vars.contains(&array_id) || ctx.closure_captures.contains_key(&array_id) {
            return None;
        }
        if let Some(slot) = ctx.locals.get(&array_id) {
            return Some(Self::Slot(slot.clone()));
        }
        if let Some(name) = ctx.module_globals.get(&array_id) {
            return Some(Self::Global(format!("@{}", name)));
        }
        None
    }

    fn store_head(&self, blk: &mut LlBlock, new_box: &str) {
        match self {
            Self::Slot(slot) => {
                blk.store(DOUBLE, new_box, slot);
            }
            Self::Global(g_ref) => {
                // GC_STORE_AUDIT(ROOT): module global array slot is a
                // registered mutable GC root.
                emit_root_nanbox_store_on_block(blk, new_box, g_ref);
            }
        }
    }

    fn load_head(&self, blk: &mut LlBlock) -> String {
        match self {
            Self::Slot(slot) => blk.load(DOUBLE, slot),
            Self::Global(g_ref) => blk.load(DOUBLE, g_ref),
        }
    }
}

fn lower_array_push_value(
    ctx: &mut FnCtx<'_>,
    value: &Expr,
    layout_note_needed: bool,
    write_barrier_needed: bool,
) -> Result<(String, Option<String>)> {
    if !layout_note_needed && !write_barrier_needed {
        return Ok((lower_expr(ctx, value)?, None));
    }

    let lowered = lower_expr_native(ctx, value, ExpectedNativeRep::JsValueBits)?;
    let value_bits = lowered.value.clone();
    let value_double = ctx.block().bitcast_i64_to_double(&value_bits);
    ctx.record_lowered_value_with_access_mode(
        "ArrayPush",
        None,
        "array_push.slot_value_bits",
        &lowered,
        None,
        None,
        None,
        None,
        false,
        false,
        vec![
            format!("layout_note_needed={}", layout_note_needed as u8),
            format!("write_barrier_needed={}", write_barrier_needed as u8),
            "boxed_at=array_push_slot_or_runtime_helper_edge".to_string(),
        ],
    );
    Ok((value_double, Some(value_bits)))
}

/// Does evaluating `arg` **rebind** `array_id` — assign the binding a different
/// array — rather than merely mutate the array it names?
///
/// Only a direct `LocalSet` / `Update` on that id, in this expression, can do
/// it without the binding also being reachable from other code. A closure
/// LITERAL inside the argument is answered `true` without looking inside:
/// `walk_expr_children` deliberately does not descend into a closure body, and
/// "a closure that assigns it is boxed, so [`push_receiver_is_rebindable`]'s
/// other clause catches it" is a second-order argument this predicate should
/// not rest on.
fn expr_rebinds_local(arg: &Expr, array_id: u32) -> bool {
    match arg {
        Expr::LocalSet(id, _) | Expr::Update { id, .. } => {
            if *id == array_id {
                return true;
            }
        }
        Expr::Closure { .. } => return true,
        _ => {}
    }
    let mut found = false;
    perry_hir::walker::walk_expr_children(arg, &mut |child| {
        found = found || expr_rebinds_local(child, array_id);
    });
    found
}

/// Is the receiver binding of `arr.push(arg)` reachable for **rebinding** while
/// `arg` is evaluated? (#7634)
///
/// ES2024 evaluates the `MemberExpression` `arr.push` to a Reference *before*
/// the argument list, so the push must land on the array `arr` named at that
/// moment. Perry lowers the argument first and reads the receiver afterwards,
/// which observes whatever `arg` left in the binding. That divergence is
/// **unobservable unless the binding can change**, and this predicate is the
/// as-if test: when it answers `false`, the two orders name the same array and
/// the historical lowering — with its inline fast tiers and its `Reuse` verdict
/// from `operand_protection` — is kept byte for byte.
///
/// It answers `true` in exactly three cases:
///
///  * `arg` rebinds the id itself ([`expr_rebinds_local`]);
///  * the binding is **boxed** — `collect_boxed_vars`' rule is "captured AND
///    mutated", so a boxed id is one some closure or the enclosing function
///    assigns. A captured-but-never-assigned array is not boxed and stays on
///    the fast path, which is why `items.forEach(x => rows.push(f(x)))` costs
///    nothing;
///  * the binding is a module global, which any function can assign.
///
/// The last two additionally require the argument to be able to reach a
/// collection point at all (`any_operand_may_collect`, the same window
/// predicate every rooting decision in this crate consults). `g.push(1)` on a
/// module global cannot rebind anything: there is no call to do it in.
fn push_receiver_is_rebindable(ctx: &FnCtx<'_>, array_id: u32, arg: &Expr) -> bool {
    if expr_rebinds_local(arg, array_id) {
        return true;
    }
    let reachable_from_other_code =
        ctx.boxed_vars.contains(&array_id) || ctx.module_globals.contains_key(&array_id);
    reachable_from_other_code && rooting::any_operand_may_collect(ctx, std::iter::once(arg))
}

/// The spec-ordered push: receiver first, rooted across the argument.
///
/// Reached only when [`push_receiver_is_rebindable`] says the order is
/// observable, which is also exactly when the receiver acquires a rooting
/// window — the receiver is now live across arbitrary user code, so it is an
/// operand group rather than a post-argument load. The two are one change
/// (#7634's own framing) and `operand_protection` supplies the `Root`: a local
/// or a module global is deliberately NOT `Reload`-able, because re-deriving it
/// would observe the argument's assignment, which is the bug.
///
/// It deliberately does **not** take any of the inline fast tiers. Those all
/// publish the reallocated head back into the binding unconditionally, and once
/// the argument may have rebound the binding that store lands on the wrong
/// array: `a.push(f())` with `f` setting `a = [9]` would overwrite `[9]` with
/// the grown `[1,2]`. Here the write-back is guarded on the binding still
/// naming the array that was pushed onto; when it does not, the store is simply
/// skipped and aliases stay valid through the forwarding pointer
/// `js_array_push_f64` installs (issue #233), exactly as they do for
/// `const x = a; a.push(1)`.
fn lower_array_push_spec_order(
    ctx: &mut FnCtx<'_>,
    array_id: u32,
    array_expr: &Expr,
    value: &Expr,
    layout_note_needed: bool,
    write_barrier_needed: bool,
    value_discarded: bool,
) -> Result<String> {
    rooting::with_operands_rooted_across(
        ctx,
        std::slice::from_ref(&array_expr),
        std::slice::from_ref(&value),
        |ctx| lower_array_push_value(ctx, value, layout_note_needed, write_barrier_needed),
        |ctx, vals, (v, _v_bits)| {
            let recv_box = vals[0].clone();
            // The binding as it stands NOW. Equal to `recv_box` unless the
            // argument rebound it.
            let cur_box = lower_expr(ctx, array_expr)?;
            let blk = ctx.block();
            let recv_bits = blk.bitcast_double_to_i64(&recv_box);
            let cur_bits = blk.bitcast_double_to_i64(&cur_box);
            let still_bound = blk.icmp_eq(I64, &cur_bits, &recv_bits);
            let recv_handle = unbox_to_i64(blk, &recv_box);
            let new_handle = blk.call(
                I64,
                "js_array_push_f64_spec",
                &[(I64, &recv_handle), (DOUBLE, &v)],
            );
            let new_box = nanbox_pointer_inline(blk, &new_handle);

            let wb_idx = ctx.new_block("apush.spec.writeback");
            let done_idx = ctx.new_block("apush.spec.done");
            let wb_label = ctx.block_label(wb_idx);
            let done_label = ctx.block_label(done_idx);
            ctx.block().cond_br(&still_bound, &wb_label, &done_label);

            ctx.current_block = wb_idx;
            emit_push_writeback(ctx, array_id, &new_box, "ArrayPush")?;
            ctx.block().br(&done_label);

            ctx.current_block = done_idx;
            Ok(emit_array_handle_length(ctx, &new_handle, value_discarded))
        },
    )
}

/// [`lower_array_push_spec_order`] for `arr.push(...src)`.
fn lower_array_push_spread_spec_order(
    ctx: &mut FnCtx<'_>,
    array_id: u32,
    array_expr: &Expr,
    source: &Expr,
    value_discarded: bool,
) -> Result<String> {
    rooting::with_operands_rooted(ctx, &[array_expr, source], |ctx, vals| {
        let recv_box = vals[0].clone();
        let src_box = vals[1].clone();
        let cur_box = lower_expr(ctx, array_expr)?;
        let blk = ctx.block();
        let recv_bits = blk.bitcast_double_to_i64(&recv_box);
        let cur_bits = blk.bitcast_double_to_i64(&cur_box);
        let still_bound = blk.icmp_eq(I64, &cur_bits, &recv_bits);
        let dst_handle = unbox_to_i64(blk, &recv_box);
        let src_handle = unbox_to_i64(blk, &src_box);
        let new_handle = blk.call(
            I64,
            "js_array_concat",
            &[(I64, &dst_handle), (I64, &src_handle)],
        );
        let new_box = nanbox_pointer_inline(blk, &new_handle);

        let wb_idx = ctx.new_block("apushspread.spec.writeback");
        let done_idx = ctx.new_block("apushspread.spec.done");
        let wb_label = ctx.block_label(wb_idx);
        let done_label = ctx.block_label(done_idx);
        ctx.block().cond_br(&still_bound, &wb_label, &done_label);

        ctx.current_block = wb_idx;
        emit_push_writeback(ctx, array_id, &new_box, "ArrayPushSpread")?;
        ctx.block().br(&done_label);

        ctx.current_block = done_idx;
        Ok(emit_array_handle_length(ctx, &new_handle, value_discarded))
    })
}

/// Flags in the object's `GcHeader::_reserved` word any of which takes the
/// receiver off the plain-data-property path: `OBJ_FLAG_FROZEN` (0x01),
/// `OBJ_FLAG_SEALED` (0x02), `OBJ_FLAG_NO_EXTEND` (0x04) and
/// `OBJ_FLAG_HAS_DESCRIPTORS` (0x800). The field write-back below is a
/// repair, not a user store: on such a receiver it is skipped rather than
/// risk a throw or an accessor, and the field keeps working through the
/// forwarding stub as it always did.
const FIELD_WRITEBACK_BLOCKING_FLAGS_I16: &str = "2055";
const POINTER_TAG_HI16: &str = "32765"; // 0x7FFD
const HANDLE_BAND_TOP: &str = "1048575"; // 0x0FFFFF — objects are above
const HANDLE_MASK_48: &str = "281474976710655"; // 0x0000_FFFF_FFFF_FFFF
const GC_TYPE_OBJECT_I8: &str = "2";

/// `Expr::ArrayPush`. When `field_writeback` names a class field (the
/// `perry-transform::field_push_local_bind` expansion of `this.f.push(v)`),
/// the append is followed by the field write-back the HIR cannot express:
/// the receiver local's HANDLE BITS are compared before and after the push
/// and, when they differ, `this.f` is re-pointed at the local. A JS-level
/// `!==` cannot do this — a growing append leaves the old head forwarding to
/// the new one and equality sees through forwarding (#8897), so the field
/// would keep the stub and every later `this.f.length` / `this.f[i]` would
/// take the dynamic property path.
pub(crate) fn lower(ctx: &mut FnCtx<'_>, expr: &Expr, value_discarded: bool) -> Result<String> {
    let Expr::ArrayPush {
        array_id,
        field_writeback: Some(field),
        ..
    } = expr
    else {
        return lower_inner(ctx, expr, value_discarded);
    };
    // The bits BEFORE the append, held as an integer, never a pointer: a
    // collection inside the push may move the array and refresh the rooted
    // local, and stale bits then compare unequal — the conservative
    // direction (one redundant re-point of the same object).
    let before_box = lower_expr(ctx, &Expr::LocalGet(*array_id))?;
    let before_bits = ctx.block().bitcast_double_to_i64(&before_box);
    let result = lower_inner(ctx, expr, value_discarded)?;
    emit_field_push_writeback(ctx, *array_id, field, &before_bits)?;
    Ok(result)
}

/// The write-back half of [`lower`]: `if (bits(local) != before && this is
/// a plain object && bits(this.<field>) == before) this.<field> = local`, as
/// an ordinary `PropertySet` lowering (class-field IC, barrier and layout
/// note included) behind three inline tests, all off the hot path (the
/// first fails whenever the append did not re-allocate). The header test is
/// what makes the repair unobservable: it runs only on a `GC_TYPE_OBJECT`
/// receiver with none of [`FIELD_WRITEBACK_BLOCKING_FLAGS_I16`] set, i.e. a
/// plain data-property store. The field test is what makes it a REPAIR and
/// not a store: the receiver was read before the argument was evaluated
/// (`let __push_recv = this.f` precedes the push), so an argument that
/// assigns `this.f` itself — `this.f.push(this.reset())` — must win, and it
/// does, because the field then no longer holds the captured head. (A
/// collection that already rewrote the field to the moved array fails the
/// same test and skips a store that would have been redundant.)
fn emit_field_push_writeback(
    ctx: &mut FnCtx<'_>,
    array_id: u32,
    field: &str,
    before_bits: &str,
) -> Result<()> {
    let after_box = lower_expr(ctx, &Expr::LocalGet(array_id))?;
    let this_box = lower_expr(ctx, &Expr::This)?;

    let deref_idx = ctx.new_block("apush.field.deref");
    let field_idx = ctx.new_block("apush.field.still_held");
    let store_idx = ctx.new_block("apush.field.writeback");
    let done_idx = ctx.new_block("apush.field.done");
    let deref_label = ctx.block_label(deref_idx);
    let field_label = ctx.block_label(field_idx);
    let store_label = ctx.block_label(store_idx);
    let done_label = ctx.block_label(done_idx);

    {
        let blk = ctx.block();
        let after_bits = blk.bitcast_double_to_i64(&after_box);
        let same = blk.icmp_eq(I64, &after_bits, before_bits);
        let this_bits = blk.bitcast_double_to_i64(&this_box);
        let tag = blk.lshr(I64, &this_bits, "48");
        let is_ptr = blk.icmp_eq(I64, &tag, POINTER_TAG_HI16);
        let handle = blk.and(I64, &this_bits, HANDLE_MASK_48);
        let above_band = blk.icmp_ugt(I64, &handle, HANDLE_BAND_TOP);
        let ptr_ok = blk.and(I1, &is_ptr, &above_band);
        let moved = blk.icmp_eq(I1, &same, "false");
        let deref = blk.and(I1, &moved, &ptr_ok);
        blk.cond_br(&deref, &deref_label, &done_label);
    }

    ctx.current_block = deref_idx;
    {
        let blk = ctx.block();
        let this_bits = blk.bitcast_double_to_i64(&this_box);
        let handle = blk.and(I64, &this_bits, HANDLE_MASK_48);
        let obj_ptr = blk.inttoptr(I64, &handle);
        // GcHeader precedes the object: obj_type @-8 (i8), _reserved @-6 (i16).
        let gtype_ptr = blk.gep(I8, &obj_ptr, &[(I64, "-8")]);
        let gtype = blk.load(I8, &gtype_ptr);
        let is_object = blk.icmp_eq(I8, &gtype, GC_TYPE_OBJECT_I8);
        let res_ptr = blk.gep(I8, &obj_ptr, &[(I64, "-6")]);
        let reserved = blk.load(I16, &res_ptr);
        let blocking = blk.and(I16, &reserved, FIELD_WRITEBACK_BLOCKING_FLAGS_I16);
        let plain = blk.icmp_eq(I16, &blocking, "0");
        let plain_object = blk.and(I1, &is_object, &plain);
        blk.cond_br(&plain_object, &field_label, &done_label);
    }

    ctx.current_block = field_idx;
    let field_box = lower_expr(
        ctx,
        &Expr::PropertyGet {
            object: Box::new(Expr::This),
            property: field.to_string(),
            byte_offset: 0,
        },
    )?;
    {
        let blk = ctx.block();
        let field_bits = blk.bitcast_double_to_i64(&field_box);
        let still_held = blk.icmp_eq(I64, &field_bits, before_bits);
        blk.cond_br(&still_held, &store_label, &done_label);
    }

    ctx.current_block = store_idx;
    lower_expr(
        ctx,
        &Expr::PropertySet {
            object: Box::new(Expr::This),
            property: field.to_string(),
            value: Box::new(Expr::LocalGet(array_id)),
        },
    )?;
    ctx.block().br(&done_label);

    ctx.current_block = done_idx;
    Ok(())
}

fn lower_inner(ctx: &mut FnCtx<'_>, expr: &Expr, value_discarded: bool) -> Result<String> {
    match expr {
        Expr::ArrayPush {
            array_id, value, ..
        } => {
            // Resolve the array storage in priority order: closure
            // capture (slot in the closure header), local alloca slot,
            // module-level global. The realloc-pointer write-back must
            // go to whichever storage we read from.
            let array_expr = Expr::LocalGet(*array_id);
            // #7469: this local's element layout was declared all-pointer at
            // its allocation site (`collectors/all_pointer_arrays.rs` proved
            // every store into it is a push of a by-construction heap
            // pointer), and THIS pushed value is one of them. The inline store
            // below then needs neither the per-slot layout note nor the
            // numeric-write note — but only behind the header test in the
            // `nofwd` block, which re-validates the declaration at every single
            // push. Any push that fails it falls through to `js_array_push_f64`
            // and records the slot exactly as it always did.
            let declared_all_pointer = ctx.native_facts.declares_all_pointer_elements(*array_id)
                && crate::expr::expr_produces_fresh_heap_allocation(value);
            let layout_note_needed =
                !declared_all_pointer && array_store_needs_layout_note(ctx, &array_expr, value);
            // The string-addref demote is a DIFFERENT question from the layout
            // note and must not ride its gate here: `expr_produces_fresh_heap_
            // allocation` admits `new C()`, whose constructor return override
            // can hand back a uniquely-owned heap string. Every other push
            // keeps the historical coupling exactly.
            let string_addref_needed = if declared_all_pointer {
                crate::expr::store_needs_string_addref(ctx, value)
            } else {
                layout_note_needed
            };
            let write_barrier_needed = array_store_needs_write_barrier(ctx, value);
            let value_is_statically_numeric = is_numeric_expr(ctx, value);
            let value_is_numeric = guarded_numeric_add_push_candidate(ctx, value);
            let require_numeric_layout =
                value_is_numeric && expr_has_numeric_pointer_free_array_layout(ctx, &array_expr);
            // #7839 — the inline append's three GC-bookkeeping calls behind ONE
            // live test of the stored bits, exactly #7511's class-field shape.
            // See `emit_numeric_push_store_pointer_tested` for why each call is
            // dead when the test says "no pointer, no watched layout state",
            // and why the gate is `value_is_numeric` rather than unconditional.
            let guarded_numeric_bookkeeping = value_is_numeric
                && !declared_all_pointer
                && (layout_note_needed || string_addref_needed || write_barrier_needed);
            // #7634: spec order (receiver Reference, then argument) is only
            // observable when the argument can rebind the receiver. When it
            // can, take the rooted spec-ordered arm; when it cannot — the hot
            // shape, `out.push(f(x))` over a plain local — the historical
            // argument-then-receiver order names the same array and every tier
            // below keeps the IR it has always emitted.
            if push_receiver_is_rebindable(ctx, *array_id, value) {
                return lower_array_push_spec_order(
                    ctx,
                    *array_id,
                    &array_expr,
                    value,
                    layout_note_needed,
                    write_barrier_needed,
                    value_discarded,
                );
            }
            let (v, v_bits) =
                lower_array_push_value(ctx, value, layout_note_needed, write_barrier_needed)?;
            let arr_box = lower_expr(ctx, &array_expr)?;

            // Repsel 4a.1 (#6904 recon): the guarded numeric push was an
            // INVERSION — 3 out-of-line calls (guard + unboxed push + length)
            // where the untyped tier below inlines the store. When feedback
            // emission is off and the pushed value is canonical-raw-f64 by
            // construction, the untyped inline tier is byte-identical for a
            // numeric-layout array: the bare `store double` writes canonical
            // bits (keeping the raw-f64 invariant with no canonicalization
            // call — `array_store_needs_layout_note` already skips the note
            // for exactly this array/value class), and every guard the
            // runtime tier checked (forwarded / integrity / descriptors /
            // capacity) is checked inline before the store. Non-canonical
            // numeric values (e.g. a read fallback's INT32-boxed bits) keep
            // the runtime-guarded tier: stored verbatim they would corrupt
            // the dense raw-f64 invariant.
            // Metadata-selected `+` is only a guarded candidate: a declared
            // number may hold a string, boolean, or any other JS value. Keep
            // that shape on the runtime numeric guard even when feedback
            // emission is disabled. A guard miss reaches `js_array_push_f64`,
            // which performs the generic store and revokes raw-f64 layout for
            // every non-number (including non-pointer tags).
            let inline_value_shape =
                crate::type_analysis::expr_produces_canonical_raw_f64(ctx, value);
            let keep_guarded_numeric_push =
                super::typed_feedback_emission_enabled() || !inline_value_shape;
            if require_numeric_layout && keep_guarded_numeric_push {
                if let Some(home) = PushReceiverHome::resolve(ctx, *array_id) {
                    let feedback_site_id = emit_typed_feedback_register_site(
                        ctx,
                        TypedFeedbackKind::ArrayElement,
                        "array.push",
                        TypedFeedbackContract::numeric_array_push(),
                    );
                    let fast_idx = ctx.new_block("apush.numeric_fast");
                    let fallback_idx = ctx.new_block("apush.numeric_fallback");
                    let merge_idx = ctx.new_block("apush.numeric_merge");
                    let fast_label = ctx.block_label(fast_idx);
                    let fallback_label = ctx.block_label(fallback_idx);
                    let merge_label = ctx.block_label(merge_idx);

                    let guard_ok = {
                        let blk = ctx.block();
                        let guard_i32 = blk.call(
                            I32,
                            "js_typed_feedback_numeric_array_push_guard",
                            &[(I64, &feedback_site_id), (DOUBLE, &arr_box), (DOUBLE, &v)],
                        );
                        blk.icmp_ne(I32, &guard_i32, "0")
                    };
                    ctx.block().cond_br(&guard_ok, &fast_label, &fallback_label);

                    ctx.current_block = fast_idx;
                    {
                        let blk = ctx.block();
                        let arr_handle = unbox_to_i64(blk, &arr_box);
                        let new_handle = blk.call(
                            I64,
                            "js_array_numeric_push_f64_unboxed",
                            &[(I64, &arr_handle), (DOUBLE, &v)],
                        );
                        let new_box = nanbox_pointer_inline(blk, &new_handle);
                        home.store_head(blk, &new_box);
                        blk.br(&merge_label);
                    }
                    let pushed = LoweredValue {
                        semantic: SemanticKind::JsNumber,
                        rep: NativeRep::F64,
                        llvm_ty: DOUBLE,
                        value: v.clone(),
                    };
                    ctx.record_lowered_value_with_access_mode_and_facts(
                        "NumericArrayPush",
                        Some(*array_id),
                        "js_array_numeric_push_f64_unboxed",
                        &pushed,
                        Some(BoundsState::Guarded {
                            guard_id: "numeric_array_push_guard".to_string(),
                        }),
                        None,
                        Some(BufferAccessMode::CheckedNative),
                        None,
                        None,
                        None,
                        vec![raw_f64_layout_fact(
                            Some(*array_id),
                            "consumed",
                            "numeric_array_push_guard",
                            None,
                        )],
                        Vec::new(),
                        false,
                        false,
                        Vec::new(),
                    );

                    ctx.current_block = fallback_idx;
                    {
                        let blk = ctx.block();
                        crate::expr::emit_typed_feedback_record_call(
                            blk,
                            "js_typed_feedback_record_fallback_call",
                            &[(I64, &feedback_site_id)],
                        );
                        let arr_handle = unbox_to_i64(blk, &arr_box);
                        let new_handle = blk.call(
                            I64,
                            "js_array_push_f64_spec",
                            &[(I64, &arr_handle), (DOUBLE, &v)],
                        );
                        let new_box = nanbox_pointer_inline(blk, &new_handle);
                        home.store_head(blk, &new_box);
                        blk.br(&merge_label);
                    }
                    let fallback = LoweredValue {
                        semantic: SemanticKind::JsValue,
                        rep: NativeRep::JsValue,
                        llvm_ty: DOUBLE,
                        value: v.clone(),
                    };
                    ctx.record_lowered_value_with_access_mode_and_facts(
                        "NumericArrayPush",
                        Some(*array_id),
                        "js_array_push_f64_spec",
                        &fallback,
                        Some(BoundsState::Unknown),
                        None,
                        Some(BufferAccessMode::DynamicFallback),
                        Some(MaterializationReason::RuntimeApi),
                        None,
                        None,
                        Vec::new(),
                        vec![
                            raw_f64_layout_fact(
                                Some(*array_id),
                                "rejected",
                                "numeric_array_push_guard",
                                Some(MaterializationReason::RuntimeApi),
                            ),
                            raw_f64_layout_fact(
                                Some(*array_id),
                                "invalidated",
                                "runtime_api",
                                Some(MaterializationReason::RuntimeApi),
                            ),
                        ],
                        false,
                        false,
                        Vec::new(),
                    );

                    ctx.current_block = merge_idx;
                    if value_discarded {
                        // Skip the slot reload too — it only feeds the length.
                        return Ok(double_literal(0.0));
                    }
                    let current_box = home.load_head(ctx.block());
                    return Ok(emit_array_box_length(ctx, &current_box, false));
                }
            }

            // Fast path: local-bound, non-captured, non-boxed array.
            // This is the canonical hot shape — `out.push(...)` over a
            // local array variable. The runtime's `js_array_push_f64`
            // does `clean_arr_ptr_mut` (heap-range check + forwarding
            // chain walk + length/capacity sanity check + lazy detect)
            // before every store; for an array that's known to be a
            // plain heap pointer, that's wasted work on the *millions*
            // of pushes a JSON-pipeline-style workload performs.
            //
            // Inline shape (mirrors `lower_index_set_fast`):
            //
            //   if (gc_flags & FORWARDED): call js_array_push_f64 (slow)
            //   else:
            //     length   = load i32, arr+0
            //     capacity = load i32, arr+4
            //     if (length < capacity):
            //       store double value, arr+8+length*8
            //       store i32 (length+1), arr+0
            //       done
            //     else:
            //       call js_array_push_f64 (grow path)
            //
            // The fast inline branch needs no slot write-back — the
            // array pointer doesn't change unless we grow. The slow
            // branches both update the slot via the existing
            // boxed/captured/local fall-through below.
            if let Some(home) = PushReceiverHome::resolve(ctx, *array_id) {
                let apush_meta_offset =
                    crate::target_layout::object_meta_slot_offset_bytes(ctx.target_triple)
                        .to_string();
                let blk = ctx.block();
                let arr_handle = unbox_to_i64(blk, &arr_box);

                // Issue #233: forwarded arrays must follow the
                // forwarding chain. Route through the runtime which
                // calls clean_arr_ptr_mut and writes into the live
                // head — the inline path's offset-0 length read would
                // otherwise pick up the lower 32 bits of the
                // forwarding pointer (garbage).
                //
                // #7574: the same load also has to prove the receiver IS an
                // array. `Expr::ArrayPush` is folded from the receiver's
                // DECLARED type, and a declared type is a hint, never a layout
                // fact (CLAUDE.md, *Known Limitations*), so
                // `const a: number[] = new MyArr()` — a `class X extends Array`
                // instance, which perry models as a plain `ObjectHeader` —
                // reached the inline store below. `ObjectHeader` overlays
                // `ArrayHeader` field for field, so `length` read
                // `class_id` and `capacity` read the ShapeId word (#8113; it
                // was `object_type` (= 1) and `class_id` before):
                // `1 < class_id` passed the in-bounds test and the value was
                // stored at `handle + 8 + 1*8` — i.e. over `ObjectHeader
                // .keys_array`, a live GC child edge — while `length + 1`
                // overwrote the first header word. The SECOND push then SIGSEGVed
                // (exit 139) dereferencing `keys_array`, whose bytes were now
                // the double `1.0` (fault address `0x3ff0000000000000`).
                //
                // Route any non-`GC_TYPE_ARRAY` receiver to `js_array_push_f64`
                // — the same slow arm forwarding already uses — which resolves
                // an array-like object receiver onto the spec-generic engine.
                // Strictly more restrictive than the old test: nothing that
                // used to take the slow arm now takes the inline store.
                let gc_type_addr = blk.sub(I64, &arr_handle, "8");
                let gc_type_ptr = blk.inttoptr(I64, &gc_type_addr);
                let gc_type = blk.load(I8, &gc_type_ptr);
                let not_array = blk.icmp_ne(I8, &gc_type, "1"); // != GC_TYPE_ARRAY
                let gc_flags_addr = blk.sub(I64, &arr_handle, "7");
                let gc_flags_ptr = blk.inttoptr(I64, &gc_flags_addr);
                let gc_flags = blk.load(I8, &gc_flags_ptr);
                let fwd_bits = blk.and(I8, &gc_flags, "128");
                let fwd_set = blk.icmp_ne(I8, &fwd_bits, "0");
                let is_fwd = blk.or(I1, &not_array, &fwd_set);

                let fwd_idx = ctx.new_block("apush.fwd");
                // An elements-backed Array subclass (`ObjectMeta.elements`)
                // appends to its store: the payload is resolved here and the
                // ordinary room/flag tests and the inline store below run on
                // it, so `sub.push(v)` stops paying a runtime entry whose only
                // job is to follow one pointer.
                let elem_idx = ctx.new_block("apush.elements");
                let elem_check_idx = ctx.new_block("apush.elements.check");
                let nofwd_idx = ctx.new_block("apush.nofwd");
                let inbounds_idx = ctx.new_block("apush.inbounds");
                let realloc_idx = ctx.new_block("apush.realloc");
                let merge_idx = ctx.new_block("apush.merge");

                let fwd_label = ctx.block_label(fwd_idx);
                let elem_label = ctx.block_label(elem_idx);
                let elem_check_label = ctx.block_label(elem_check_idx);
                let nofwd_label = ctx.block_label(nofwd_idx);
                let inbounds_label = ctx.block_label(inbounds_idx);
                let realloc_label = ctx.block_label(realloc_idx);
                let merge_label = ctx.block_label(merge_idx);

                {
                    let blk = ctx.block();
                    // A forwarded receiver keeps the runtime arm; a live
                    // non-Array receiver gets the elements probe.
                    blk.cond_br(&fwd_set, &fwd_label, &elem_label);
                }
                let _ = &is_fwd;

                ctx.current_block = elem_idx;
                let elem_store = {
                    let blk = ctx.block();
                    let array_receiver = blk.icmp_eq(I8, &gc_type, "1");
                    let meta_addr = blk.add(I64, &arr_handle, &apush_meta_offset);
                    let meta_slot = blk.inttoptr(I64, &meta_addr);
                    let meta = blk.load(I64, &meta_slot);
                    let has_meta = blk.icmp_ne(I64, &meta, "0");
                    let is_object = blk.icmp_eq(I8, &gc_type, "2");
                    let can_read_meta = blk.and(I1, &is_object, &has_meta);
                    // `select` keeps the word-12 load off a null meta pointer.
                    let safe_meta = blk.select(I1, &can_read_meta, I64, &meta, &arr_handle);
                    let meta_ptr = blk.inttoptr(I64, &safe_meta);
                    let store_slot = blk.gep(I64, &meta_ptr, &[(I64, "12")]);
                    let store = blk.load(I64, &store_slot);
                    let has_store = blk.icmp_ne(I64, &store, "0");
                    let backed = blk.and(I1, &can_read_meta, &has_store);
                    // The payload for the check block, materialised BEFORE the
                    // terminator so it dominates its uses; an ordinary Array
                    // receiver skips the probe entirely.
                    let payload = blk.select(I1, &backed, I64, &store, &arr_handle);
                    blk.cond_br(&array_receiver, &nofwd_label, &elem_check_label);
                    payload
                };
                let elem_probe_end = ctx.block_label(elem_idx);

                ctx.current_block = elem_check_idx;
                {
                    let blk = ctx.block();
                    let has_store = blk.icmp_ne(I64, &elem_store, "0");
                    let gc_type_addr = blk.sub(I64, &elem_store, "8");
                    let gc_type_ptr = blk.inttoptr(I64, &gc_type_addr);
                    let store_type = blk.load(I8, &gc_type_ptr);
                    let store_is_array = blk.icmp_eq(I8, &store_type, "1");
                    let gc_flags_addr = blk.sub(I64, &elem_store, "7");
                    let gc_flags_ptr = blk.inttoptr(I64, &gc_flags_addr);
                    let store_flags = blk.load(I8, &gc_flags_ptr);
                    let store_fwd = blk.and(I8, &store_flags, "128");
                    let store_live = blk.icmp_eq(I8, &store_fwd, "0");
                    let mut ok = blk.and(I1, &has_store, &store_is_array);
                    ok = blk.and(I1, &ok, &store_live);
                    blk.cond_br(&ok, &nofwd_label, &fwd_label);
                }
                let elem_check_end = ctx.block_label(elem_check_idx);

                // FORWARDED branch: route through runtime.
                ctx.current_block = fwd_idx;
                {
                    let blk = ctx.block();
                    let new_handle = blk.call(
                        I64,
                        "js_array_push_f64_spec",
                        &[(I64, &arr_handle), (DOUBLE, &v)],
                    );
                    let new_box = nanbox_pointer_inline(blk, &new_handle);
                    home.store_head(blk, &new_box);
                    blk.br(&merge_label);
                }

                // No forwarding — check the integrity flags, then read
                // length & capacity and branch on capacity. inline_store on
                // length < capacity, slow call on full.
                //
                // A frozen / sealed / non-extensible array, or one carrying
                // per-index/`length` descriptors (`OBJ_FLAG_ARRAY_DESCRIPTORS`),
                // must NOT take the raw inline store: `push` performs
                // `Set(O,"length",…,true)`, so a frozen array or one whose
                // `length` was made non-writable must throw a **TypeError**
                // (test262 push/set-length-zero-array-is-frozen and
                // set-length-zero-array-length-is-non-writable), and a
                // descriptor-carrying array needs the descriptor-aware runtime
                // store. All of these route to `js_array_push_f64`, which
                // throws / handles them correctly. The integrity bits live in
                // the GcHeader `_reserved` u16 at `arr - 6` (obj_type u8 at -8,
                // gc_flags u8 at -7, `_reserved` u16 at -6): mask
                // FROZEN|SEALED|NO_EXTEND|ARRAY_DESCRIPTORS = 0x407.
                let live_object_flags: String;
                ctx.current_block = nofwd_idx;
                let payload = ctx.block().phi(
                    I64,
                    &[
                        (&arr_handle, &elem_probe_end),
                        (&elem_store, &elem_check_end),
                    ],
                );
                {
                    let blk = ctx.block();
                    let flags_addr = blk.sub(I64, &payload, "6");
                    let flags_ptr = blk.inttoptr(I64, &flags_addr);
                    let obj_flags = blk.load(I16, &flags_ptr);
                    live_object_flags = obj_flags.clone();
                    let clean = if declared_all_pointer {
                        // #7469 — the elided-bookkeeping admission test. Same
                        // `_reserved` load, same one `and` + one `icmp` as the
                        // integrity test it replaces, but it additionally
                        // demands the array still carry the element-layout
                        // declaration this push's elisions rest on. Bits, from
                        // `gc/types.rs` + `gc/layout.rs` (GC_TYPE_ARRAY):
                        //
                        //   0x0407  FROZEN|SEALED|NO_EXTEND|ARRAY_DESCRIPTORS
                        //           -> must be 0, exactly as below
                        //   0x0080  GC_ARRAY_RAW_F64_LAYOUT   -> must be 0
                        //   0x1000  GC_ARRAY_RAW_F64_HOLES    -> must be 0
                        //   0x2000  GC_LAYOUT_ALL_POINTERS    -> must be 1
                        //   0xC000  layout state              -> SIDE_MASK
                        //
                        // mask 0xF487 == 62599, expected 0xA000 == 40960.
                        //
                        // The two raw-f64 bits are what makes eliding
                        // `js_array_note_numeric_write` sound: its whole body
                        // is "clear the numeric layout when the value is not a
                        // number", and with both bits already clear there is
                        // nothing left for it to clear.
                        //
                        // `ALL_POINTERS | SIDE_MASK` is what makes eliding
                        // `js_gc_note_slot_layout` sound: in that state the
                        // collector visits every slot in `0..length`, so the
                        // slot this push is about to write is scanned whether
                        // or not a mask bit was ever recorded for it.
                        //
                        // Testing the LIVE header rather than trusting the
                        // allocation-site declaration is deliberate. The
                        // runtime can revoke it — `rebuild_array_layout`
                        // (sort/splice) installs a precise mask,
                        // `js_array_is_numeric_f64_layout` can re-publish a
                        // still-empty array as RawF64 + POINTER_FREE — and an
                        // elided pointer store into a POINTER_FREE array is a
                        // stranded live child. Failing the test costs this push
                        // the inline store (it takes `js_array_push_f64`, which
                        // notes the slot); it can never cost correctness.
                        let admitted_bits = blk.and(I16, &obj_flags, "62599");
                        blk.icmp_eq(I16, &admitted_bits, "40960")
                    } else if guarded_numeric_bookkeeping {
                        // #7839 — the array's half of the guard, folded into the
                        // integrity test rather than emitted as a second one:
                        // same `and`, same `icmp`, a wider constant. Reaching
                        // the inline store now additionally proves the three
                        // `_reserved` states in which `js_gc_note_slot_layout`
                        // does real work for a NON-pointer value, so the store's
                        // guard has only the value left to test. See
                        // `emit_numeric_push_store_pointer_tested`.
                        let admitted_bits = blk.and(I16, &obj_flags, ARRAY_PUSH_NUMERIC_CLEAN_I16);
                        blk.icmp_eq(I16, &admitted_bits, "0")
                    } else {
                        // FROZEN(0x1)|SEALED(0x2)|NO_EXTEND(0x4)|ARRAY_DESCRIPTORS(0x400).
                        let integrity_bits = blk.and(I16, &obj_flags, "1031");
                        blk.icmp_eq(I16, &integrity_bits, "0")
                    };
                    // A sticky runtime byte records indexed properties on
                    // Array/Object.prototype (and custom Array prototypes).
                    // Such a property can intercept push with an inherited
                    // setter, so the raw append is valid only while the default
                    // prototype chain remains pristine.
                    let invalidated =
                        blk.load_volatile(I8, "@PERRY_ARRAY_INDEX_FAST_PATH_INVALIDATED");
                    let prototype_clean = blk.icmp_eq(I8, &invalidated, "0");
                    let clean = blk.and(I1, &clean, &prototype_clean);
                    let length = blk.safe_load_i32_from_ptr(&payload);
                    let cap_addr = blk.add(I64, &payload, "4");
                    let cap_ptr = blk.inttoptr(I64, &cap_addr);
                    let capacity = blk.load(I32, &cap_ptr);
                    let has_room = blk.icmp_ult(I32, &length, &capacity);
                    // Take the inline store only when there is room AND no
                    // integrity flag is set; otherwise fall to the runtime
                    // (`js_array_push_f64` throws for frozen / non-writable
                    // length and applies descriptors correctly).
                    let inline_ok = blk.and(I1, &has_room, &clean);
                    blk.cond_br(&inline_ok, &inbounds_label, &realloc_label);
                }

                // Inline store: arr+8+length*8 = value, length++.
                ctx.current_block = inbounds_idx;
                // #7511: the barrier is emitted separately, behind an inline
                // live test of the PARENT's generation, so the store emitter
                // below is told not to emit it. Everything else about the store
                // — the slot write, the string addref, the layout note, and
                // their ordering — is unchanged.
                //
                // `js_write_barrier_slot` still lands in exactly the position it
                // did before (after the layout note, before the numeric-write
                // note and the length bump), because a collection reached
                // between the store and the barrier would run with the
                // old→young edge unrecorded. The block is split here rather
                // than the call being sunk to the end of the block.
                let dynamic_pointer_bookkeeping =
                    !declared_all_pointer && !value_is_statically_numeric && layout_note_needed;
                let (length, element_addr, barrier_value_bits) = if guarded_numeric_bookkeeping {
                    emit_numeric_push_store_pointer_tested(
                        ctx,
                        &payload,
                        &v,
                        v_bits.as_deref(),
                        string_addref_needed,
                        layout_note_needed,
                        write_barrier_needed,
                    )
                } else if dynamic_pointer_bookkeeping {
                    let (length, element_addr, value_bits) = emit_dynamic_pointer_push_store(
                        ctx,
                        &payload,
                        &v,
                        v_bits.as_deref(),
                        &live_object_flags,
                        string_addref_needed,
                        layout_note_needed,
                    );
                    (
                        length,
                        element_addr,
                        write_barrier_needed.then_some(value_bits),
                    )
                } else {
                    let blk = ctx.block();
                    let length = blk.safe_load_i32_from_ptr(&payload);
                    let length_i64 = blk.zext(I32, &length, I64);
                    let byte_offset = blk.shl(I64, &length_i64, "3");
                    let with_header = blk.add(I64, &byte_offset, "8");
                    let element_addr = blk.add(I64, &payload, &with_header);
                    let element_ptr = blk.inttoptr(I64, &element_addr);
                    let value_bits = if let Some(value_bits) = v_bits.as_deref() {
                        emit_jsvalue_slot_store_with_value_bits_on_block(
                            blk,
                            &element_ptr,
                            &v,
                            value_bits,
                            &payload,
                            &length,
                            string_addref_needed,
                            layout_note_needed,
                            &payload,
                            &element_addr,
                            false,
                        )
                    } else {
                        emit_jsvalue_slot_store_with_flags_on_block(
                            blk,
                            &element_ptr,
                            &v,
                            &payload,
                            &length,
                            string_addref_needed,
                            layout_note_needed,
                            &payload,
                            &element_addr,
                            false,
                        )
                    };
                    // The store emitter only hands back the bits when it needed
                    // them itself; the barrier needs them whenever it is
                    // emitted, so materialize them here otherwise.
                    let barrier_value_bits = if write_barrier_needed {
                        Some(
                            value_bits
                                .clone()
                                .unwrap_or_else(|| blk.bitcast_double_to_i64(&v)),
                        )
                    } else {
                        None
                    };
                    // #7469: provably dead under `declared_all_pointer` — the
                    // `nofwd` admission test proved both raw-f64 bits already
                    // clear, and clearing them is this call's only effect.
                    // A metadata-only numeric candidate has not established
                    // the live value's kind. If this generic inline tier is
                    // used (for an array without a static raw-f64 fact), let
                    // the runtime note inspect every stored tag and revoke a
                    // dynamically active numeric layout on strings, booleans,
                    // undefined, and all other non-number values.
                    if !value_is_statically_numeric && !declared_all_pointer {
                        let value_bits = barrier_value_bits
                            .clone()
                            .or(value_bits)
                            .unwrap_or_else(|| blk.bitcast_double_to_i64(&v));
                        emit_array_numeric_write_note_on_block(blk, &payload, &value_bits);
                    }
                    (length, element_addr, barrier_value_bits)
                };
                if let Some(child_bits) = barrier_value_bits {
                    // `arr_handle` reached this block through the `nofwd` header
                    // test, so it is a live, non-forwarded GC array user
                    // pointer — the precondition for reading its header byte.
                    emit_write_barrier_slot_generation_tested(
                        ctx,
                        &payload,
                        &payload,
                        &element_addr,
                        &child_bits,
                        "apush",
                    );
                }
                {
                    let blk = ctx.block();
                    let new_length = blk.add(I32, &length, "1");
                    let arr_ptr = blk.inttoptr(I64, &payload);
                    // GC_STORE_AUDIT(POINTER_FREE): array length header update has no child pointer.
                    blk.store(I32, &new_length, &arr_ptr);
                    blk.br(&merge_label);
                }

                // Realloc: capacity exhausted. Runtime allocates a
                // bigger backing block and installs the forwarding
                // pointer; writeback the new head to the local slot.
                ctx.current_block = realloc_idx;
                {
                    let blk = ctx.block();
                    let new_handle = blk.call(
                        I64,
                        "js_array_push_f64_spec",
                        &[(I64, &arr_handle), (DOUBLE, &v)],
                    );
                    let new_box = nanbox_pointer_inline(blk, &new_handle);
                    home.store_head(blk, &new_box);
                    blk.br(&merge_label);
                }

                ctx.current_block = merge_idx;
                if value_discarded {
                    // Skip the slot reload too — it only feeds the length.
                    return Ok(double_literal(0.0));
                }
                let current_box = home.load_head(ctx.block());
                return Ok(emit_array_box_length(ctx, &current_box, false));
            }

            let blk = ctx.block();
            let arr_handle = unbox_to_i64(blk, &arr_box);
            let new_handle = blk.call(
                I64,
                "js_array_push_f64_spec",
                &[(I64, &arr_handle), (DOUBLE, &v)],
            );
            let new_box = nanbox_pointer_inline(blk, &new_handle);
            emit_push_writeback(ctx, *array_id, &new_box, "ArrayPush")?;
            Ok(emit_array_handle_length(ctx, &new_handle, value_discarded))
        }

        // `arr.push(...src)` — HIR variant carrying the destination
        // array's LocalId and the source expression (any iterable, in
        // practice an array or Set). Mirrors `Expr::ArrayPush` above:
        // load the destination from its slot, unbox both pointers, call
        // the runtime's `js_array_concat` (which walks the source and
        // calls `js_array_push_f64` per element + already handles
        // Set sources via SET_REGISTRY), NaN-box the realloc-aware
        // return pointer, and write back to whichever storage backs
        // `array_id`. Issue #248.
        Expr::ArrayPushSpread { array_id, source } => {
            let array_expr = Expr::LocalGet(*array_id);
            // #7634, same as `Expr::ArrayPush`: spec order is only observable
            // when the source can rebind the destination binding.
            if push_receiver_is_rebindable(ctx, *array_id, source) {
                return lower_array_push_spread_spec_order(
                    ctx,
                    *array_id,
                    &array_expr,
                    source,
                    value_discarded,
                );
            }
            // The operand pair, stated through the API instead of by statement
            // order. The window is EMPTY and stays empty: the only thing lowered
            // after `source` is the receiver, which is a slot read, so
            // `operand_protection` answers `Reuse` for both and this emits no
            // rooting IR at all. It is here so that a later edit which inserts a
            // lowering between the two is a change to a rooted group rather than
            // a silent reopening.
            rooting::with_operands_rooted(ctx, &[source.as_ref(), &array_expr], |ctx, vals| {
                let blk = ctx.block();
                let dst_handle = unbox_to_i64(blk, &vals[1]);
                let src_handle = unbox_to_i64(blk, &vals[0]);
                let new_handle = blk.call(
                    I64,
                    "js_array_concat",
                    &[(I64, &dst_handle), (I64, &src_handle)],
                );
                let new_box = nanbox_pointer_inline(blk, &new_handle);
                emit_push_writeback(ctx, *array_id, &new_box, "ArrayPushSpread")?;
                Ok(emit_array_handle_length(ctx, &new_handle, value_discarded))
            })
        }

        // -------- Closures (Phase D.1) --------
        // `function() { ... }` / `(x) => { ... }` — allocate a closure
        // object pointing at a pre-emitted function body, populate
        // capture slots, return the NaN-boxed pointer.
        //
        // The closure body is emitted as a top-level LLVM function
        // (`perry_closure_<modprefix>__<func_id>`) earlier in
        // `compile_module` via the `compile_closure` pass.
        _ => unreachable!("expr/mod.rs dispatched a variant not handled by this submodule"),
    }
}

/// #7634: the receiver-order gate, and the two IR shapes it selects between.
#[cfg(test)]
mod receiver_order_tests {
    use super::expr_rebinds_local;
    use perry_hir::types::Type;
    use perry_hir::{Expr, Function, Module as HirModule, Stmt};

    /// A one-function module whose body is `let a = []; a.push(<value>); return a;`.
    fn push_ir(value: Expr) -> String {
        let mut hir = HirModule::new("apush_receiver_order_test");
        hir.functions.push(Function {
            id: 0,
            name: "pushes".to_string(),
            type_params: Vec::new(),
            params: Vec::new(),
            return_type: Type::Any,
            body: vec![
                Stmt::Let {
                    id: 0,
                    name: "a".to_string(),
                    ty: Type::Any,
                    mutable: true,
                    init: Some(Expr::Array(Vec::new())),
                },
                Stmt::Expr(Expr::ArrayPush {
                    array_id: 0,
                    value: Box::new(value),
                    field_writeback: None,
                }),
                Stmt::Return(Some(Expr::LocalGet(0))),
            ],
            is_async: false,
            is_generator: false,
            is_strict: true,
            is_exported: false,
            captures: Vec::new(),
            decorators: Vec::new(),
            was_plain_async: false,
            was_unrolled: false,
        });
        let opts = crate::CompileOptions {
            emit_ir_only: true,
            ..Default::default()
        };
        let bytes = crate::compile_module(&hir, opts).expect("test module compiles");
        String::from_utf8(bytes).expect("LLVM IR is UTF-8")
    }

    /// An allocating argument that CANNOT reach the receiver's binding leaves
    /// the historical argument-then-receiver order in place: no spec-order
    /// blocks, and therefore none of the cost #7634 worried about.
    ///
    /// It asserts the ABSENCE of the spec-ordered arm rather than counting
    /// `js_gc_temp_root_push` call sites, and that is not a stylistic choice:
    /// `temp_root_push_double` lowers to a plain alloca `store` in alloca mode
    /// and to a stack-map index under statepoints, so a count reads zero on the
    /// default build and passes vacuously. The spec-ordered arm is the only
    /// thing in this lowering that roots the receiver, so its absence is the
    /// no-cost claim, stated in something the emitted IR always shows.
    ///
    /// It cannot pass vacuously in the other direction either: the same IR is
    /// required to still contain the push's own inline tier (`apush.nofwd`), so
    /// an empty or failed compile fails here rather than reporting "clean".
    #[test]
    fn an_unreachable_binding_keeps_the_historical_order_and_roots_nothing() {
        let ir = push_ir(Expr::Object(vec![("v".to_string(), Expr::Number(1.0))]));
        assert!(
            ir.contains("apush.nofwd"),
            "the push must still take its inline tier, or this test proves nothing:\n{ir}"
        );
        assert!(
            !ir.contains("apush.spec."),
            "a plain local nothing else can reach must NOT take the spec-ordered arm — and \
             the spec-ordered arm is the ONLY thing that roots the receiver, so its absence \
             is the no-cost claim:\n{ir}"
        );
    }

    /// An argument that assigns the receiver's own binding takes the
    /// spec-ordered arm: the receiver is lowered FIRST, rooted across the
    /// argument, and the realloc write-back is guarded on the binding still
    /// naming the array that was pushed onto.
    #[test]
    fn an_argument_that_rebinds_the_receiver_takes_the_spec_ordered_arm() {
        let ir = push_ir(Expr::Sequence(vec![
            Expr::LocalSet(0, Box::new(Expr::Array(vec![Expr::Number(9.0)]))),
            Expr::Number(2.0),
        ]));
        assert!(
            ir.contains("apush.spec.writeback"),
            "the rebinding argument must take the spec-ordered arm:\n{ir}"
        );
        assert!(
            ir.contains("apush.spec.done"),
            "the spec-ordered arm must join back through its own merge block:\n{ir}"
        );
        // The guard is what stops the grown OLD array being published into a
        // binding the argument has already pointed somewhere else.
        let guard = ir
            .lines()
            .find(|l| l.contains("br i1") && l.contains("label %apush.spec.writeback"))
            .unwrap_or_else(|| panic!("no guarded write-back branch in:\n{ir}"));
        let cond = guard
            .split_whitespace()
            .nth(2)
            .and_then(|c| c.strip_suffix(','))
            .unwrap_or_else(|| panic!("cannot read the branch condition from {guard:?}"));
        let def = ir
            .lines()
            .find(|l| l.trim_start().starts_with(&format!("{cond} = ")))
            .unwrap_or_else(|| panic!("no definition of {cond} in:\n{ir}"));
        assert!(
            def.contains("icmp eq i64"),
            "the write-back must be guarded on the binding still holding the pushed array, \
             got {def:?}"
        );
    }

    /// The predicate itself: a write nested inside the argument counts, a write
    /// to a DIFFERENT local does not, and a closure literal is answered
    /// conservatively because `walk_expr_children` does not descend into one.
    #[test]
    fn rebinding_predicate_sees_nested_writes_only_for_the_right_local() {
        let nested = Expr::Binary {
            op: perry_hir::BinaryOp::Add,
            left: Box::new(Expr::Number(1.0)),
            right: Box::new(Expr::LocalSet(7, Box::new(Expr::Number(2.0)))),
        };
        assert!(expr_rebinds_local(&nested, 7));
        assert!(!expr_rebinds_local(&nested, 8));
        assert!(expr_rebinds_local(
            &Expr::Update {
                id: 7,
                op: perry_hir::UpdateOp::Increment,
                prefix: false,
            },
            7
        ));
        assert!(!expr_rebinds_local(
            &Expr::Call {
                callee: Box::new(Expr::LocalGet(3)),
                args: vec![Expr::Number(1.0)],
                type_args: Vec::new(),
                byte_offset: 0,
            },
            7
        ));
    }
}

#[cfg(test)]
mod parent_gate_tests {
    use perry_hir::types::Type;
    use perry_hir::{Expr, Function, Module as HirModule, Stmt};

    /// `const a = []; a.push({v: 1});` — a pointer-valued push into a local
    /// array, which is the shape whose barrier #7511 gates.
    fn pushing_ir() -> String {
        let mut hir = HirModule::new("apush_parent_gate_test");
        hir.functions.push(Function {
            id: 0,
            name: "pushes".to_string(),
            type_params: Vec::new(),
            params: Vec::new(),
            return_type: Type::Any,
            body: vec![
                Stmt::Let {
                    id: 0,
                    name: "a".to_string(),
                    ty: Type::Any,
                    mutable: true,
                    init: Some(Expr::Array(Vec::new())),
                },
                Stmt::Expr(Expr::ArrayPush {
                    array_id: 0,
                    value: Box::new(Expr::Object(vec![("v".to_string(), Expr::Number(1.0))])),
                    field_writeback: None,
                }),
                Stmt::Return(Some(Expr::LocalGet(0))),
            ],
            is_async: false,
            is_generator: false,
            is_strict: true,
            is_exported: false,
            captures: Vec::new(),
            decorators: Vec::new(),
            was_plain_async: false,
            was_unrolled: false,
        });
        let opts = crate::CompileOptions {
            emit_ir_only: true,
            ..Default::default()
        };
        let bytes = crate::compile_module(&hir, opts).expect("test module compiles");
        String::from_utf8(bytes).expect("LLVM IR is UTF-8")
    }

    fn assert_default_barrier_env_not_disabled() {
        assert!(
            !matches!(
                std::env::var("PERRY_WRITE_BARRIERS").as_deref(),
                Ok("0") | Ok("off") | Ok("false")
            ),
            "this test describes DEFAULT barrier emission; PERRY_WRITE_BARRIERS must be unset or on"
        );
    }

    /// Block labels carry a uniquing suffix (`apush.barrier.21:`), so collect
    /// the gated block's body by walking labels rather than by substring —
    /// `apush.barrier.done.22:` would otherwise match a `apush.barrier.` prefix
    /// test and silently hand back the WRONG block, which is exactly the block
    /// the store is supposed to be in.
    fn gated_barrier_block(ir: &str) -> String {
        let mut body = Vec::new();
        let mut inside = false;
        for line in ir.lines() {
            if let Some(label) = line.strip_suffix(':') {
                if !label.starts_with(char::is_whitespace) {
                    inside = label.starts_with("apush.barrier.")
                        && !label.starts_with("apush.barrier.done");
                    continue;
                }
            }
            if inside {
                body.push(line);
            }
        }
        assert!(
            !body.is_empty(),
            "no `apush.barrier.<n>` block in the emitted IR — the push did not take the \
             gated inline tier, so this test would be vacuous:\n{ir}"
        );
        body.join("\n")
    }

    /// The barrier call must sit in its own block, reached only through the
    /// parent-generation `cond_br`, and both clauses of the gate must be
    /// present.
    #[test]
    fn array_push_barrier_is_gated_on_the_parent_header() {
        assert_default_barrier_env_not_disabled();
        let ir = pushing_ir();
        assert!(
            ir.contains("js_write_barrier_slot"),
            "the pointer-valued push must still emit a barrier at all:\n{ir}"
        );
        let gated = gated_barrier_block(&ir);
        assert!(
            gated.contains("js_write_barrier_slot"),
            "the gated block must be the one holding the barrier call:\n{gated}"
        );
        // Count CALL sites only — the module's `declare` line names the symbol
        // too, and counting it would make this compare 2 against 1 forever.
        assert_eq!(
            ir.matches("call void @js_write_barrier_slot").count(),
            gated.matches("call void @js_write_barrier_slot").count(),
            "every array-push barrier must be inside the gate — an ungated one would be the \
             cost this ticket exists to remove:\n{ir}"
        );
        assert_gate_condition_is_both_clauses(&ir);
    }

    /// Follow the `cond_br`'s condition back to its definition and require it to
    /// be the `or` of a `GC_FLAG_TENURED` header test and the incremental-count
    /// test.
    ///
    /// Checking only that the IR *contains* `and i8 …, 32` and the global's name
    /// is not enough, and this is not hypothetical: replacing the `or` with a
    /// constant-true left both of those substrings in place (the clauses are
    /// still computed, just no longer consulted) and the test stayed green while
    /// the gate had stopped gating. A branch that is always taken is precisely
    /// the failure this ticket's perf claim rests on not happening.
    fn assert_gate_condition_is_both_clauses(ir: &str) {
        let br = ir
            .lines()
            .find(|l| l.contains("br i1") && l.contains("label %apush.barrier."))
            .unwrap_or_else(|| panic!("no gated branch in the emitted IR:\n{ir}"));
        let cond = br
            .split_whitespace()
            .nth(2)
            .and_then(|c| c.strip_suffix(','))
            .unwrap_or_else(|| panic!("cannot read the branch condition from {br:?}"));
        let def = ir
            .lines()
            .find(|l| l.trim_start().starts_with(&format!("{cond} = ")))
            .unwrap_or_else(|| panic!("no definition of {cond} in:\n{ir}"));
        assert!(
            def.contains("or i1"),
            "the gate's branch condition must be the OR of both clauses, not {def:?} — a \
             condition that is not an `or` of the two tests is a gate that never skips"
        );
        let mut operands = def
            .split("or i1 ")
            .nth(1)
            .expect("or operands")
            .split(", ")
            .map(str::trim);
        let tenured = operands.next().expect("tenured operand");
        let incremental = operands.next().expect("incremental operand");
        let def_of = |name: &str| {
            ir.lines()
                .find(|l| l.trim_start().starts_with(&format!("{name} = ")))
                .unwrap_or_else(|| panic!("no definition of {name} in:\n{ir}"))
                .to_string()
        };
        assert!(
            def_of(tenured).contains("icmp ne i8"),
            "the first clause must be the parent's header-byte test, got {:?}",
            def_of(tenured)
        );
        assert!(
            def_of(incremental).contains("icmp ne i32"),
            "the second clause must be the incremental-count test, got {:?}",
            def_of(incremental)
        );
        assert!(
            ir.contains("and i8") && ir.contains(", 32"),
            "the header test must mask GC_FLAG_TENURED (0x20):\n{ir}"
        );
        assert!(
            ir.contains("@PERRY_INCREMENTAL_MARK_BARRIER_ACTIVE_COUNT"),
            "dropping the incremental clause would skip the insertion barrier's shading:\n{ir}"
        );
    }

    /// The SLOT STORE is unconditional: it must NOT be inside the gated block.
    /// Only the bookkeeping moves.
    #[test]
    fn array_push_slot_store_stays_outside_the_gate() {
        assert_default_barrier_env_not_disabled();
        let ir = pushing_ir();
        let gated = gated_barrier_block(&ir);
        assert!(
            !gated.contains("store double"),
            "the element store must stay OUTSIDE the gate — a store that only happens when the \
             parent is tenured would drop the value entirely:\n{gated}"
        );
    }
}
