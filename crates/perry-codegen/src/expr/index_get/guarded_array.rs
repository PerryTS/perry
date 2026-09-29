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
    BoundsProof, BoundsState, BufferAccessMode, LoweredValue, MaterializationReason, NativeRep,
    SemanticKind,
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
    crate::expr::store_census::bump(ctx, crate::expr::store_census::ELEM_READ_OTHER_TIER);
    let blk = ctx.block();
    let idx_i64 = blk.zext(I32, idx_i32, I64);
    let byte_offset = blk.shl(I64, &idx_i64, "3");
    let elements_addr = blk.array_elements_addr(array_handle);
    let element_addr = blk.add(I64, &elements_addr, &byte_offset);
    let element_ptr = blk.inttoptr(I64, &element_addr);
    let raw = blk.load(DOUBLE, &element_ptr);
    let raw_bits = blk.bitcast_double_to_i64(&raw);
    let is_hole = blk.icmp_eq(I64, &raw_bits, crate::nanbox::TAG_HOLE_I64);
    let undefined = blk.bitcast_i64_to_double(crate::nanbox::TAG_UNDEFINED_I64);
    blk.select(I1, &is_hole, DOUBLE, &undefined, &raw)
}

fn lower_trusted_numeric_array_index_get(
    ctx: &mut FnCtx<'_>,
    array_handle: &str,
    idx_i32: &str,
    coerce_numeric_fallback: bool,
) -> String {
    crate::expr::store_census::bump(ctx, crate::expr::store_census::ELEM_READ_OTHER_TIER);
    let blk = ctx.block();
    let idx_i64 = blk.zext(I32, idx_i32, I64);
    let byte_offset = blk.shl(I64, &idx_i64, "3");
    let elements_addr = blk.array_elements_addr(array_handle);
    let element_addr = blk.add(I64, &elements_addr, &byte_offset);
    let element_ptr = blk.inttoptr(I64, &element_addr);
    let raw = blk.load(DOUBLE, &element_ptr);
    if coerce_numeric_fallback {
        // A number-context consumer accepts the same raw-f64-or-holes
        // contract as the established guarded tier. Phase 3 currently installs
        // only the stronger dense numeric descriptor, but retaining the
        // canonicalization here keeps this consumer correct if that admission
        // widens later.
        let is_ordered = blk.fcmp("ord", &raw, &raw);
        blk.select(I1, &is_ordered, DOUBLE, &raw, "0x7FF8000000000000")
    } else {
        raw
    }
}

/// Consume #9254 phase 3's one-time receiver validation at an exact bounded
/// read. `valid_i1` dominates the loop and is invariant; the true arm needs
/// only the refreshed handle load and raw element access, while the false arm
/// is the pre-existing guarded implementation in full.
#[allow(clippy::too_many_arguments)]
pub(super) fn lower_region_validated_array_index_get(
    ctx: &mut FnCtx<'_>,
    arr_id: u32,
    arr_box: &str,
    idx_i32: &str,
    block_prefix: &str,
    require_numeric_layout: bool,
    coerce_numeric_fallback: bool,
    receiver_slot: Option<&str>,
) -> Result<String> {
    let Some(access) = ctx
        .receiver_descriptors
        .array_access(arr_id, require_numeric_layout)
    else {
        return lower_guarded_array_index_get(
            ctx,
            arr_box,
            idx_i32,
            block_prefix,
            require_numeric_layout,
            coerce_numeric_fallback,
            receiver_slot,
        );
    };

    let fast_idx = ctx.new_block(&format!("{}.receiver_region.fast", block_prefix));
    let fallback_idx = ctx.new_block(&format!("{}.receiver_region.fallback", block_prefix));
    let merge_idx = ctx.new_block(&format!("{}.receiver_region.merge", block_prefix));
    let fast_label = ctx.block_label(fast_idx);
    let fallback_label = ctx.block_label(fallback_idx);
    let merge_label = ctx.block_label(merge_idx);
    ctx.block()
        .cond_br(&access.valid_i1, &fast_label, &fallback_label);

    ctx.current_block = fast_idx;
    crate::expr::store_census::bump(ctx, crate::expr::store_census::ELEM_READ_OTHER_TIER);
    let array_handle = ctx.block().load(I64, &access.base_handle_slot);
    let fast_value = if require_numeric_layout {
        lower_trusted_numeric_array_index_get(ctx, &array_handle, idx_i32, coerce_numeric_fallback)
    } else {
        lower_trusted_plain_array_index_get(ctx, &array_handle, idx_i32)
    };
    let fast_end = ctx.block().label.clone();
    ctx.block().br(&merge_label);

    if require_numeric_layout {
        let lowered = LoweredValue {
            semantic: SemanticKind::JsNumber,
            rep: NativeRep::F64,
            llvm_ty: DOUBLE,
            value: fast_value.clone(),
        };
        ctx.record_lowered_value_with_access_mode_and_facts(
            "NumericArrayIndexGet",
            Some(arr_id),
            "receiver_descriptor",
            &lowered,
            Some(BoundsState::Proven {
                proof: BoundsProof::LoopGuard,
            }),
            None,
            Some(BufferAccessMode::CheckedNative),
            None,
            None,
            None,
            vec![raw_f64_layout_fact(
                Some(arr_id),
                "consumed",
                "receiver_descriptor",
                None,
            )],
            Vec::new(),
            false,
            false,
            vec!["receiver_region=validated_once".to_string()],
        );
    }

    ctx.current_block = fallback_idx;
    let fallback_value = lower_guarded_array_index_get(
        ctx,
        arr_box,
        idx_i32,
        &format!("{}.receiver_region.checked", block_prefix),
        require_numeric_layout,
        coerce_numeric_fallback,
        receiver_slot,
    )?;
    let fallback_end = ctx.block().label.clone();
    ctx.block().br(&merge_label);

    ctx.current_block = merge_idx;
    Ok(ctx.block().phi(
        DOUBLE,
        &[(&fast_value, &fast_end), (&fallback_value, &fallback_end)],
    ))
}

/// `POINTER_TAG << 48 | 1 MiB`: subtracted from a NaN-boxed receiver, a heap
/// array handle lands in `[0, HEAP_POINTER_BAND_SPAN)`.
pub(in crate::expr) const HEAP_POINTER_BAND_BASE_I64: &str = "9222527611925692416"; // 0x7FFD_0000_0010_0000
/// `2^48 - 1 MiB`: the handles above the runtime-id band.
pub(in crate::expr) const HEAP_POINTER_BAND_SPAN_I64: &str = "281474975662080"; // 0xFFFF_FFF0_0000
/// The array read's guard mask over the header word `[h-8]` read as an i32:
/// the type byte, `GC_FLAG_FORWARDED` (0x80 in byte 1) and
/// `OBJ_FLAG_ARRAY_DESCRIPTORS` (0x400 in `_reserved`, bytes 2..3).
const ARRAY_READ_GUARD_MASK_I32: &str = "67141887"; // 0x0400_80FF
/// The masked word of a readable array: `GC_TYPE_ARRAY`, every masked flag clear.
const ARRAY_READ_GUARD_EXPECT_I32: &str = "1";
/// The array STORE's guard mask: the read's, plus the integrity bits a
/// write must respect, `FROZEN | SEALED | NO_EXTEND` (0x1..0x4 in `_reserved`).
pub(in crate::expr) const ARRAY_STORE_GUARD_MASK_I32: &str = "67600639"; // 0x0407_80FF
/// The STORE's band: `is_valid_obj_ptr`'s 2^47 ceiling, less the 1 MiB the
/// band is measured from. A store keeps the ceiling the runtime guard applies
/// before it dereferences anything (#7396), at no extra instruction.
pub(in crate::expr) const HEAP_POINTER_STORE_BAND_SPAN_I64: &str = "140737487306752"; // 2^47 - 1 MiB
/// The masked word of a writable array (the read's expectation).
pub(in crate::expr) const ARRAY_STORE_GUARD_EXPECT_I32: &str = ARRAY_READ_GUARD_EXPECT_I32;

/// The GC header's first word, `{obj_type, gc_flags, _reserved}`, of `handle`.
pub(in crate::expr) fn emit_array_guard_word(
    blk: &mut crate::block::LlBlock,
    handle: &str,
) -> String {
    let word_addr = blk.sub(I64, handle, "8");
    let word_ptr = blk.inttoptr(I64, &word_addr);
    blk.load(I32, &word_ptr)
}

/// The GC header's `_reserved` half-word of `handle` (the array's own flags).
fn emit_array_reserved(blk: &mut crate::block::LlBlock, handle: &str) -> String {
    let reserved_addr = blk.sub(I64, handle, "6");
    let reserved_ptr = blk.inttoptr(I64, &reserved_addr);
    blk.load(I16, &reserved_ptr)
}

/// `(word & MASK) == EXPECT`: an ordinary array, not forwarded, no element
/// descriptors.
fn emit_array_guard_word_ok(blk: &mut crate::block::LlBlock, word: &str) -> String {
    let masked = blk.and(I32, word, ARRAY_READ_GUARD_MASK_I32);
    blk.icmp_eq(I32, &masked, ARRAY_READ_GUARD_EXPECT_I32)
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
    let site_id = ctx.typed_feedback_site_id(ctx.ic_site_counter);
    crate::typed_feedback_profile::register_site(
        site_id,
        &ctx.func.name,
        "array_element",
        "array[index]",
    );
    let replay_fact =
        crate::typed_feedback_profile::select_numeric_array(site_id, require_numeric_layout);
    // Preserve the original consumer's coercion contract. A replay hint can
    // select representation handling, but cannot turn a JS-value read into
    // a numeric-context read.
    let coerce_numeric_fallback = require_numeric_layout && coerce_numeric_fallback;
    let require_numeric_layout = require_numeric_layout || replay_fact.is_some();
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
    // Replay selects the existing numeric tier, including its full inline
    // receiver/layout/bounds checks and cold runtime guard. The observation
    // itself never admits a load or suppresses a check.
    let inline_guard = !typed_feedback_emission_enabled();
    let fast_idx = ctx.new_block(&format!("{}.fast", block_prefix));
    let fallback_idx = ctx.new_block(&format!("{}.fallback", block_prefix));
    // A non-negative ordinary-array index at or above `length` has no own
    // element: defining an array-index property would have raised `length`.
    // Once the same structural checks used by the raw-load tier have also
    // proved that there are no indexed descriptors and no indexed prototype
    // properties, that result is `undefined` without consulting the generic
    // polymorphic getter. Sparse-set membership tests hit exactly this arm for
    // absent ids, so keep it separate from the in-bounds raw-load block.
    let inline_oob_idx = if inline_guard {
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
    // The value tier's hole arm (inline guard only): the fast block branches
    // there on a hole instead of selecting `undefined` unguarded.
    let mut fast_hole_label: Option<String> = None;
    let mut fast_hole_branch: Option<String> = None;
    let mut fast_capacity: Option<String> = None;

    if inline_guard {
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
        // ONE header word is the structural guard (DESIGN arrayread §2.2).
        // `[h-8]` read as a little-endian i32 is {obj_type, gc_flags,
        // _reserved}; masked with the type byte, `GC_FLAG_FORWARDED` and
        // `OBJ_FLAG_ARRAY_DESCRIPTORS` it must equal `GC_TYPE_ARRAY`: an
        // ordinary array, not a growth/evacuation stub, with no element
        // descriptors. The rest of the read follows from runtime invariants:
        // * an in-bounds non-hole element is an own data property, so the
        //   prototype facts (the process-wide protector byte and the array's
        //   custom-proto bit) are consulted only on the hole/out-of-bounds arm;
        // * `[length, capacity)` holds `TAG_HOLE` (`array_truncate_length`), so
        //   a value read is bounded by `capacity` and a slot past `length`
        //   reaches the hole arm like any other hole;
        // * `capacity` is under the allocation ceiling (`ARRAY_MAX_CAPACITY`),
        //   so the size-derived element base is exact without a plausibility
        //   bound.
        // A forwarded stub fails the word; the cold arm follows one edge and
        // re-checks the destination's word.
        let deref_idx = ctx.new_block(&format!("{}.guard.deref", block_prefix));
        let deref_label = ctx.block_label(deref_idx);
        let follow_idx = ctx.new_block(&format!("{}.guard.follow", block_prefix));
        let follow_label = ctx.block_label(follow_idx);
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
        let hole_idx = ctx.new_block(&format!("{}.guard.hole", block_prefix));
        let hole_label = ctx.block_label(hole_idx);
        if !require_numeric_layout {
            fast_hole_label = Some(hole_label.clone());
        }
        let band_offset = {
            // POINTER_TAG and a handle above the 1 MiB runtime-id band, as one
            // range compare: `bits - (POINTER_TAG << 48 | 1 MiB) <u 2^48 - 1 MiB`.
            let blk = ctx.block();
            let arr_bits = blk.bitcast_double_to_i64(arr_box);
            let band_offset = blk.sub(I64, &arr_bits, HEAP_POINTER_BAND_BASE_I64);
            let heap_candidate = blk.icmp_ult(I64, &band_offset, HEAP_POINTER_BAND_SPAN_I64);
            blk.cond_br(&heap_candidate, &deref_label, &guard_fail_label);
            band_offset
        };

        ctx.current_block = deref_idx;
        let (arr_handle, word) = {
            let blk = ctx.block();
            // In the band the tag is POINTER_TAG, so the handle is the band
            // offset plus the 1 MiB it was measured from.
            let arr_handle = blk.add(I64, &band_offset, "1048576");
            let word = emit_array_guard_word(blk, &arr_handle);
            let word_ok = emit_array_guard_word_ok(blk, &word);
            blk.cond_br(&word_ok, &range_label, &follow_label);
            (arr_handle, word)
        };
        let deref_end = ctx.block().label.clone();

        ctx.current_block = follow_idx;
        let live_handle = {
            // Array growth and GC evacuation leave the live user address in
            // the first payload word of a forwarded array stub. Follow one
            // edge, then re-check the destination's word. Longer or corrupt
            // chains take the boxed fallback. A word that failed for any
            // other reason selects the original handle and fails again.
            let blk = ctx.block();
            let type_byte = blk.and(I32, &word, "255");
            let is_array = blk.icmp_eq(I32, &type_byte, "1"); // GC_TYPE_ARRAY
            let forwarded_bit = blk.and(I32, &word, "32768"); // GC_FLAG_FORWARDED << 8
            let is_forwarded = blk.icmp_ne(I32, &forwarded_bit, "0");
            let follow_forwarding = blk.and(I1, &is_array, &is_forwarded);
            let original_arr_ptr = blk.inttoptr(I64, &arr_handle);
            let forwarding_target = blk.load(I64, &original_arr_ptr);
            let live_handle =
                blk.select(I1, &follow_forwarding, I64, &forwarding_target, &arr_handle);
            let live_top = blk.lshr(I64, &live_handle, "48");
            let live_top_clear = blk.icmp_eq(I64, &live_top, "0");
            let live_above_handle_band = blk.icmp_ugt(I64, &live_handle, "1048575");
            let live_heap_candidate = blk.and(I1, &live_top_clear, &live_above_handle_band);
            // A forwarding word is not trusted until its address is in the
            // heap band: never dereference a malformed target.
            blk.cond_br(&live_heap_candidate, &live_deref_label, &fallback_label);
            live_handle
        };

        ctx.current_block = live_deref_idx;
        {
            let blk = ctx.block();
            let live_word = emit_array_guard_word(blk, &live_handle);
            let word_ok = emit_array_guard_word_ok(blk, &live_word);
            blk.cond_br(&word_ok, &range_label, &guard_fail_label);
        }
        let live_end = ctx.block().label.clone();

        let numeric_in_bounds_idx = require_numeric_layout
            .then(|| ctx.new_block(&format!("{}.guard.numeric_in_bounds", block_prefix)));
        let numeric_in_bounds_label = numeric_in_bounds_idx.map(|idx| ctx.block_label(idx));
        let oob_label = ctx.block_label(inline_oob_idx.expect("normal-build OOB block"));
        ctx.current_block = range_idx;
        let (range_handle, reserved, bound, range_capacity) = {
            let blk = ctx.block();
            let handle = blk.phi(I64, &[(&arr_handle, &deref_end), (&live_handle, &live_end)]);
            let arr_ptr = blk.inttoptr(I64, &handle);
            // `_reserved` for the numeric tier's layout bits; the value tier
            // reads it on the hole arm only.
            let reserved = if require_numeric_layout {
                Some(emit_array_reserved(blk, &handle))
            } else {
                None
            };
            // A value read is bounded by `capacity` (the hole invariant covers
            // `[length, capacity)`). The numeric tier exposes raw slot bits
            // under a dense-layout proof that covers `[0, length)` only, so it
            // keeps the `length` bound.
            let (bound, range_capacity) = if require_numeric_layout {
                (blk.load(I32, &arr_ptr), None)
            } else {
                let capacity_addr = blk.add(I64, &handle, "4");
                let capacity_ptr = blk.inttoptr(I64, &capacity_addr);
                let capacity = blk.load(I32, &capacity_ptr);
                (capacity.clone(), Some(capacity))
            };
            // Unsigned: a negative index is out of bounds here and is sorted
            // out on the hole arm.
            let index_in_bounds = blk.icmp_ult(I32, idx_i32, &bound);
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
                // for a stored NaN. That reads a hole as undefined without the
                // hole arm, so it also needs the prototype facts.
                let raw_mask = if coerce_numeric_fallback {
                    "4224" // 0x1080 = RAW_F64_LAYOUT | RAW_F64_HOLES
                } else {
                    "128" // dense only: the raw slot is exposed verbatim
                };
                let reserved = reserved.as_deref().expect("numeric tier loads _reserved");
                let raw_bits = blk.and(I16, reserved, raw_mask);
                let is_raw = blk.icmp_ne(I16, &raw_bits, "0");
                let mut in_bounds_ok = blk.and(I1, &index_in_bounds, &is_raw);
                if coerce_numeric_fallback {
                    let default_prototype_chain =
                        crate::expr::array_proto_guard::emit_array_default_prototype_chain(
                            blk, reserved,
                        );
                    in_bounds_ok = blk.and(I1, &in_bounds_ok, &default_prototype_chain);
                }
                // An in-bounds array without the requested numeric layout must
                // still visit the cold rebuilding guard.
                let in_bounds_idx = numeric_in_bounds_idx.expect("numeric in-bounds block");
                let in_bounds_label = numeric_in_bounds_label
                    .as_deref()
                    .expect("numeric in-bounds label");
                blk.cond_br(&index_in_bounds, in_bounds_label, &hole_label);

                ctx.current_block = in_bounds_idx;
                ctx.block()
                    .cond_br(&in_bounds_ok, &fast_label, &guard_fail_label);
                inline_fast_handle = Some((handle.clone(), ctx.block().label.clone()));
            } else {
                inline_fast_handle = Some((handle.clone(), blk.label.clone()));
                blk.cond_br(&index_in_bounds, &fast_label, &hole_label);
            }
            (handle, reserved, bound, range_capacity)
        };
        // The value tier's fast block has the range block as its only
        // predecessor: it computes the element base from this capacity.
        fast_capacity = range_capacity;

        // The hole / out-of-bounds arm. `undefined` needs three facts: a
        // non-negative index (a negative one is a named property), no own
        // element past `capacity` (an over-long `new Array(n)` keeps sparse
        // indices below `length` as named properties), and a prototype chain
        // without index properties. Anything else takes the boxed fallback.
        ctx.current_block = hole_idx;
        crate::expr::store_census::bump(ctx, crate::expr::store_census::ELEM_READ_HOLE);
        {
            let blk = ctx.block();
            let index_negative = blk.icmp_slt(I32, idx_i32, "0");
            let arr_ptr = blk.inttoptr(I64, &range_handle);
            let length = if require_numeric_layout {
                bound.clone()
            } else {
                blk.load(I32, &arr_ptr)
            };
            let capacity_ptr = blk.gep(I8, &arr_ptr, &[(I64, "4")]);
            let capacity = blk.load(I32, &capacity_ptr);
            let past_store = blk.icmp_uge(I32, idx_i32, &capacity);
            let below_length = blk.icmp_ult(I32, idx_i32, &length);
            let sparse_candidate = blk.and(I1, &past_store, &below_length);
            let own_absent = blk.or(I1, &index_negative, &sparse_candidate);
            let own_absent = blk.xor(I1, &own_absent, "true");
            let reserved = match reserved.as_deref() {
                Some(reserved) => reserved.to_string(),
                None => emit_array_reserved(blk, &range_handle),
            };
            let default_prototype_chain =
                crate::expr::array_proto_guard::emit_array_default_prototype_chain(blk, &reserved);
            let undefined_ok = blk.and(I1, &own_absent, &default_prototype_chain);
            blk.cond_br(&undefined_ok, &oob_label, &fallback_label);
        }

        if let Some(cold_idx) = cold_guard_idx {
            // Cold arm: the out-of-line guard rebuilds unmarked-but-numeric
            // arrays into raw-f64 layout (then this call site goes inline on
            // every later read); everything else routes to the boxed fallback.
            ctx.current_block = cold_idx;
            crate::expr::store_census::bump(ctx, crate::expr::store_census::ELEM_READ_COLD);
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
        crate::expr::store_census::bump(ctx, crate::expr::store_census::ELEM_READ_HOLE);
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
    crate::expr::store_census::bump(ctx, crate::expr::store_census::ELEM_READ_FALLBACK);
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
            replay_fact
                .as_ref()
                .map(|fact| vec![format!("typed_feedback_replay_fallback={}", fact.fact_id)])
                .unwrap_or_default(),
        );
    }

    ctx.current_block = fast_idx;
    crate::expr::store_census::bump(ctx, crate::expr::store_census::ELEM_READ_FAST);
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
        let elements_addr = fast_blk.array_elements_addr(&arr_handle);
        let element_addr = fast_blk.add(I64, &elements_addr, &byte_offset);
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
        let elements_addr = match fast_capacity.as_deref() {
            Some(capacity) => fast_blk.array_elements_addr_with_capacity(&arr_handle, capacity),
            None => fast_blk.array_elements_addr(&arr_handle),
        };
        let element_addr = fast_blk.add(I64, &elements_addr, &byte_offset);
        let element_ptr = fast_blk.inttoptr(I64, &element_addr);
        // The slot is read as bits: the hole test is an integer compare.
        let fast_raw_bits = fast_blk.load(I64, &element_ptr);
        let fast_raw = fast_blk.bitcast_i64_to_double(&fast_raw_bits);
        // `new Array(n)` slots are TAG_HOLE internally; JavaScript reads expose
        // `undefined`.
        let is_hole = fast_blk.icmp_eq(I64, &fast_raw_bits, crate::nanbox::TAG_HOLE_I64);
        if fast_hole_label.is_some() {
            // Inline tier: a hole (including any slot past `length`) branches
            // to the hole arm, which owns the prototype facts.
            fast_hole_branch = Some(is_hole);
            fast_raw
        } else {
            let undef_d = fast_blk.bitcast_i64_to_double(crate::nanbox::TAG_UNDEFINED_I64);
            fast_blk.select(I1, &is_hole, DOUBLE, &undef_d, &fast_raw)
        }
    };
    let fast_end_label = fast_blk.label.clone();
    match (fast_hole_label.as_deref(), fast_hole_branch.as_deref()) {
        (Some(hole), Some(is_hole)) => fast_blk.cond_br(is_hole, hole, &merge_label),
        _ => fast_blk.br(&merge_label),
    }
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
            {
                let mut facts = vec![raw_f64_layout_fact(
                    None,
                    "consumed",
                    "numeric_array_index_get_guard",
                    None,
                )];
                if let Some(fact) = &replay_fact {
                    facts.push(fact.clone());
                }
                facts
            },
            Vec::new(),
            false,
            false,
            replay_fact
                .as_ref()
                .map(|_| {
                    vec!["typed_feedback_replay_selected=fresh_numeric_array_observation".into()]
                })
                .unwrap_or_default(),
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
    ctx.receiver_descriptors
        .packed_f64_loop_facts()
        .find(|fact| fact.array_local_id == arr_id && fact.index_local_id == idx_id)
        .cloned()
}

pub(super) fn lower_packed_f64_loop_index_get(
    ctx: &mut FnCtx<'_>,
    arr_id: u32,
    arr_box: &str,
    idx_i32: &str,
    fact: &PackedF64LoopFact,
    bounds_check: bool,
) -> String {
    let guard_id = fact.guard_id.as_str();
    let array_kind = fact.array_kind;
    // A foreign index carries no in-range proof from the loop bound, so test it
    // against the live length (`ArrayHeader.length`, i32 at offset 0 — the same
    // word `expr/index.rs`'s store guard reads) and take the fact's side exit
    // when it fails. One compare and a never-taken branch, against the
    // typed-feedback guard CALL plus boxed fallback this replaces.
    if bounds_check {
        let in_bounds = {
            let blk = ctx.block();
            let arr_bits = blk.bitcast_double_to_i64(arr_box);
            let arr_handle = blk.and(I64, &arr_bits, POINTER_MASK_I64);
            let arr_ptr = blk.inttoptr(I64, &arr_handle);
            let length = blk.load(I32, &arr_ptr);
            blk.icmp_ult(I32, idx_i32, &length)
        };
        let cont_idx = ctx.new_block("packed_f64_loop.foreign.inbounds");
        let cont_label = ctx.block_label(cont_idx);
        ctx.block()
            .cond_br(&in_bounds, &cont_label, &fact.store_side_exit_label);
        ctx.current_block = cont_idx;
    }
    // #9379 proved this clone has no safepoint: the matcher admits no call,
    // closure or await, its reads and writes are bare `double` load/store on
    // existing slots, and the back-edge poll is suppressed for exactly that
    // reason. So the receiver cannot move and its header words cannot change
    // for the clone's whole dynamic extent — take the pre-masked handle from
    // the hoisted receiver cache instead of re-laundering the rooted slot and
    // re-masking it per element. The cache is a plain `i64` alloca nothing in
    // the clone stores to, so the element-base chain hanging off it
    // (`size` at `-4`, `capacity` at `+4`, the shifts and the subtract) becomes
    // loop-invariant to LICM, which the laundered reload deliberately blocked.
    let value = {
        let arr_handle = super::super::receiver_descriptor_handle_i64(ctx, Some(arr_id), arr_box);
        let blk = ctx.block();
        let idx_i64 = blk.zext(I32, idx_i32, I64);
        let byte_offset = blk.shl(I64, &idx_i64, "3");
        let elements_addr = blk.array_elements_addr(&arr_handle);
        let element_addr = blk.add(I64, &elements_addr, &byte_offset);
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
