//! Array index-set fast-path lowering (extracted from `expr.rs`, issue
//! #1098). Pure move — no logic changes.

use anyhow::{anyhow, Result};

use super::{
    emit_array_numeric_write_note_on_block, emit_jsvalue_slot_store_on_block,
    emit_jsvalue_slot_store_scalar_aware_on_block, emit_write_barrier_slot_on_block,
    nanbox_pointer_inline, raw_f64_layout_fact, FnCtx,
};
use crate::block::LlBlock;
use crate::nanbox::POINTER_MASK_I64;
use crate::native_value::{
    BoundsState, BufferAccessMode, LoweredValue, MaterializationReason, NativeRep, SemanticKind,
};
use crate::types::{DOUBLE, I1, I16, I32, I64, I8};

fn canonicalize_raw_f64_numeric_store_value(blk: &mut LlBlock, value_double: &str) -> String {
    blk.call(
        DOUBLE,
        "js_array_numeric_value_to_raw_f64",
        &[(DOUBLE, value_double)],
    )
}

/// Inline fast-path lowering for `local_arr[i] = v`.
///
/// Compiles to:
///
/// ```text
///   <current>:
///     %arr_handle = unbox(arr_box)
///     %idx_i32 = fptosi %idx
///     %guard_ok = call @js_typed_feedback_plain_array_index_set_guard(...)
///     br i1 %guard_ok, label %guarded, label %fallback
///
///   <guarded>:
///     %length = load i32, ptr @ arr_handle+0
///     %in_bounds = icmp ult %idx_i32, %length
///     br i1 %in_bounds, label %fast_inbounds, label %check_capacity
///
///   fast_inbounds:
///     ; element_ptr = arr_handle + 8 + idx*8
///     store double %v, ptr %element_ptr
///     br merge
///
///   check_capacity:
///     %capacity = load i32, ptr @ arr_handle+4
///     %within_cap = icmp ult %idx_i32, %capacity
///     %dense_append = icmp eq %idx_i32, %length
///     %can_extend_inline = and %within_cap, %dense_append
///     br i1 %can_extend_inline, label %extend_inline, label %runtime_extend
///
///   extend_inline:
///     store double %v, ptr %element_ptr
///     %new_len = add i32 %idx, 1
///     store i32 %new_len, ptr @ arr_handle+0
///     br merge
///
///   runtime_extend:
///     %new_handle = call i64 @js_array_set_f64_extend(...)
///     %new_box = nanbox_pointer_inline(new_handle)
///     store double %new_box, ptr %local_slot
///     br merge
///
///   fallback:
///     %new_box = call double @js_typed_feedback_array_index_set_fallback_boxed(...)
///     store double %new_box, ptr %local_slot
///     br merge
///
///   merge:
///     <continues here>
/// ```
///
/// The inline store paths are entered only after the runtime guard proves the
/// receiver is a live, non-forwarded plain array with a sane header. The realloc
/// path also handles sparse extensions so holes are filled and numeric raw
/// layout is downgraded before JavaScript can observe the gap.
pub(crate) fn lower_index_set_fast(
    ctx: &mut FnCtx<'_>,
    arr_box: &str,
    idx_double: &str,
    val_double: &str,
    local_id: u32,
    layout_note_needed: bool,
    write_barrier_needed: bool,
    value_is_numeric: bool,
    require_numeric_layout: bool,
    // Repsel 4a.0: RHS proven canonical-raw-f64 by
    // `expr_produces_canonical_raw_f64` — the slot store may skip the
    // `js_array_numeric_value_to_raw_f64` canonicalization call entirely.
    value_is_canonical_raw_f64: bool,
    feedback_site_id: &str,
) -> Result<()> {
    // Capture the local slot for the realloc path.
    let slot = ctx
        .locals
        .get(&local_id)
        .ok_or_else(|| anyhow!("IndexSet: local {} not in scope", local_id))?
        .clone();

    // Unbox the array pointer.
    let blk = ctx.block();
    let arr_bits = blk.bitcast_double_to_i64(arr_box);
    let arr_handle = blk.and(I64, &arr_bits, POINTER_MASK_I64);
    let idx_i32 = blk.fptosi(DOUBLE, idx_double, I32);

    let guarded_idx = ctx.new_block("idxset.guarded");
    let guard_fallback_idx = ctx.new_block("idxset.guard_fallback");
    let inbounds_idx = ctx.new_block("idxset.inbounds");
    let check_cap_idx = ctx.new_block("idxset.check_cap");
    let extend_inline_idx = ctx.new_block("idxset.extend_inline");
    let realloc_idx = ctx.new_block("idxset.realloc");
    let merge_idx = ctx.new_block("idxset.merge");

    let guarded_label = ctx.block_label(guarded_idx);
    let guard_fallback_label = ctx.block_label(guard_fallback_idx);
    let inbounds_label = ctx.block_label(inbounds_idx);
    let check_cap_label = ctx.block_label(check_cap_idx);
    let extend_inline_label = ctx.block_label(extend_inline_idx);
    let realloc_label = ctx.block_label(realloc_idx);
    let merge_label = ctx.block_label(merge_idx);

    // Runtime guard before any ArrayHeader read or raw element store. This
    // rejects dynamic/cross-boundary receivers, lazy arrays, stale forwarded
    // heads, and corrupt layouts; the fallback then uses boxed JSValue
    // semantics and writes the returned receiver back to the local slot.
    //
    // Repsel 4a.1: the numeric WRITE gets an inline first tier mirroring the
    // read side — the structural facts (array type, no forwarding, integrity
    // + descriptor bits, prototype-chain byte, header sanity) plus the
    // `GC_ARRAY_RAW_F64_LAYOUT` dense bit live in two header bytes and one
    // sticky global. It only applies when the RHS is canonical-raw-f64 by
    // construction (`expr_produces_canonical_raw_f64`): the out-of-line
    // guard's remaining job on such values is exactly these header tests
    // (its `is_numeric_value_bits(value)` leg is statically true). Guard
    // misses fall to the existing out-of-line guard, whose first-touch path
    // rebuilds unmarked numeric arrays (setting the dense flag), so the
    // steady state is call-free. Feedback-emission builds keep the
    // out-of-line guard for observation coverage.
    let inline_write_tier = require_numeric_layout
        && value_is_canonical_raw_f64
        && !super::typed_feedback_emission_enabled();
    let cold_guard_idx = if inline_write_tier {
        Some(ctx.new_block("idxset.guard.cold"))
    } else {
        None
    };
    if inline_write_tier {
        let cold_label = ctx.block_label(cold_guard_idx.unwrap());
        let deref_idx = ctx.new_block("idxset.guard.deref");
        let deref_label = ctx.block_label(deref_idx);
        {
            let blk = ctx.block();
            let tag = blk.lshr(I64, &arr_bits, "48");
            let is_pointer = blk.icmp_eq(I64, &tag, "32765"); // POINTER_TAG
            let above_handle_band = blk.icmp_ugt(I64, &arr_handle, "1048575");
            let heap_candidate = blk.and(I1, &is_pointer, &above_handle_band);
            blk.cond_br(&heap_candidate, &deref_label, &cold_label);
        }
        ctx.current_block = deref_idx;
        {
            let blk = ctx.block();
            let gc_type_addr = blk.sub(I64, &arr_handle, "8");
            let gc_type_ptr = blk.inttoptr(I64, &gc_type_addr);
            let gc_type = blk.load(I8, &gc_type_ptr);
            let is_array = blk.icmp_eq(I8, &gc_type, "1"); // GC_TYPE_ARRAY

            let gc_flags_addr = blk.sub(I64, &arr_handle, "7");
            let gc_flags_ptr = blk.inttoptr(I64, &gc_flags_addr);
            let gc_flags = blk.load(I8, &gc_flags_ptr);
            let forwarded_bits = blk.and(I8, &gc_flags, "128");
            let not_forwarded = blk.icmp_eq(I8, &forwarded_bits, "0");

            // FROZEN(0x1)|SEALED(0x2)|NO_EXTEND(0x4)|ARRAY_DESCRIPTORS(0x400):
            // integrity/descriptor-carrying arrays route through the runtime
            // (writes may throw in strict mode / dispatch accessor setters).
            let reserved_addr = blk.sub(I64, &arr_handle, "6");
            let reserved_ptr = blk.inttoptr(I64, &reserved_addr);
            let reserved = blk.load(I16, &reserved_ptr);
            let integrity_bits = blk.and(I16, &reserved, "1031"); // 0x407
            let integrity_clean = blk.icmp_eq(I16, &integrity_bits, "0");
            // Repsel 4a.2: accept EITHER raw-f64 invariant — dense
            // (GC_ARRAY_RAW_F64_LAYOUT, 0x80) or raw-f64-or-holes
            // (GC_ARRAY_RAW_F64_HOLES, 0x1000). A canonical-numeric store
            // preserves both invariants, and the extend arm below maintains
            // the flag transition when it creates holes. This is what lets a
            // `new Array(n)` mid-fill histogram write inline (the runtime
            // set guard rejects holey arrays outright).
            let dense_bits = blk.and(I16, &reserved, "4224"); // 0x1080
            let is_dense = blk.icmp_ne(I16, &dense_bits, "0");

            let invalidated = blk.load_volatile(I8, "@PERRY_ARRAY_INDEX_FAST_PATH_INVALIDATED");
            let default_prototype_chain = blk.icmp_eq(I8, &invalidated, "0");

            let arr_ptr = blk.inttoptr(I64, &arr_handle);
            let hdr_length = blk.load(I32, &arr_ptr);
            let cap_addr = blk.add(I64, &arr_handle, "4");
            let cap_ptr = blk.inttoptr(I64, &cap_addr);
            let hdr_capacity = blk.load(I32, &cap_ptr);
            let index_nonnegative = blk.icmp_slt(I32, &idx_i32, "0");
            let index_nonnegative = blk.icmp_eq(I1, &index_nonnegative, "false");
            let length_sane = blk.icmp_ule(I32, &hdr_length, "16000000");
            let capacity_sane = blk.icmp_ule(I32, &hdr_capacity, "16000000");
            let length_within_capacity = blk.icmp_ule(I32, &hdr_length, &hdr_capacity);

            let mut guard_ok = blk.and(I1, &is_array, &not_forwarded);
            guard_ok = blk.and(I1, &guard_ok, &integrity_clean);
            guard_ok = blk.and(I1, &guard_ok, &is_dense);
            guard_ok = blk.and(I1, &guard_ok, &default_prototype_chain);
            guard_ok = blk.and(I1, &guard_ok, &index_nonnegative);
            guard_ok = blk.and(I1, &guard_ok, &length_sane);
            guard_ok = blk.and(I1, &guard_ok, &capacity_sane);
            guard_ok = blk.and(I1, &guard_ok, &length_within_capacity);
            blk.cond_br(&guard_ok, &guarded_label, &cold_label);
        }
    }
    if let Some(cold_idx) = cold_guard_idx {
        ctx.current_block = cold_idx;
        // Repsel 4a.2 (#6904): self-heal a stale growth-forwarded binding —
        // follow the chain and write the live head back to the local slot
        // (safe: this fast path is only taken for a plain stack local, and
        // the fallback below already stores boxed heads into the same slot).
        // This iteration still guards/falls back on the ORIGINAL value
        // (chain-following keeps it correct); the NEXT iteration re-loads
        // the repaired slot and takes the inline tier.
        let blk = ctx.block();
        let fresh = blk.call(DOUBLE, "js_array_refresh_local_head", &[(DOUBLE, arr_box)]);
        blk.store(DOUBLE, &fresh, &slot);
    }
    let guard_ok = {
        let blk = ctx.block();
        let guard_fn = if require_numeric_layout {
            "js_typed_feedback_numeric_array_index_set_guard"
        } else {
            "js_typed_feedback_plain_array_index_set_guard"
        };
        let guard_i32 = blk.call(
            I32,
            guard_fn,
            &[
                (I64, feedback_site_id),
                (DOUBLE, arr_box),
                (I32, &idx_i32),
                (DOUBLE, val_double),
                (I32, "0"),
            ],
        );
        blk.icmp_ne(I32, &guard_i32, "0")
    };
    ctx.block()
        .cond_br(&guard_ok, &guarded_label, &guard_fallback_label);

    ctx.current_block = guard_fallback_idx;
    {
        let fallback_box = ctx.block().call(
            DOUBLE,
            "js_typed_feedback_array_index_set_fallback_boxed",
            &[
                (I64, feedback_site_id),
                (DOUBLE, arr_box),
                (DOUBLE, idx_double),
                (DOUBLE, val_double),
            ],
        );
        ctx.block().store(DOUBLE, &fallback_box, &slot);
        ctx.block().br(&merge_label);
        if require_numeric_layout {
            let fallback = LoweredValue {
                semantic: SemanticKind::JsValue,
                rep: NativeRep::JsValue,
                llvm_ty: DOUBLE,
                value: fallback_box,
            };
            ctx.record_lowered_value_with_access_mode_and_facts(
                "NumericArrayIndexSet",
                Some(local_id),
                "js_typed_feedback_array_index_set_fallback_boxed",
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
                        Some(local_id),
                        "rejected",
                        "numeric_array_index_set_guard",
                        Some(MaterializationReason::RuntimeApi),
                    ),
                    raw_f64_layout_fact(
                        Some(local_id),
                        "invalidated",
                        "runtime_api",
                        Some(MaterializationReason::RuntimeApi),
                    ),
                ],
                false,
                false,
                Vec::new(),
            );
        }
    }

    ctx.current_block = guarded_idx;
    // Load length from offset 0 (null-guarded).
    let length = ctx.block().safe_load_i32_from_ptr(&arr_handle);
    let in_bounds = ctx.block().icmp_ult(I32, &idx_i32, &length);
    ctx.block()
        .cond_br(&in_bounds, &inbounds_label, &check_cap_label);

    // Helper: compute element_ptr = arr_ptr + 8 + idx*8.
    fn element_slot(blk: &mut LlBlock, arr_handle: &str, idx_i32: &str) -> (String, String) {
        let idx_i64 = blk.zext(I32, idx_i32, I64);
        let byte_offset = blk.shl(I64, &idx_i64, "3"); // *8
        let with_header = blk.add(I64, &byte_offset, "8"); // +8 for header
        let element_addr = blk.add(I64, arr_handle, &with_header);
        let element_ptr = blk.inttoptr(I64, &element_addr);
        (element_addr, element_ptr)
    }

    // FASTEST: in-bounds path. Store directly, jump to merge.
    ctx.current_block = inbounds_idx;
    {
        let blk = ctx.block();
        let (element_addr, element_ptr) = element_slot(blk, &arr_handle, &idx_i32);
        if require_numeric_layout {
            // GC_STORE_AUDIT(POINTER_FREE): require_numeric_layout proves the
            // array is raw-f64 and the value is canonicalized to a plain f64 —
            // no GC pointer is written into the slot, so no write barrier.
            if value_is_canonical_raw_f64 {
                // Repsel 4a.0: the RHS is canonical by construction (literal /
                // arithmetic / Math.* / coerce chain) — store verbatim.
                blk.store(DOUBLE, val_double, &element_ptr);
            } else {
                // GC_STORE_AUDIT(POINTER_FREE): js_array_numeric_value_to_raw_f64
                // returns a plain unboxed f64 — no GC pointer, so no barrier.
                let numeric_value = canonicalize_raw_f64_numeric_store_value(blk, val_double);
                blk.store(DOUBLE, &numeric_value, &element_ptr);
            }
        } else {
            // In-place overwrite of a non-raw-layout (e.g. downgraded `any[]`)
            // array element: the slot holds a valid value, so the scalar-aware
            // note skips the GC layout hashmap on scalar-over-scalar stores
            // (#5094 — ~9× on bench_numeric_array_downgrade).
            let value_bits = emit_jsvalue_slot_store_scalar_aware_on_block(
                blk,
                &element_ptr,
                val_double,
                &arr_handle,
                &idx_i32,
                layout_note_needed,
                &arr_handle,
                &element_addr,
                write_barrier_needed,
            )
            .unwrap_or_else(|| blk.bitcast_double_to_i64(val_double));
            if !value_is_numeric {
                emit_array_numeric_write_note_on_block(blk, &arr_handle, &value_bits);
            }
        }
        blk.br(&merge_label);
    }
    if require_numeric_layout {
        let stored = LoweredValue {
            semantic: SemanticKind::JsNumber,
            rep: NativeRep::F64,
            llvm_ty: DOUBLE,
            value: val_double.to_string(),
        };
        ctx.record_lowered_value_with_access_mode_and_facts(
            "NumericArrayIndexSet",
            Some(local_id),
            "js_array_numeric_set_f64_unboxed",
            &stored,
            Some(BoundsState::Guarded {
                guard_id: "numeric_array_index_set_guard".to_string(),
            }),
            None,
            Some(BufferAccessMode::CheckedNative),
            None,
            None,
            None,
            vec![raw_f64_layout_fact(
                Some(local_id),
                "consumed",
                "numeric_array_index_set_guard",
                None,
            )],
            Vec::new(),
            false,
            false,
            Vec::new(),
        );
    }

    // MEDIUM: idx == length and idx < capacity. Store + bump length.
    // Sparse writes must go through `js_array_set_f64_extend` so gaps become
    // TAG_HOLE and raw numeric layout is downgraded before user-visible reads.
    ctx.current_block = check_cap_idx;
    let capacity = {
        let blk = ctx.block();
        // Load capacity from offset 4 — we need a typed pointer that
        // points 4 bytes into the array header. Use inttoptr after add.
        let cap_addr = blk.add(I64, &arr_handle, "4");
        let cap_ptr = blk.inttoptr(I64, &cap_addr);
        blk.load(I32, &cap_ptr)
    };
    // Repsel 4a.2: the widened (hole-filling) extend arm is emitted only in
    // non-feedback builds — feedback builds keep the previous shape (dense
    // append inline, sparse extends via the recorded runtime arm) so their
    // observation stream is unchanged.
    let widened_numeric_extend =
        require_numeric_layout && !super::typed_feedback_emission_enabled();
    let can_extend_inline = {
        let blk = ctx.block();
        let within_cap = blk.icmp_ult(I32, &idx_i32, &capacity);
        if widened_numeric_extend {
            // Repsel 4a.2: widen the inline arm from `idx == length` (dense
            // append) to any in-capacity extend. The gap `[length, idx)` is
            // raw-TAG_HOLE-filled inline (pointer-free by construction — no
            // per-slot GC notes or barriers needed under the raw-f64 layout
            // proof), and the header flags transition dense→holes when a gap
            // was actually created. Only `idx >= capacity` pays the runtime
            // grow call. `check_cap` is only reached with `idx >= length`
            // (the in-bounds branch tested `idx < length`; negative indices
            // were rejected by both guard tiers), so `within_cap` alone
            // decides.
            within_cap
        } else {
            let dense_append = blk.icmp_eq(I32, &idx_i32, &length);
            blk.and(I1, &within_cap, &dense_append)
        }
    };
    ctx.block()
        .cond_br(&can_extend_inline, &extend_inline_label, &realloc_label);

    ctx.current_block = extend_inline_idx;
    if widened_numeric_extend {
        // Hole-fill loop: for (j = length; j < idx; j++) slot[j] = TAG_HOLE.
        // The counter lives in an entry-block alloca (a non-entry alloca
        // inside a user loop would leak stack per iteration — #167 class);
        // mem2reg rewrites it to a phi.
        let fill_slot = ctx.func.alloca_entry(I32);
        ctx.block().store(I32, &length, &fill_slot);
        let fill_cond_idx = ctx.new_block("idxset.fill.cond");
        let fill_body_idx = ctx.new_block("idxset.fill.body");
        let fill_done_idx = ctx.new_block("idxset.fill.done");
        let fill_cond_label = ctx.block_label(fill_cond_idx);
        let fill_body_label = ctx.block_label(fill_body_idx);
        let fill_done_label = ctx.block_label(fill_done_idx);
        ctx.block().br(&fill_cond_label);

        ctx.current_block = fill_cond_idx;
        {
            let blk = ctx.block();
            let j = blk.load(I32, &fill_slot);
            let more = blk.icmp_ult(I32, &j, &idx_i32);
            blk.cond_br(&more, &fill_body_label, &fill_done_label);
        }

        ctx.current_block = fill_body_idx;
        {
            let blk = ctx.block();
            let j = blk.load(I32, &fill_slot);
            let (_, hole_ptr) = element_slot(blk, &arr_handle, &j);
            let hole_d = blk.bitcast_i64_to_double(crate::nanbox::TAG_HOLE_I64);
            // GC_STORE_AUDIT(POINTER_FREE): TAG_HOLE sentinel under the
            // raw-f64 layout proof — pointer-free, no note, no barrier.
            blk.store(DOUBLE, &hole_d, &hole_ptr);
            let j_next = blk.add(I32, &j, "1");
            blk.store(I32, &j_next, &fill_slot);
            blk.br(&fill_cond_label);
        }

        ctx.current_block = fill_done_idx;
        {
            let blk = ctx.block();
            let (_, element_ptr) = element_slot(blk, &arr_handle, &idx_i32);
            // GC_STORE_AUDIT(POINTER_FREE): require_numeric_layout proves the
            // array is raw-f64(-or-holes) and the value is canonical — no GC
            // pointer is written, so no write barrier.
            if value_is_canonical_raw_f64 {
                blk.store(DOUBLE, val_double, &element_ptr);
            } else {
                // GC_STORE_AUDIT(POINTER_FREE): js_array_numeric_value_to_raw_f64
                // returns a plain unboxed f64 — no GC pointer, so no barrier.
                let numeric_value = canonicalize_raw_f64_numeric_store_value(blk, val_double);
                blk.store(DOUBLE, &numeric_value, &element_ptr);
            }
            // Bump length: store idx+1 to arr_ptr+0.
            let new_len = blk.add(I32, &idx_i32, "1");
            let len_ptr = blk.inttoptr(I64, &arr_handle);
            blk.store(I32, &new_len, &len_ptr);
            // Flag transition: holes were created iff idx > length. Then the
            // DENSE bit (0x80) must drop and the HOLES bit (0x1000) records
            // the still-valid raw-f64-or-holes invariant (branchless header
            // rewrite; idempotent for already-holes-flagged arrays). This is
            // feedback-stat-free by design: `invalidate_representation_change`
            // only updates typed-feedback observation counters, and this tier
            // is not emitted in feedback builds.
            let created = blk.icmp_ugt(I32, &idx_i32, &length);
            let reserved_addr = blk.sub(I64, &arr_handle, "6");
            let reserved_ptr = blk.inttoptr(I64, &reserved_addr);
            let reserved = blk.load(I16, &reserved_ptr);
            let without_dense = blk.and(I16, &reserved, "-129"); // ~0x80
            let with_holes = blk.or(I16, &without_dense, "4096"); // 0x1000
            let new_reserved = blk.select(I1, &created, I16, &with_holes, &reserved);
            blk.store(I16, &new_reserved, &reserved_ptr);
            blk.br(&merge_label);
        }
    } else if require_numeric_layout {
        // Feedback-build numeric shape: dense append only (idx == length was
        // proven by `can_extend_inline`), no holes are created.
        let blk = ctx.block();
        let (_, element_ptr) = element_slot(blk, &arr_handle, &idx_i32);
        // GC_STORE_AUDIT(POINTER_FREE): require_numeric_layout proves the
        // array is raw-f64 and the value is canonicalized to a plain f64 —
        // no GC pointer is written into the slot, so no write barrier.
        if value_is_canonical_raw_f64 {
            blk.store(DOUBLE, val_double, &element_ptr);
        } else {
            // GC_STORE_AUDIT(POINTER_FREE): js_array_numeric_value_to_raw_f64
            // returns a plain unboxed f64 — no GC pointer, so no barrier.
            let numeric_value = canonicalize_raw_f64_numeric_store_value(blk, val_double);
            blk.store(DOUBLE, &numeric_value, &element_ptr);
        }
        let new_len = blk.add(I32, &idx_i32, "1");
        let len_ptr = blk.inttoptr(I64, &arr_handle);
        blk.store(I32, &new_len, &len_ptr);
        blk.br(&merge_label);
    } else {
        let blk = ctx.block();
        let (element_addr, element_ptr) = element_slot(blk, &arr_handle, &idx_i32);
        {
            let value_bits = emit_jsvalue_slot_store_on_block(
                blk,
                &element_ptr,
                val_double,
                &arr_handle,
                &idx_i32,
                layout_note_needed,
                &arr_handle,
                &element_addr,
                write_barrier_needed,
            )
            .unwrap_or_else(|| blk.bitcast_double_to_i64(val_double));
            if !value_is_numeric {
                emit_array_numeric_write_note_on_block(blk, &arr_handle, &value_bits);
            }
        }
        // Bump length: store idx+1 to arr_ptr+0.
        let new_len = blk.add(I32, &idx_i32, "1");
        let len_ptr = blk.inttoptr(I64, &arr_handle); // length is at offset 0
        blk.store(I32, &new_len, &len_ptr);
        blk.br(&merge_label);
    }

    // SLOW: realloc needed. Call the runtime, write new ptr to local.
    ctx.current_block = realloc_idx;
    {
        let blk = ctx.block();
        blk.call_void(
            "js_typed_feedback_record_fallback_call",
            &[(I64, feedback_site_id)],
        );
        // Strict `arr[i] = v`: a frozen array's element is non-writable and a
        // non-extensible array rejects a new index, so route to the throwing
        // variant. (The inline fast/medium paths above are only reached for
        // arrays with a proven dense-numeric layout, which excludes frozen /
        // sealed / non-extensible arrays — those always fall to this call.)
        let new_handle = blk.call(
            I64,
            "js_array_set_f64_extend_strict",
            &[(I64, &arr_handle), (I32, &idx_i32), (DOUBLE, val_double)],
        );
        let new_box = nanbox_pointer_inline(blk, &new_handle);
        blk.store(DOUBLE, &new_box, &slot);
        let val_bits = blk.bitcast_double_to_i64(val_double);
        emit_write_barrier_slot_on_block(blk, &arr_handle, "0", &val_bits);
        blk.br(&merge_label);
    }

    ctx.current_block = merge_idx;
    Ok(())
}
