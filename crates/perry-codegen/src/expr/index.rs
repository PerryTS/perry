//! Array index-set fast-path lowering (extracted from `expr.rs`, issue
//! #1098). Pure move — no logic changes.

use anyhow::{anyhow, Result};

use super::{
    emit_jsvalue_slot_store_with_flags_on_block, emit_scalar_aware_store_gated_on_pointerness,
    emit_write_barrier_slot_on_block, emit_write_barrier_slot_value_and_generation_tested,
    nanbox_pointer_inline, raw_f64_layout_fact, FnCtx,
};
use crate::block::LlBlock;
use crate::nanbox::POINTER_MASK_I64;
use crate::native_value::{
    BoundsState, BufferAccessMode, LoweredValue, MaterializationReason, NativeRep, SemanticKind,
};
use crate::types::{DOUBLE, I1, I16, I32, I64};

use super::index_get::guarded_array::{
    emit_array_guard_word, ARRAY_STORE_GUARD_EXPECT_I32, ARRAY_STORE_GUARD_MASK_I32,
    HEAP_POINTER_BAND_BASE_I64, HEAP_POINTER_STORE_BAND_SPAN_I64,
};

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
    // #9186: whether the stored value can be a heap string, decided by the
    // caller with `store_needs_string_addref`. The plain slot-store emitters
    // tie the addref demote to `layout_note_needed`, so a boolean or `null`
    // written into an array that needs a layout note paid an
    // `js_string_addref_if_heap_string` call it can never use.
    string_addref_needed: bool,
    write_barrier_needed: bool,
    // Unused since every store settles the element kind first
    // (`emit_array_store_kind_value`); kept for the callers.
    _value_is_numeric: bool,
    require_numeric_layout: bool,
    // Repsel 4a.0: RHS proven canonical-raw-f64 by
    // `expr_produces_canonical_raw_f64` — the slot store may skip the
    // `js_array_numeric_value_to_raw_f64` canonicalization call entirely.
    value_is_canonical_raw_f64: bool,
    // #9394: the assignment's own `Throw` flag (ES2024 §6.2.5.7). The guard
    // declines exactly the receivers whose element write can be REJECTED
    // (frozen, sealed, descriptor-bearing, prototype-sensitive), so this is
    // the flag the fallback continuation needs to decide between a TypeError
    // and a silent no-op.
    assignment_strict: bool,
    feedback_site_id: &str,
) -> Result<()> {
    // #8583-followup: if evaluating an operand diverged — a throwing
    // sub-expression (e.g. a TDZ access on a captured `let`) emitted a
    // `js_throw_error_with_code` + `unreachable` — the current block is
    // terminated. `LlBlock` silently drops any instruction emitted after a
    // terminator, so the element setup below (`arr_bits`/`arr_handle`/`idx_i32`)
    // is dropped, but the guarded fast path still creates fresh blocks that
    // reference those dropped registers, which the dialect builder rejects as
    // "register %rN used but never defined". The index-set is unreachable on
    // this path, so emit nothing.
    if ctx.block().is_terminated() {
        return Ok(());
    }

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
    // sticky global. Guard misses fall to the existing out-of-line guard,
    // whose first-touch path rebuilds unmarked numeric arrays (setting the
    // dense flag), so the steady state is call-free. Feedback-emission builds
    // keep the out-of-line guard for observation coverage.
    //
    // #7396: this tier used to additionally require the RHS to be
    // canonical-raw-f64 *by construction* (`expr_produces_canonical_raw_f64`),
    // on the reasoning that the out-of-line guard's `is_numeric_value_bits`
    // leg is then statically true. That was the only thing the flag bought —
    // the `inbounds`/`extend_inline` arms below already canonicalize a
    // non-canonical RHS through `js_array_numeric_value_to_raw_f64`. But it
    // gated the tier off for the single most common numeric store there is:
    // element-to-element traffic, `arr[i] = arr[j]`. A guarded array READ
    // lowers to a phi over {raw slot load, boxed fallback}, which is not
    // statically canonical, so `bench_array_ops`' reverse-in-place loop paid
    // a five-argument cross-crate call per store — 103 of ~660 profile
    // samples, the largest single non-generated frame.
    //
    // The static flag is now replaced by a *runtime* test of the same
    // predicate (see `value_numeric` below) whenever it is not statically
    // known, so the tier covers those stores too and the flag only decides
    // whether those three instructions are emitted at all.
    // #9237: the tier is no longer restricted to statically numeric receivers.
    // Every condition the out-of-line guard checks is already tested inline here
    // — array type, not-forwarded, no element descriptors, integrity flags, the
    // prototype-chain invalidation flag, and the length/capacity sanity bounds —
    // and the in-bounds arm below already knows how to store a tagged value into
    // a non-raw-f64 receiver (that arm is what the out-of-line guard fronts
    // today). Only two of the conditions are specific to the raw-f64 store, and
    // they are now applied only when it is the one being emitted, so a
    // `boolean[]` store reaches the same inline guard instead of paying
    // `js_typed_feedback_plain_array_index_set_guard` per element: ~41% of such
    // a loop, and the reason `benchmarks/suite/11_prime_sieve.ts` sits on the
    // call tier for its whole run.
    let inline_write_tier = !super::typed_feedback_emission_enabled();
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
            // A heap pointer iff the box, less `POINTER_TAG << 48 | 1 MiB`,
            // lands below 2^47 - 1 MiB (`is_valid_obj_ptr`'s ceiling, #7396).
            let band_offset = blk.sub(I64, &arr_bits, HEAP_POINTER_BAND_BASE_I64);
            let heap_candidate = blk.icmp_ult(I64, &band_offset, HEAP_POINTER_STORE_BAND_SPAN_I64);
            blk.cond_br(&heap_candidate, &deref_label, &cold_label);
        }
        ctx.current_block = deref_idx;
        {
            let blk = ctx.block();
            // ONE header word (DESIGN arrayread §3): an ordinary array, not a
            // growth stub, no element descriptors, not frozen / sealed /
            // non-extensible. The prototype facts matter only when the store
            // adds an element (a hole or `index >= length`), so they are
            // tested on that arm (`idxset.ext`), not here.
            let word = emit_array_guard_word(blk, &arr_handle);
            let masked = blk.and(I32, &word, ARRAY_STORE_GUARD_MASK_I32);
            let mut guard_ok = blk.icmp_eq(I32, &masked, ARRAY_STORE_GUARD_EXPECT_I32);
            if require_numeric_layout {
                // Raw-f64 layout is a precondition of the RAW store only
                // (dense 0x80 or raw-f64-or-holes 0x1000, `_reserved` is the
                // word's upper half). A tagged store is correct whatever the
                // layout says.
                let f64_bits = blk.and(I32, &word, "276824064"); // 0x1080 << 16
                let is_f64 = blk.icmp_ne(I32, &f64_bits, "0");
                guard_ok = blk.and(I1, &guard_ok, &is_f64);
            }
            if require_numeric_layout && !value_is_canonical_raw_f64 {
                // #7396: the out-of-line guard's `is_numeric_value_bits(value)`
                // leg, inlined. It is load-bearing and NOT implied by
                // `require_numeric_layout`: that is a *static* TypeScript
                // judgment, and Perry does not validate declared types at
                // runtime, so a `number[]` slot can genuinely receive a
                // non-number — a hole/OOB read fallback returning `undefined`
                // is the ordinary way it happens for `arr[i] = arr[j]`. Letting
                // one through would hand a NaN-boxed tag to
                // `js_array_numeric_value_to_raw_f64` and corrupt the array's
                // raw-f64 invariant.
                //
                // Deliberately STRICTER than `is_numeric_value_bits`, which
                // also admits `INT32_TAG` payloads that are not registered
                // class ids — replicating that needs the class-id registry
                // lookup, i.e. exactly the out-of-line call being removed. So
                // reject the whole non-numeric NaN-box tag range
                // `0x7FF9..=0x7FFF` (pointer, string, BigInt, INT32,
                // undefined/null/bool) in one unsigned range compare; INT32
                // numerics simply take the cold arm. Unsigned wraparound covers
                // both sides: a real double's top 16 bits are either below
                // 0x7FF9 (subtraction wraps high) or >= 0x8000 (negative).
                let val_bits = blk.bitcast_double_to_i64(val_double);
                let val_tag = blk.lshr(I64, &val_bits, "48");
                let tag_offset = blk.sub(I64, &val_tag, "32761"); // 0x7FF9
                let value_numeric = blk.icmp_ugt(I64, &tag_offset, "6"); // span of 0x7FF9..=0x7FFF
                guard_ok = blk.and(I1, &guard_ok, &value_numeric);
            }
            blk.cond_br(&guard_ok, &guarded_label, &cold_label);
        }
    }
    if let Some(cold_idx) = cold_guard_idx {
        ctx.current_block = cold_idx;
        crate::expr::store_census::bump(ctx, crate::expr::store_census::ELEM_STORE_GUARD_MISS);
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
    crate::expr::store_census::bump(ctx, crate::expr::store_census::ELEM_STORE_FALLBACK);
    {
        let strict_flag = if assignment_strict { "1" } else { "0" };
        let fallback_box = ctx.block().call(
            DOUBLE,
            "js_typed_feedback_array_index_set_fallback_boxed",
            &[
                (I64, feedback_site_id),
                (DOUBLE, arr_box),
                (DOUBLE, idx_double),
                (DOUBLE, val_double),
                (I32, strict_flag),
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
    let probe_idx = ctx.new_block("idxset.probe");
    let probe_label = ctx.block_label(probe_idx);
    let ext_idx = ctx.new_block("idxset.ext");
    let ext_label = ctx.block_label(ext_idx);
    {
        // `idx <u capacity` (a negative index wraps high). `[length,
        // capacity)` holds `TAG_HOLE` (`array_truncate_length`), so a non-hole
        // slot below capacity is an existing element below `length`.
        let blk = ctx.block();
        let cap_addr = blk.add(I64, &arr_handle, "4");
        let cap_ptr = blk.inttoptr(I64, &cap_addr);
        let capacity = blk.load(I32, &cap_ptr);
        let within_capacity = blk.icmp_ult(I32, &idx_i32, &capacity);
        blk.cond_br(&within_capacity, &probe_label, &ext_label);
    }
    ctx.current_block = probe_idx;
    {
        // An existing element is an own data property: overwriting it never
        // consults the prototype chain.
        let blk = ctx.block();
        let (_, element_ptr) = element_slot(blk, &arr_handle, &idx_i32);
        let old = blk.load(I64, &element_ptr);
        let is_hole = blk.icmp_eq(I64, &old, crate::nanbox::TAG_HOLE_I64);
        blk.cond_br(&is_hole, &ext_label, &inbounds_label);
    }
    ctx.current_block = ext_idx;
    // `length` matters only to an add; an overwrite never reads it. Both guard
    // tiers proved `arr_handle` a live array head.
    let length = {
        let blk = ctx.block();
        let len_ptr = blk.inttoptr(I64, &arr_handle);
        blk.load(I32, &len_ptr)
    };
    {
        // An ADD (a hole, or `index >= length`): an inherited index setter
        // could intercept it, so the prototype facts are required here (#10593:
        // the process-wide byte and the array's own custom-proto bit). A hole
        // below `length`, or a negative index, is not an extend: full `[[Set]]`.
        let blk = ctx.block();
        let reserved_addr = blk.sub(I64, &arr_handle, "6");
        let reserved_ptr = blk.inttoptr(I64, &reserved_addr);
        let reserved = blk.load(I16, &reserved_ptr);
        let default_prototype_chain =
            crate::expr::array_proto_guard::emit_array_default_prototype_chain(blk, &reserved);
        let at_or_past_length = blk.icmp_sge(I32, &idx_i32, &length);
        let extend_ok = blk.and(I1, &default_prototype_chain, &at_or_past_length);
        blk.cond_br(&extend_ok, &check_cap_label, &guard_fallback_label);
    }

    // Helper: compute element_ptr = arr_ptr + 8 + idx*8.
    fn element_slot(blk: &mut LlBlock, arr_handle: &str, idx_i32: &str) -> (String, String) {
        let idx_i64 = blk.zext(I32, idx_i32, I64);
        let byte_offset = blk.shl(I64, &idx_i64, "3"); // *8
        let elements_addr = blk.array_elements_addr(arr_handle);
        let element_addr = blk.add(I64, &elements_addr, &byte_offset);
        let element_ptr = blk.inttoptr(I64, &element_addr);
        (element_addr, element_ptr)
    }

    // FASTEST: in-bounds path. Store directly, jump to merge.
    ctx.current_block = inbounds_idx;
    crate::expr::store_census::bump(ctx, crate::expr::store_census::ELEM_STORE_INBOUNDS);
    // The element kind is settled BEFORE the value is written (DESIGN
    // arrayread §3): an F64 array takes a Number as its canonical double and
    // clears its F64 bits first for anything else.
    let stored_value = if value_is_canonical_raw_f64 {
        val_double.to_string()
    } else {
        super::index_set_guarded::emit_array_store_kind_value(
            ctx,
            &arr_handle,
            val_double,
            "idxset.inbounds",
        )
    };
    // #7715 B3: on the JSValue arm the barrier is emitted separately, behind an
    // inline live test of the stored VALUE and then of the parent array's
    // generation — see `emit_write_barrier_slot_value_and_generation_tested`.
    // Nothing else about the store moves, and the barrier still lands between
    // the layout note and the numeric-write note exactly where it did.
    let inbounds_overwrite = {
        let blk = ctx.block();
        let (element_addr, element_ptr) = element_slot(blk, &arr_handle, &idx_i32);
        if require_numeric_layout {
            // GC_STORE_AUDIT(POINTER_FREE): require_numeric_layout proves the
            // array is raw-f64 and the value is canonicalized to a plain f64 —
            // no GC pointer is written into the slot, so no write barrier.
            // Canonical by construction (Repsel 4a.0), or made canonical by
            // `emit_array_store_kind_value` on this array's F64 kind.
            blk.store(DOUBLE, &stored_value, &element_ptr);
            None
        } else {
            // In-place overwrite of a non-raw-layout (e.g. downgraded `any[]`)
            // array element: the slot holds a valid value, so the scalar-aware
            // note skips the GC layout hashmap on scalar-over-scalar stores
            // (#5094 — ~9× on bench_numeric_array_downgrade).
            Some((element_addr, element_ptr))
        }
    };
    // #9237: the in-bounds overwrite's bookkeeping, emitted OUTSIDE the block
    // borrow so the pointer test can branch around it. Two independent gates,
    // because the two notes answer different questions: the layout note is dead
    // unless a pointer is involved on either side, while the numeric-write note
    // is what DOWNGRADES a raw-f64 array on its first non-numeric store and so
    // is gated on the header's raw-f64 bits instead (the test
    // `expr/index_set_guarded.rs` already applies).
    let gated_barrier = if let Some((element_addr, element_ptr)) = inbounds_overwrite {
        let value_bits = emit_scalar_aware_store_gated_on_pointerness(
            ctx,
            &element_ptr,
            &stored_value,
            &arr_handle,
            &idx_i32,
            string_addref_needed,
            layout_note_needed,
            &arr_handle,
            &element_addr,
            false,
            "idxset.inbounds",
        );
        if write_barrier_needed {
            Some((element_addr, value_bits))
        } else {
            None
        }
    } else {
        None
    };
    if let Some((element_addr, child_bits)) = gated_barrier {
        // `arr_handle` reached this block through the guard — the inline tier's
        // own `obj_type`/`!FORWARDED` header reads, or the out-of-line
        // `js_typed_feedback_plain_array_index_set_guard` — and the arm above
        // has already stored raw into `arr_handle + 8 + i*8`, so reading the
        // header byte at `arr_handle - 7` is strictly weaker than what this
        // block already does.
        emit_write_barrier_slot_value_and_generation_tested(
            ctx,
            &arr_handle,
            &arr_handle,
            &element_addr,
            &child_bits,
            "idxset.inbounds",
        );
    }
    ctx.block().br(&merge_label);
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
    crate::expr::store_census::bump(ctx, crate::expr::store_census::ELEM_STORE_APPEND);
    // The kind first, exactly as the in-bounds arm.
    let ext_value = if value_is_canonical_raw_f64 {
        val_double.to_string()
    } else {
        super::index_set_guarded::emit_array_store_kind_value(
            ctx,
            &arr_handle,
            val_double,
            "idxset.extend",
        )
    };
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
            blk.store(DOUBLE, &ext_value, &element_ptr);
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
        blk.store(DOUBLE, &ext_value, &element_ptr);
        let new_len = blk.add(I32, &idx_i32, "1");
        let len_ptr = blk.inttoptr(I64, &arr_handle);
        blk.store(I32, &new_len, &len_ptr);
        blk.br(&merge_label);
    } else {
        let blk = ctx.block();
        let (element_addr, element_ptr) = element_slot(blk, &arr_handle, &idx_i32);
        {
            // The kind was settled above; the returned bits are not needed.
            let _ = emit_jsvalue_slot_store_with_flags_on_block(
                blk,
                &element_ptr,
                &ext_value,
                &arr_handle,
                &idx_i32,
                string_addref_needed,
                layout_note_needed,
                &arr_handle,
                &element_addr,
                write_barrier_needed,
            );
        }
        // Bump length: store idx+1 to arr_ptr+0.
        let new_len = blk.add(I32, &idx_i32, "1");
        let len_ptr = blk.inttoptr(I64, &arr_handle); // length is at offset 0
        blk.store(I32, &new_len, &len_ptr);
        blk.br(&merge_label);
    }

    // SLOW: realloc needed. Call the runtime, write new ptr to local.
    ctx.current_block = realloc_idx;
    crate::expr::store_census::bump(ctx, crate::expr::store_census::ELEM_STORE_FALLBACK);
    {
        let blk = ctx.block();
        crate::expr::emit_typed_feedback_record_call(
            blk,
            "js_typed_feedback_record_fallback_call",
            &[(I64, feedback_site_id)],
        );
        // Growth for a receiver the guard already ACCEPTED. That guard
        // (`plain_array_index_set_guard`) declines frozen, sealed and
        // non-extensible arrays, descriptor-bearing arrays, and every
        // prototype-sensitive shape — all of which take the `fallback` edge
        // above instead — so no store reaching here can be rejected and the
        // entry's `Throw` argument is unobservable. The strict entry is kept
        // because it is the one that carries the fused key/policy/store
        // path (#9394 left this arm alone deliberately).
        let new_handle = blk.call(
            I64,
            "js_array_set_f64_extend_strict",
            &[(I64, &arr_handle), (I32, &idx_i32), (DOUBLE, val_double)],
        );
        let new_box = nanbox_pointer_inline(blk, &new_handle);
        blk.store(DOUBLE, &new_box, &slot);
        let val_bits = blk.bitcast_double_to_i64(val_double);
        // #7640 section E: the grow helper can return a replacement allocation.
        // The pre-call raw handle then names the forwarding source, not the array
        // that received the value. Use the returned live head for the barrier.
        emit_write_barrier_slot_on_block(blk, &new_handle, "0", &val_bits);
        blk.br(&merge_label);
    }

    ctx.current_block = merge_idx;
    Ok(())
}
