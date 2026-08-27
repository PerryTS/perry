//! Guarded and packed-f64 array element reads for `IndexGet`.
//!
//! Split out of `index_get.rs` to keep that file under the 2000-line cap.
//! Pure mechanical move — the items below are verbatim copies (only the
//! visibility of the three entry points is widened to `pub(super)` so the
//! trunk's call sites keep compiling).
//!
//! # Rooting (Layer 1, slice 4)
//!
//! Listed in `crate::rooting`'s `MIGRATED_MODULES`, and the listing is
//! **vacuous on the committed source**: this module has never named an
//! `expr::temp_root` symbol, so only the sabotage arm makes the line an
//! assertion. The audit that earned it: every entry point here takes operands
//! its caller has already lowered, and lowers no user expression of its own, so
//! there is no window for a root to span. `js_array_refresh_local_head` and the
//! `*_index_get_guard` helpers are the only calls in the receiver's live range,
//! and neither re-enters user code.

use anyhow::Result;

use crate::nanbox::POINTER_MASK_I64;
use crate::native_value::{
    BoundsState, BufferAccessMode, LoweredValue, MaterializationReason, NativeRep, SemanticKind,
};
use crate::types::{DOUBLE, I1, I16, I32, I64, I8};

use super::{
    array_kind_fact, emit_typed_feedback_register_site, raw_f64_layout_fact,
    typed_feedback_emission_enabled, FnCtx, PackedF64LoopFact, TypedFeedbackContract,
    TypedFeedbackKind,
};

/// Load one generic JavaScript array element through a handle admitted by a
/// versioned caller. Bounds, descriptor/prototype state, forwarding state, and
/// the live array header were checked at that iteration's entry. This function
/// intentionally has no branch to an ordinary array fallback.
pub(super) fn lower_trusted_plain_array_index_get(
    ctx: &mut FnCtx<'_>,
    array_handle: &str,
    idx_i32: &str,
) -> String {
    let blk = ctx.block();
    let idx_i64 = blk.zext(I32, idx_i32, I64);
    let byte_offset = blk.shl(I64, &idx_i64, "3");
    let with_header = blk.add(I64, &byte_offset, "8");
    let element_addr = blk.add(I64, array_handle, &with_header);
    let element_ptr = blk.inttoptr(I64, &element_addr);
    let raw = blk.load(DOUBLE, &element_ptr);
    let raw_bits = blk.bitcast_double_to_i64(&raw);
    let is_hole = blk.icmp_eq(I64, &raw_bits, crate::nanbox::TAG_HOLE_I64);
    let undefined = blk.bitcast_i64_to_double(crate::nanbox::TAG_UNDEFINED_I64);
    blk.select(I1, &is_hole, DOUBLE, &undefined, &raw)
}

pub(super) fn lower_guarded_array_index_get(
    ctx: &mut FnCtx<'_>,
    arr_box: &str,
    idx_i32: &str,
    block_prefix: &str,
    require_numeric_layout: bool,
    coerce_numeric_fallback: bool,
    receiver_slot: Option<&str>,
) -> Result<String> {
    let contract = if require_numeric_layout {
        TypedFeedbackContract::numeric_array_get_index()
    } else {
        TypedFeedbackContract::array_get_index()
    };
    let feedback_site_id = emit_typed_feedback_register_site(
        ctx,
        TypedFeedbackKind::ArrayElement,
        "array[index]",
        contract,
    );
    let fast_idx = ctx.new_block(&format!("{}.fast", block_prefix));
    let fallback_idx = ctx.new_block(&format!("{}.fallback", block_prefix));
    // A non-negative ordinary-array index at or above `length` has no own
    // element: defining an array-index property would have raised `length`.
    // Once the same structural checks used by the raw-load tier have also
    // proved that there are no indexed descriptors and no indexed prototype
    // properties, that result is `undefined` without consulting the generic
    // polymorphic getter. Sparse-set membership tests hit exactly this arm for
    // absent ids, so keep it separate from the in-bounds raw-load block.
    let inline_oob_idx = if !typed_feedback_emission_enabled() {
        Some(ctx.new_block(&format!("{}.guard.oob", block_prefix)))
    } else {
        None
    };
    let merge_idx = ctx.new_block(&format!("{}.merge", block_prefix));
    let fast_label = ctx.block_label(fast_idx);
    let fallback_label = ctx.block_label(fallback_idx);
    let merge_label = ctx.block_label(merge_idx);
    // The inline guard can heal one ordinary growth/evacuation forwarding
    // edge before it admits the raw load. Keep the exact handle proved by
    // each predecessor so the fast block never re-derives a stale address
    // from the original boxed receiver.
    let mut inline_fast_handle: Option<(String, String)> = None;
    let mut runtime_fast_handle: Option<(String, String)> = None;

    if !typed_feedback_emission_enabled() {
        // Normal builds do not collect feedback. Inline the plain-array
        // structural guard instead of paying an out-of-line call merely to
        // rediscover the same header facts before the direct slot load below.
        // Prototype-chain invalidators are summarized by one sticky runtime
        // byte; per-array descriptors and forwarding remain receiver-local.
        //
        // Repsel 4a.1: the NUMERIC tier gets the same inline guard — plus an
        // `_reserved & GC_ARRAY_RAW_F64_LAYOUT (0x80)` dense-proof test on the
        // header word the plain guard already loads. A dense-flagged array
        // needs no runtime call at all (the raw-f64 slot IS the value, no
        // hole select). Arrays not yet flagged take a COLD out-of-line
        // `js_typed_feedback_numeric_array_index_get_guard` call, whose
        // first-touch path verifies-and-rewrites the layout (setting the
        // flag), so the steady state is the inline tier. This ends the
        // typed-`number[]`-slower-than-untyped inversion for reads.
        let deref_idx = ctx.new_block(&format!("{}.guard.deref", block_prefix));
        let deref_label = ctx.block_label(deref_idx);
        let live_deref_idx = ctx.new_block(&format!("{}.guard.live", block_prefix));
        let live_deref_label = ctx.block_label(live_deref_idx);
        let cold_guard_idx = if require_numeric_layout {
            Some(ctx.new_block(&format!("{}.guard.cold", block_prefix)))
        } else {
            None
        };
        let guard_fail_label = match cold_guard_idx {
            Some(idx) => ctx.block_label(idx),
            None => fallback_label.clone(),
        };
        let range_idx = ctx.new_block(&format!("{}.guard.range", block_prefix));
        let range_label = ctx.block_label(range_idx);
        {
            let blk = ctx.block();
            let arr_bits = blk.bitcast_double_to_i64(arr_box);
            let arr_handle = blk.and(I64, &arr_bits, POINTER_MASK_I64);
            let tag = blk.lshr(I64, &arr_bits, "48");
            let is_pointer = blk.icmp_eq(I64, &tag, "32765"); // POINTER_TAG
            let above_handle_band = blk.icmp_ugt(I64, &arr_handle, "1048575");
            let heap_candidate = blk.and(I1, &is_pointer, &above_handle_band);
            blk.cond_br(&heap_candidate, &deref_label, &guard_fail_label);
        }

        ctx.current_block = deref_idx;
        let live_handle = {
            let blk = ctx.block();
            let arr_bits = blk.bitcast_double_to_i64(arr_box);
            let arr_handle = blk.and(I64, &arr_bits, POINTER_MASK_I64);

            let gc_type_addr = blk.sub(I64, &arr_handle, "8");
            let gc_type_ptr = blk.inttoptr(I64, &gc_type_addr);
            let gc_type = blk.load(I8, &gc_type_ptr);
            let is_array = blk.icmp_eq(I8, &gc_type, "1"); // GC_TYPE_ARRAY

            let gc_flags_addr = blk.sub(I64, &arr_handle, "7");
            let gc_flags_ptr = blk.inttoptr(I64, &gc_flags_addr);
            let gc_flags = blk.load(I8, &gc_flags_ptr);
            let forwarded_bits = blk.and(I8, &gc_flags, "128");
            let is_forwarded = blk.icmp_ne(I8, &forwarded_bits, "0");

            // Array growth and GC evacuation leave the live user address in
            // the first payload word of a forwarded array stub. Follow one
            // edge inline, then re-brand and re-check the destination below.
            // Longer/corrupt chains remain closed and take the boxed fallback.
            // This mirrors the common one-edge arm of `clean_arr_ptr` without
            // paying its allocator/registry probes on every indexed read.
            let original_arr_ptr = blk.inttoptr(I64, &arr_handle);
            let forwarding_target = blk.load(I64, &original_arr_ptr);
            let follow_forwarding = blk.and(I1, &is_array, &is_forwarded);
            let live_handle =
                blk.select(I1, &follow_forwarding, I64, &forwarding_target, &arr_handle);

            let live_top = blk.lshr(I64, &live_handle, "48");
            let live_top_clear = blk.icmp_eq(I64, &live_top, "0");
            let live_above_handle_band = blk.icmp_ugt(I64, &live_handle, "1048575");
            let live_heap_candidate = blk.and(I1, &live_top_clear, &live_above_handle_band);
            // A forwarding word is not trusted until its address is in the
            // heap band. In particular, do not read the destination header
            // speculatively: malformed or longer chains must reach the boxed
            // fallback without a native dereference of the selected target.
            blk.cond_br(&live_heap_candidate, &live_deref_label, &fallback_label);
            live_handle
        };

        ctx.current_block = live_deref_idx;
        let (index_in_bounds, reserved) = {
            let blk = ctx.block();
            let live_gc_type_addr = blk.sub(I64, &live_handle, "8");
            let live_gc_type_ptr = blk.inttoptr(I64, &live_gc_type_addr);
            let live_gc_type = blk.load(I8, &live_gc_type_ptr);
            let is_array = blk.icmp_eq(I8, &live_gc_type, "1"); // GC_TYPE_ARRAY

            let live_gc_flags_addr = blk.sub(I64, &live_handle, "7");
            let live_gc_flags_ptr = blk.inttoptr(I64, &live_gc_flags_addr);
            let live_gc_flags = blk.load(I8, &live_gc_flags_ptr);
            let live_forwarded_bits = blk.and(I8, &live_gc_flags, "128");
            let not_forwarded = blk.icmp_eq(I8, &live_forwarded_bits, "0");

            let reserved_addr = blk.sub(I64, &live_handle, "6");
            let reserved_ptr = blk.inttoptr(I64, &reserved_addr);
            let reserved = blk.load(I16, &reserved_ptr);
            let descriptor_bits = blk.and(I16, &reserved, "1024");
            let no_descriptors = blk.icmp_eq(I16, &descriptor_bits, "0");

            let invalidated = blk.load_volatile(I8, "@PERRY_ARRAY_INDEX_FAST_PATH_INVALIDATED");
            let default_prototype_chain = blk.icmp_eq(I8, &invalidated, "0");

            let arr_ptr = blk.inttoptr(I64, &live_handle);
            let length = blk.load(I32, &arr_ptr);
            let capacity_ptr = blk.gep(I8, &arr_ptr, &[(I64, "4")]);
            let capacity = blk.load(I32, &capacity_ptr);
            let index_nonnegative = blk.icmp_slt(I32, idx_i32, "0");
            let index_nonnegative = blk.icmp_eq(I1, &index_nonnegative, "false");
            let index_in_bounds = blk.icmp_ult(I32, idx_i32, &length);
            let length_sane = blk.icmp_ule(I32, &length, "16000000");
            let capacity_sane = blk.icmp_ule(I32, &capacity, "16000000");
            let length_within_capacity = blk.icmp_ule(I32, &length, &capacity);

            let mut structural_ok = blk.and(I1, &is_array, &not_forwarded);
            structural_ok = blk.and(I1, &structural_ok, &no_descriptors);
            structural_ok = blk.and(I1, &structural_ok, &default_prototype_chain);
            structural_ok = blk.and(I1, &structural_ok, &index_nonnegative);
            structural_ok = blk.and(I1, &structural_ok, &length_sane);
            structural_ok = blk.and(I1, &structural_ok, &capacity_sane);
            structural_ok = blk.and(I1, &structural_ok, &length_within_capacity);
            blk.cond_br(&structural_ok, &range_label, &guard_fail_label);

            // `index_in_bounds` and `reserved` dominate the range block. The
            // former selects raw load versus the proven-absent result; the
            // latter carries the optional numeric-layout proof below.
            (index_in_bounds, reserved)
        };

        let numeric_in_bounds_idx = require_numeric_layout
            .then(|| ctx.new_block(&format!("{}.guard.numeric_in_bounds", block_prefix)));
        let numeric_in_bounds_label = numeric_in_bounds_idx.map(|idx| ctx.block_label(idx));
        let oob_label = ctx.block_label(inline_oob_idx.expect("normal-build OOB block"));
        ctx.current_block = range_idx;
        {
            let blk = ctx.block();
            let mut in_bounds_ok = index_in_bounds.clone();
            if require_numeric_layout {
                // Dense raw-f64 proof: every slot in [0, length) holds
                // canonical raw f64 bits (GC_ARRAY_RAW_F64_LAYOUT, 0x80).
                //
                // Repsel 4a.2 (#6904): a NUMBER-CONTEXT read (the caller will
                // ToNumber the element regardless — `coerce_numeric_fallback`)
                // additionally accepts the hole-tolerant invariant
                // (GC_ARRAY_RAW_F64_HOLES, 0x1000): every slot is canonical
                // raw f64 OR TAG_HOLE, and the fast arm canonicalizes any NaN
                // payload (TAG_HOLE included) to the quiet NaN — bit-exact
                // with ToNumber(undefined) for a hole and with ToNumber(NaN)
                // for a stored NaN. This is the `new Array(n)` mid-fill axis:
                // such arrays are provably-not-dense until the last slot is
                // written, so the dense-only tier never fired for them.
                let raw_mask = if coerce_numeric_fallback {
                    "4224" // 0x1080 = RAW_F64_LAYOUT | RAW_F64_HOLES
                } else {
                    "128" // dense only: the raw slot is exposed verbatim
                };
                let raw_bits = blk.and(I16, &reserved, raw_mask);
                let is_raw = blk.icmp_ne(I16, &raw_bits, "0");
                in_bounds_ok = blk.and(I1, &in_bounds_ok, &is_raw);
            }
            if require_numeric_layout {
                // An in-bounds array without the requested numeric layout must
                // still visit the cold rebuilding guard. OOB needs no element
                // layout at all and can return directly.
                let in_bounds_idx = numeric_in_bounds_idx.expect("numeric in-bounds block");
                let in_bounds_label = numeric_in_bounds_label
                    .as_deref()
                    .expect("numeric in-bounds label");
                blk.cond_br(&index_in_bounds, &in_bounds_label, &oob_label);

                ctx.current_block = in_bounds_idx;
                ctx.block()
                    .cond_br(&in_bounds_ok, &fast_label, &guard_fail_label);
                inline_fast_handle = Some((live_handle, ctx.block().label.clone()));
            } else {
                inline_fast_handle = Some((live_handle, blk.label.clone()));
                blk.cond_br(&in_bounds_ok, &fast_label, &oob_label);
            }
        }

        if let Some(cold_idx) = cold_guard_idx {
            // Cold arm: the out-of-line guard rebuilds unmarked-but-numeric
            // arrays into raw-f64 layout (then this call site goes inline on
            // every later read); everything else routes to the boxed fallback.
            ctx.current_block = cold_idx;
            // Self-heal a stale growth-forwarded binding first (see
            // `receiver_repair_slot`): follow the chain, write the live head
            // back to the local slot. This iteration still takes the guard
            // on the ORIGINAL value (a forwarded head fails it → boxed
            // fallback, which follows the chain — correct either way); every
            // later iteration re-loads the repaired slot and goes inline.
            if let Some(slot) = receiver_slot {
                let blk = ctx.block();
                let fresh = blk.call(DOUBLE, "js_array_refresh_local_head", &[(DOUBLE, arr_box)]);
                blk.store(DOUBLE, &fresh, slot);
            }
            let guard_ok = {
                let blk = ctx.block();
                let arr_bits = blk.bitcast_double_to_i64(arr_box);
                let arr_handle = blk.and(I64, &arr_bits, POINTER_MASK_I64);
                runtime_fast_handle = Some((arr_handle, blk.label.clone()));
                let guard_i32 = blk.call(
                    I32,
                    "js_typed_feedback_numeric_array_index_get_guard",
                    &[
                        (I64, &feedback_site_id),
                        (DOUBLE, arr_box),
                        (I32, idx_i32),
                        (I32, "1"),
                    ],
                );
                blk.icmp_ne(I32, &guard_i32, "0")
            };
            ctx.block().cond_br(&guard_ok, &fast_label, &fallback_label);
        }
    } else {
        let guard_ok = {
            let blk = ctx.block();
            let arr_bits = blk.bitcast_double_to_i64(arr_box);
            let arr_handle = blk.and(I64, &arr_bits, POINTER_MASK_I64);
            runtime_fast_handle = Some((arr_handle, blk.label.clone()));
            let guard_fn = if require_numeric_layout {
                "js_typed_feedback_numeric_array_index_get_guard"
            } else {
                "js_typed_feedback_plain_array_index_get_guard"
            };
            let guard_i32 = blk.call(
                I32,
                guard_fn,
                &[
                    (I64, &feedback_site_id),
                    (DOUBLE, arr_box),
                    (I32, idx_i32),
                    (I32, "1"),
                ],
            );
            blk.icmp_ne(I32, &guard_i32, "0")
        };
        ctx.block().cond_br(&guard_ok, &fast_label, &fallback_label);
    }

    let inline_oob = inline_oob_idx.map(|oob_idx| {
        ctx.current_block = oob_idx;
        let value = if require_numeric_layout && coerce_numeric_fallback {
            // This is ToNumber(undefined), matching the boxed fallback.
            "0x7FF8000000000000".to_string()
        } else {
            ctx.block()
                .bitcast_i64_to_double(crate::nanbox::TAG_UNDEFINED_I64)
        };
        let end_label = ctx.block().label.clone();
        ctx.block().br(&merge_label);
        (value, end_label)
    });

    ctx.current_block = fallback_idx;
    // Materialize the f64 index only here (cold path) so the int→fp conversion
    // stays out of the numeric loop's hot region.
    let idx_box = ctx.block().sitofp(I32, idx_i32, DOUBLE);
    let fallback_boxed = ctx.block().call(
        DOUBLE,
        "js_typed_feedback_array_index_get_fallback_boxed",
        &[
            (I64, &feedback_site_id),
            (DOUBLE, arr_box),
            (DOUBLE, &idx_box),
        ],
    );
    let fallback_val = if require_numeric_layout && coerce_numeric_fallback {
        ctx.block()
            .call(DOUBLE, "js_number_coerce", &[(DOUBLE, &fallback_boxed)])
    } else {
        fallback_boxed.clone()
    };
    let fallback_end_label = ctx.block().label.clone();
    ctx.block().br(&merge_label);
    if require_numeric_layout {
        let fallback = LoweredValue::js_value(fallback_boxed.clone());
        ctx.record_lowered_value_with_access_mode_and_facts(
            "NumericArrayIndexGet",
            None,
            "js_typed_feedback_array_index_get_fallback_boxed",
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
                    None,
                    "rejected",
                    "numeric_array_index_get_guard",
                    Some(MaterializationReason::RuntimeApi),
                ),
                raw_f64_layout_fact(
                    None,
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

    ctx.current_block = fast_idx;
    let fast_blk = ctx.block();
    let arr_handle = match (&inline_fast_handle, &runtime_fast_handle) {
        (Some((inline_handle, inline_pred)), Some((runtime_handle, runtime_pred))) => fast_blk.phi(
            I64,
            &[
                (inline_handle.as_str(), inline_pred.as_str()),
                (runtime_handle.as_str(), runtime_pred.as_str()),
            ],
        ),
        (Some((handle, _)), None) | (None, Some((handle, _))) => handle.clone(),
        (None, None) => unreachable!("guarded array fast block has no predecessor handle"),
    };
    let fast_val = if require_numeric_layout {
        // The guard on the way into this block (inline tier or the runtime
        // `numeric_array_index_get_guard`) already proved: a plain,
        // non-forwarded `Array`, in raw-f64 (or, for number-context reads,
        // raw-f64-or-holes) layout, with `index` in bounds. So load the slot
        // inline instead of calling `js_array_numeric_get_f64_unboxed`,
        // whose hot path re-validates exactly those same conditions and then
        // does this load.
        let idx_i64 = fast_blk.zext(I32, idx_i32, I64);
        let byte_offset = fast_blk.shl(I64, &idx_i64, "3");
        let with_header = fast_blk.add(I64, &byte_offset, "8");
        let element_addr = fast_blk.add(I64, &arr_handle, &with_header);
        let element_ptr = fast_blk.inttoptr(I64, &element_addr);
        let raw = fast_blk.load(DOUBLE, &element_ptr);
        if coerce_numeric_fallback {
            // Repsel 4a.2: number-context canonicalization — any NaN payload
            // (a TAG_HOLE slot under the raw-f64-or-holes proof, or a stored
            // canonical NaN) becomes the quiet NaN. Bit-exact:
            // ToNumber(undefined) = NaN for a hole, ToNumber(NaN) = NaN for a
            // stored NaN, identity for every real number. PROOF-GATED: only
            // sound because the guard admitted raw-f64-or-holes slots — an
            // arbitrary NaN-boxed tag would be wrongly collapsed to NaN.
            let is_ord = fast_blk.fcmp("ord", &raw, &raw);
            fast_blk.select(I1, &is_ord, DOUBLE, &raw, "0x7FF8000000000000")
        } else {
            // Dense-only proof: no HOLE slots exist; the raw slot IS the
            // element value, exposed verbatim.
            raw
        }
    } else {
        let idx_i64 = fast_blk.zext(I32, idx_i32, I64);
        let byte_offset = fast_blk.shl(I64, &idx_i64, "3");
        let with_header = fast_blk.add(I64, &byte_offset, "8");
        let element_addr = fast_blk.add(I64, &arr_handle, &with_header);
        let element_ptr = fast_blk.inttoptr(I64, &element_addr);
        let fast_raw = fast_blk.load(DOUBLE, &element_ptr);
        // `new Array(n)` slots are TAG_HOLE internally; JavaScript reads expose
        // `undefined`.
        let fast_raw_bits = fast_blk.bitcast_double_to_i64(&fast_raw);
        let is_hole = fast_blk.icmp_eq(I64, &fast_raw_bits, crate::nanbox::TAG_HOLE_I64);
        let undef_d = fast_blk.bitcast_i64_to_double(crate::nanbox::TAG_UNDEFINED_I64);
        fast_blk.select(I1, &is_hole, DOUBLE, &undef_d, &fast_raw)
    };
    let fast_end_label = fast_blk.label.clone();
    fast_blk.br(&merge_label);
    if require_numeric_layout {
        let fast = LoweredValue {
            semantic: SemanticKind::JsNumber,
            rep: NativeRep::F64,
            llvm_ty: DOUBLE,
            value: fast_val.clone(),
        };
        ctx.record_lowered_value_with_access_mode_and_facts(
            "NumericArrayIndexGet",
            None,
            "js_array_numeric_get_f64_unboxed",
            &fast,
            Some(BoundsState::Guarded {
                guard_id: "numeric_array_index_get_guard".to_string(),
            }),
            None,
            Some(BufferAccessMode::CheckedNative),
            None,
            None,
            None,
            vec![raw_f64_layout_fact(
                None,
                "consumed",
                "numeric_array_index_get_guard",
                None,
            )],
            Vec::new(),
            false,
            false,
            Vec::new(),
        );
    }

    ctx.current_block = merge_idx;
    let mut incoming: Vec<(&str, &str)> = vec![
        (fast_val.as_str(), fast_end_label.as_str()),
        (fallback_val.as_str(), fallback_end_label.as_str()),
    ];
    if let Some((oob_value, oob_end_label)) = inline_oob.as_ref() {
        incoming.push((oob_value.as_str(), oob_end_label.as_str()));
    }
    Ok(ctx.block().phi(DOUBLE, &incoming))
}

pub(super) fn packed_f64_loop_fact(
    ctx: &FnCtx<'_>,
    arr_id: u32,
    idx_id: u32,
) -> Option<PackedF64LoopFact> {
    ctx.packed_f64_loop_facts
        .iter()
        .find(|fact| fact.array_local_id == arr_id && fact.index_local_id == idx_id)
        .cloned()
}

pub(super) fn lower_packed_f64_loop_index_get(
    ctx: &mut FnCtx<'_>,
    arr_id: u32,
    arr_box: &str,
    idx_i32: &str,
    fact: &PackedF64LoopFact,
) -> String {
    let guard_id = fact.guard_id.as_str();
    let array_kind = fact.array_kind;
    let value = {
        let blk = ctx.block();
        let arr_bits = blk.bitcast_double_to_i64(arr_box);
        let arr_handle = blk.and(I64, &arr_bits, POINTER_MASK_I64);
        let idx_i64 = blk.zext(I32, idx_i32, I64);
        let byte_offset = blk.shl(I64, &idx_i64, "3");
        let with_header = blk.add(I64, &byte_offset, "8");
        let element_addr = blk.add(I64, &arr_handle, &with_header);
        let element_ptr = blk.inttoptr(I64, &element_addr);
        blk.load(DOUBLE, &element_ptr)
    };
    if fact.allow_holes {
        // #6011: hole-tolerant range-guarded loop — the guard proved every
        // slot in the window is a raw-f64 number OR TAG_HOLE. Reading a hole
        // must observe `undefined` (or a polluted prototype), so side-exit to
        // the slow preheader, which re-executes the current iteration through
        // the generic read path. The side exit fires before any effect of the
        // iteration (matcher invariant), so the re-run cannot double-apply.
        let is_hole = {
            let blk = ctx.block();
            let raw_bits = blk.bitcast_double_to_i64(&value);
            blk.icmp_eq(I64, &raw_bits, crate::nanbox::TAG_HOLE_I64)
        };
        let cont_idx = ctx.new_block("packed_f64_range.load.cont");
        let cont_label = ctx.block_label(cont_idx);
        ctx.block()
            .cond_br(&is_hole, &fact.store_side_exit_label, &cont_label);
        ctx.current_block = cont_idx;
    }
    let lowered = LoweredValue {
        semantic: SemanticKind::JsNumber,
        rep: NativeRep::F64,
        llvm_ty: DOUBLE,
        value: value.clone(),
    };
    ctx.record_lowered_value_with_access_mode_and_facts(
        array_kind.load_expr_kind(),
        Some(arr_id),
        array_kind.load_consumer_f64(),
        &lowered,
        Some(BoundsState::Guarded {
            guard_id: guard_id.to_string(),
        }),
        None,
        Some(BufferAccessMode::CheckedNative),
        None,
        None,
        None,
        vec![
            array_kind_fact(
                Some(arr_id),
                "consumed",
                array_kind.array_kind_label(),
                None,
            ),
            raw_f64_layout_fact(Some(arr_id), "consumed", guard_id, None),
        ],
        Vec::new(),
        false,
        false,
        vec![
            "index_range=nonnegative_i32".to_string(),
            "length_range=guarded_i32".to_string(),
            "storage_layout=raw_f64_numeric_slots".to_string(),
        ],
    );
    value
}
