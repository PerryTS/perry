//! ArrayPush / ArrayPushSpread.
//!
//! Extracted from `expr/mod.rs` to keep that file under the 2000-line cap.
//! Pure mechanical move — match arm bodies are verbatim copies, called from
//! `lower_expr`'s outer dispatch.

use anyhow::{anyhow, Result};
use perry_hir::Expr;

use crate::nanbox::double_literal;
use crate::native_value::{
    BoundsState, BufferAccessMode, ExpectedNativeRep, LoweredValue, MaterializationReason,
    NativeRep, SemanticKind,
};
use crate::type_analysis::is_numeric_expr;
use crate::types::{DOUBLE, I1, I16, I32, I64, I8};

use super::{
    array_store_needs_layout_note, array_store_needs_write_barrier,
    emit_array_numeric_write_note_on_block, emit_jsvalue_slot_store_with_flags_on_block,
    emit_jsvalue_slot_store_with_value_bits_on_block, emit_root_nanbox_store_on_block,
    emit_typed_feedback_register_site, emit_write_barrier,
    emit_write_barrier_slot_generation_tested, expr_has_numeric_pointer_free_array_layout,
    lower_expr, lower_expr_native, nanbox_pointer_inline, raw_f64_layout_fact, unbox_to_i64, FnCtx,
    TypedFeedbackContract, TypedFeedbackKind,
};

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
    blk.sitofp(I32, &len_i32, DOUBLE)
}

fn emit_array_box_length(ctx: &mut FnCtx<'_>, array_box: &str, value_discarded: bool) -> String {
    if value_discarded {
        return double_literal(0.0);
    }
    let blk = ctx.block();
    let array_handle = unbox_to_i64(blk, array_box);
    emit_array_handle_length(ctx, &array_handle, false)
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

pub(crate) fn lower(ctx: &mut FnCtx<'_>, expr: &Expr, value_discarded: bool) -> Result<String> {
    match expr {
        Expr::ArrayPush { array_id, value } => {
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
            let value_is_numeric = is_numeric_expr(ctx, value);
            let require_numeric_layout =
                value_is_numeric && expr_has_numeric_pointer_free_array_layout(ctx, &array_expr);
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
            let keep_guarded_numeric_push = super::typed_feedback_emission_enabled()
                || !crate::type_analysis::expr_produces_canonical_raw_f64(ctx, value);
            if require_numeric_layout
                && keep_guarded_numeric_push
                && !ctx.boxed_vars.contains(array_id)
                && !ctx.closure_captures.contains_key(array_id)
                && ctx.locals.contains_key(array_id)
            {
                let slot = ctx.locals.get(array_id).cloned().unwrap();
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
                    blk.store(DOUBLE, &new_box, &slot);
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
                    blk.call_void(
                        "js_typed_feedback_record_fallback_call",
                        &[(I64, &feedback_site_id)],
                    );
                    let arr_handle = unbox_to_i64(blk, &arr_box);
                    let new_handle = blk.call(
                        I64,
                        "js_array_push_f64",
                        &[(I64, &arr_handle), (DOUBLE, &v)],
                    );
                    let new_box = nanbox_pointer_inline(blk, &new_handle);
                    blk.store(DOUBLE, &new_box, &slot);
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
                    "js_array_push_f64",
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
                let current_box = ctx.block().load(DOUBLE, &slot);
                return Ok(emit_array_box_length(ctx, &current_box, false));
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
            if !ctx.boxed_vars.contains(array_id)
                && !ctx.closure_captures.contains_key(array_id)
                && ctx.locals.contains_key(array_id)
            {
                let slot = ctx.locals.get(array_id).cloned().unwrap();
                let blk = ctx.block();
                let arr_handle = unbox_to_i64(blk, &arr_box);

                // Issue #233: forwarded arrays must follow the
                // forwarding chain. Route through the runtime which
                // calls clean_arr_ptr_mut and writes into the live
                // head — the inline path's offset-0 length read would
                // otherwise pick up the lower 32 bits of the
                // forwarding pointer (garbage).
                let gc_flags_addr = blk.sub(I64, &arr_handle, "7");
                let gc_flags_ptr = blk.inttoptr(I64, &gc_flags_addr);
                let gc_flags = blk.load(I8, &gc_flags_ptr);
                let fwd_bits = blk.and(I8, &gc_flags, "128");
                let is_fwd = blk.icmp_ne(I8, &fwd_bits, "0");

                let fwd_idx = ctx.new_block("apush.fwd");
                let nofwd_idx = ctx.new_block("apush.nofwd");
                let inbounds_idx = ctx.new_block("apush.inbounds");
                let realloc_idx = ctx.new_block("apush.realloc");
                let merge_idx = ctx.new_block("apush.merge");

                let fwd_label = ctx.block_label(fwd_idx);
                let nofwd_label = ctx.block_label(nofwd_idx);
                let inbounds_label = ctx.block_label(inbounds_idx);
                let realloc_label = ctx.block_label(realloc_idx);
                let merge_label = ctx.block_label(merge_idx);

                ctx.block().cond_br(&is_fwd, &fwd_label, &nofwd_label);

                // FORWARDED branch: route through runtime.
                ctx.current_block = fwd_idx;
                {
                    let blk = ctx.block();
                    let new_handle = blk.call(
                        I64,
                        "js_array_push_f64",
                        &[(I64, &arr_handle), (DOUBLE, &v)],
                    );
                    let new_box = nanbox_pointer_inline(blk, &new_handle);
                    blk.store(DOUBLE, &new_box, &slot);
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
                ctx.current_block = nofwd_idx;
                {
                    let blk = ctx.block();
                    let flags_addr = blk.sub(I64, &arr_handle, "6");
                    let flags_ptr = blk.inttoptr(I64, &flags_addr);
                    let obj_flags = blk.load(I16, &flags_ptr);
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
                    } else {
                        // FROZEN(0x1)|SEALED(0x2)|NO_EXTEND(0x4)|ARRAY_DESCRIPTORS(0x400).
                        let integrity_bits = blk.and(I16, &obj_flags, "1031");
                        blk.icmp_eq(I16, &integrity_bits, "0")
                    };
                    let length = blk.safe_load_i32_from_ptr(&arr_handle);
                    let cap_addr = blk.add(I64, &arr_handle, "4");
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
                let (length, element_addr, barrier_value_bits) = {
                    let blk = ctx.block();
                    let length = blk.safe_load_i32_from_ptr(&arr_handle);
                    let length_i64 = blk.zext(I32, &length, I64);
                    let byte_offset = blk.shl(I64, &length_i64, "3");
                    let with_header = blk.add(I64, &byte_offset, "8");
                    let element_addr = blk.add(I64, &arr_handle, &with_header);
                    let element_ptr = blk.inttoptr(I64, &element_addr);
                    let value_bits = if let Some(value_bits) = v_bits.as_deref() {
                        emit_jsvalue_slot_store_with_value_bits_on_block(
                            blk,
                            &element_ptr,
                            &v,
                            value_bits,
                            &arr_handle,
                            &length,
                            string_addref_needed,
                            layout_note_needed,
                            &arr_handle,
                            &element_addr,
                            false,
                        )
                    } else {
                        emit_jsvalue_slot_store_with_flags_on_block(
                            blk,
                            &element_ptr,
                            &v,
                            &arr_handle,
                            &length,
                            string_addref_needed,
                            layout_note_needed,
                            &arr_handle,
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
                    if !value_is_numeric && !declared_all_pointer {
                        let value_bits = barrier_value_bits
                            .clone()
                            .or(value_bits)
                            .unwrap_or_else(|| blk.bitcast_double_to_i64(&v));
                        emit_array_numeric_write_note_on_block(blk, &arr_handle, &value_bits);
                    }
                    (length, element_addr, barrier_value_bits)
                };
                if let Some(child_bits) = barrier_value_bits {
                    // `arr_handle` reached this block through the `nofwd` header
                    // test, so it is a live, non-forwarded GC array user
                    // pointer — the precondition for reading its header byte.
                    emit_write_barrier_slot_generation_tested(
                        ctx,
                        &arr_handle,
                        &arr_handle,
                        &element_addr,
                        &child_bits,
                        "apush",
                    );
                }
                {
                    let blk = ctx.block();
                    let new_length = blk.add(I32, &length, "1");
                    let arr_ptr = blk.inttoptr(I64, &arr_handle);
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
                        "js_array_push_f64",
                        &[(I64, &arr_handle), (DOUBLE, &v)],
                    );
                    let new_box = nanbox_pointer_inline(blk, &new_handle);
                    blk.store(DOUBLE, &new_box, &slot);
                    blk.br(&merge_label);
                }

                ctx.current_block = merge_idx;
                if value_discarded {
                    // Skip the slot reload too — it only feeds the length.
                    return Ok(double_literal(0.0));
                }
                let current_box = ctx.block().load(DOUBLE, &slot);
                return Ok(emit_array_box_length(ctx, &current_box, false));
            }

            let blk = ctx.block();
            let arr_handle = unbox_to_i64(blk, &arr_box);
            let new_handle = blk.call(
                I64,
                "js_array_push_f64",
                &[(I64, &arr_handle), (DOUBLE, &v)],
            );
            let new_box = nanbox_pointer_inline(blk, &new_handle);
            // Write back to whichever storage backs the local.
            // Boxed var takes priority: write through the box so
            // every closure sharing the box sees the new pointer.
            if ctx.boxed_vars.contains(array_id) {
                // Captured-through-closure boxed var.
                if let Some(&capture_idx) = ctx.closure_captures.get(array_id) {
                    let closure_ptr =
                        super::current_closure_ptr_value(ctx, "ArrayPush boxed captured")?;
                    let idx_str = capture_idx.to_string();
                    let blk = ctx.block();
                    let box_ptr = blk.call(
                        I64,
                        "js_closure_get_capture_bits",
                        &[(I64, &closure_ptr), (I32, &idx_str)],
                    );
                    let new_bits = blk.bitcast_double_to_i64(&new_box);
                    blk.call_void("js_box_set_bits", &[(I64, &box_ptr), (I64, &new_bits)]);
                    // Gen-GC Phase C2: the realloc'd array head is a (possibly
                    // young) heap pointer stored into an existing box — barrier
                    // the box parent so a minor GC can't miss it.
                    emit_write_barrier(ctx, &box_ptr, &new_bits);
                    // The capture slot holds the BOX pointer; the box content is
                    // the shared storage every closure sees. Return here — do NOT
                    // fall through to the `closure_set_capture_bits` store below,
                    // which would clobber the box pointer in the capture slot with
                    // the array pointer, so the next push would treat the array as
                    // the box and silently lose the realloc write-back.
                    return Ok(emit_array_handle_length(ctx, &new_handle, value_discarded));
                } else if let Some(slot) = ctx.locals.get(array_id).cloned() {
                    let blk = ctx.block();
                    let box_ptr = blk.load(I64, &slot);
                    let new_bits = blk.bitcast_double_to_i64(&new_box);
                    blk.call_void("js_box_set_bits", &[(I64, &box_ptr), (I64, &new_bits)]);
                    // Gen-GC Phase C2: barrier the box parent (see capture path).
                    emit_write_barrier(ctx, &box_ptr, &new_bits);
                    // The slot holds the BOX pointer — the box is the shared
                    // storage. Return so the slot keeps pointing at the box (see
                    // the captured branch above).
                    return Ok(emit_array_handle_length(ctx, &new_handle, value_discarded));
                }
                // #5459: `array_id` is in `boxed_vars` but has no box location in
                // THIS context — it's a module-level global accessed directly from
                // a nested function (the load path read `@global`, not a box-get).
                // Returning here would skip the realloc write-back entirely, so the
                // relocated array header is never stored to the registered GC-root
                // global slot: the old head is freed on the next GC and the global
                // dangles (use-after-free / corrupted length). Fall through to the
                // module-global store-back below instead of returning.
            }
            if let Some(&capture_idx) = ctx.closure_captures.get(array_id) {
                let closure_ptr = super::current_closure_ptr_value(ctx, "ArrayPush captured")?;
                let idx_str = capture_idx.to_string();
                let new_bits = ctx.block().bitcast_double_to_i64(&new_box);
                ctx.block().call_void(
                    "js_closure_set_capture_bits",
                    &[(I64, &closure_ptr), (I32, &idx_str), (I64, &new_bits)],
                );
                // Gen-GC Phase C2: the realloc'd array head stored into the
                // closure capture is a (possibly young) heap pointer — barrier
                // the closure parent.
                emit_write_barrier(ctx, &closure_ptr, &new_bits);
            } else if let Some(slot) = ctx.locals.get(array_id).cloned() {
                ctx.block().store(DOUBLE, &new_box, &slot);
            } else if let Some(global_name) = ctx.module_globals.get(array_id).cloned() {
                let g_ref = format!("@{}", global_name);
                // GC_STORE_AUDIT(ROOT): module global array slot is a registered mutable GC root.
                emit_root_nanbox_store_on_block(ctx.block(), &new_box, &g_ref);
            } else {
                return Err(anyhow!("ArrayPush({}): local not in scope", array_id));
            }
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
            let src_box = lower_expr(ctx, source)?;
            let arr_box = lower_expr(ctx, &Expr::LocalGet(*array_id))?;
            let blk = ctx.block();
            let dst_handle = unbox_to_i64(blk, &arr_box);
            let src_handle = unbox_to_i64(blk, &src_box);
            let new_handle = blk.call(
                I64,
                "js_array_concat",
                &[(I64, &dst_handle), (I64, &src_handle)],
            );
            let new_box = nanbox_pointer_inline(blk, &new_handle);
            if ctx.boxed_vars.contains(array_id) {
                if let Some(&capture_idx) = ctx.closure_captures.get(array_id) {
                    let closure_ptr =
                        super::current_closure_ptr_value(ctx, "ArrayPushSpread boxed captured")?;
                    let idx_str = capture_idx.to_string();
                    let blk = ctx.block();
                    let box_ptr = blk.call(
                        I64,
                        "js_closure_get_capture_bits",
                        &[(I64, &closure_ptr), (I32, &idx_str)],
                    );
                    let new_bits = blk.bitcast_double_to_i64(&new_box);
                    blk.call_void("js_box_set_bits", &[(I64, &box_ptr), (I64, &new_bits)]);
                    // Gen-GC Phase C2: the realloc'd array head is a (possibly
                    // young) heap pointer stored into an existing box — barrier
                    // the box parent so a minor GC can't miss it.
                    emit_write_barrier(ctx, &box_ptr, &new_bits);
                    // Box content is the shared storage; the capture slot must keep
                    // pointing at the box. Return so we don't fall through to the
                    // capture-slot store, which would clobber the box pointer (see
                    // the matching note in `Expr::ArrayPush`).
                    return Ok(emit_array_handle_length(ctx, &new_handle, value_discarded));
                } else if let Some(slot) = ctx.locals.get(array_id).cloned() {
                    let blk = ctx.block();
                    let box_ptr = blk.load(I64, &slot);
                    let new_bits = blk.bitcast_double_to_i64(&new_box);
                    blk.call_void("js_box_set_bits", &[(I64, &box_ptr), (I64, &new_bits)]);
                    // Gen-GC Phase C2: barrier the box parent (see capture path).
                    emit_write_barrier(ctx, &box_ptr, &new_bits);
                    return Ok(emit_array_handle_length(ctx, &new_handle, value_discarded));
                }
                // #5459: in `boxed_vars` but no box location here — a module-level
                // global accessed directly from a nested function. Fall through to
                // the module-global store-back so the relocated head reaches the
                // GC-root slot (see the matching note in `Expr::ArrayPush`).
            }
            if let Some(&capture_idx) = ctx.closure_captures.get(array_id) {
                let closure_ptr =
                    super::current_closure_ptr_value(ctx, "ArrayPushSpread captured")?;
                let idx_str = capture_idx.to_string();
                let new_bits = ctx.block().bitcast_double_to_i64(&new_box);
                ctx.block().call_void(
                    "js_closure_set_capture_bits",
                    &[(I64, &closure_ptr), (I32, &idx_str), (I64, &new_bits)],
                );
                // Gen-GC Phase C2: the realloc'd array head stored into the
                // closure capture is a (possibly young) heap pointer — barrier
                // the closure parent.
                emit_write_barrier(ctx, &closure_ptr, &new_bits);
            } else if let Some(slot) = ctx.locals.get(array_id).cloned() {
                ctx.block().store(DOUBLE, &new_box, &slot);
            } else if let Some(global_name) = ctx.module_globals.get(array_id).cloned() {
                let g_ref = format!("@{}", global_name);
                // GC_STORE_AUDIT(ROOT): module global array slot is a registered mutable GC root.
                emit_root_nanbox_store_on_block(ctx.block(), &new_box, &g_ref);
            } else {
                return Err(anyhow!("ArrayPushSpread({}): local not in scope", array_id));
            }
            Ok(emit_array_handle_length(ctx, &new_handle, value_discarded))
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
