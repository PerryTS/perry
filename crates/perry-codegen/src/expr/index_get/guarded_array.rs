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
pub(in crate::expr) const HEAP_POINTER_BAND_BASE_I64: &str =
    crate::expr::receiver_range::RECEIVER_BIAS_LITERAL;
/// `2^48 - 1 MiB`: the handles above the runtime-id band.
pub(in crate::expr) const HEAP_POINTER_BAND_SPAN_I64: &str =
    crate::expr::receiver_range::RECEIVER_SPAN_LITERAL;
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
pub(in crate::expr) const HEAP_POINTER_STORE_BAND_SPAN_I64: &str =
    crate::expr::receiver_range::STORE_SPAN_LITERAL;
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

/// What a [`emit_array_region_guard`] receiver needs beyond the read facts.
#[derive(Clone, Copy)]
pub(crate) struct ArrayRegionDense<'a> {
    /// The region stores elements: the integrity bits must be clear too.
    pub(crate) store: bool,
    /// The counter's bound (an `f64`): `bound <= length`.
    pub(crate) len_bound: Option<&'a str>,
}

/// A loop region's array guard (#11650 regions, array slice S3): the S1 guard
/// word, the prototype facts a hole read needs, and `max_index <u capacity`,
/// checked once in the preheader (and at a re-check). On a pass it also
/// derives the element base from the same header and stores it into
/// `base_slot`; F-body's element reads then load `base + 8 * idx` and select
/// `undefined` for a hole. Returns the `i1` pass flag. The receiver is tested
/// against the heap band before anything is dereferenced.
///
/// #10741: a receiver whose region also STORES elements, or indexes them by
/// the loop counter, needs more, and `dense` asks for it: the store word
/// (integrity bits clear), the dense raw-f64 layout bit (every slot in
/// `[0, length)` a canonical double), and the bounds against `length`
/// as well as `capacity` (`max_index < min(length, capacity)`, and
/// `len_bound <= min(length, capacity)` for the counter's bound, an `f64`). A bare read is then one `load double`
/// and a bare store of a proven double one `store double`: neither changes
/// the length, the layout or any pointer the collector traces.
pub(crate) fn emit_array_region_guard(
    ctx: &mut FnCtx<'_>,
    recv_box: &str,
    max_index: u32,
    dense: Option<ArrayRegionDense<'_>>,
    cap_bound: Option<&str>,
    base_slot: &str,
) -> String {
    let deref_idx = ctx.new_block("rloop.arr.deref");
    let cap_idx = ctx.new_block("rloop.arr.cap");
    let join_idx = ctx.new_block("rloop.arr.join");
    let deref_label = ctx.block_label(deref_idx);
    let cap_label = ctx.block_label(cap_idx);
    let join_label = ctx.block_label(join_idx);
    let pre_label = ctx.block().label.clone();
    let band_offset = {
        let blk = ctx.block();
        let bits = blk.bitcast_double_to_i64(recv_box);
        let band_offset = blk.sub(I64, &bits, HEAP_POINTER_BAND_BASE_I64);
        let in_band = blk.icmp_ult(I64, &band_offset, HEAP_POINTER_BAND_SPAN_I64);
        blk.cond_br(&in_band, &deref_label, &join_label);
        band_offset
    };
    ctx.current_block = deref_idx;
    let handle = {
        let blk = ctx.block();
        let handle = blk.add(
            I64,
            &band_offset,
            crate::expr::receiver_range::HANDLE_FLOOR_LITERAL,
        );
        let word = emit_array_guard_word(blk, &handle);
        let word_ok = match dense {
            None => emit_array_guard_word_ok(blk, &word),
            Some(d) => {
                let mask = if d.store {
                    ARRAY_STORE_GUARD_MASK_I32
                } else {
                    ARRAY_READ_GUARD_MASK_I32
                };
                let masked = blk.and(I32, &word, mask);
                let ok = blk.icmp_eq(I32, &masked, ARRAY_READ_GUARD_EXPECT_I32);
                // GC_ARRAY_RAW_F64_LAYOUT (0x80 in `_reserved`, the word's
                // upper half): dense canonical raw f64, no holes.
                let f64_bit = blk.and(I32, &word, "8388608"); // 0x80 << 16
                let is_f64 = blk.icmp_ne(I32, &f64_bit, "0");
                blk.and(I1, &ok, &is_f64)
            }
        };
        blk.cond_br(&word_ok, &cap_label, &join_label);
        handle
    };
    ctx.current_block = cap_idx;
    let (pass, base) = {
        let blk = ctx.block();
        let reserved = emit_array_reserved(blk, &handle);
        let proto_ok =
            crate::expr::array_proto_guard::emit_array_default_prototype_chain(blk, &reserved);
        let capacity_addr = blk.add(I64, &handle, "4");
        let capacity_ptr = blk.inttoptr(I64, &capacity_addr);
        let capacity = blk.load(I32, &capacity_ptr);
        let fits = match dense {
            // Element receivers (no dense facts): every counter value is
            // below `cap_bound`, an `f64`, so `cap_bound <= capacity` keeps
            // each load inside the backing store; a slot in
            // `[length, capacity)` holds a hole (the S0 invariant).
            None => {
                let fits = blk.icmp_ult(I32, &max_index.to_string(), &capacity);
                match cap_bound {
                    Some(bound) => {
                        let cap_f = blk.uitofp(I32, &capacity, DOUBLE);
                        let within = blk.fcmp("ole", bound, &cap_f);
                        blk.and(I1, &fits, &within)
                    }
                    None => fits,
                }
            }
            Some(d) => {
                // #9784: the logical length does not prove the backing
                // store (a presized array's capacity can be smaller): every
                // slot the region touches is below BOTH.
                let length_ptr = blk.inttoptr(I64, &handle);
                let length = blk.load(I32, &length_ptr);
                let shorter = blk.icmp_ult(I32, &length, &capacity);
                let limit = blk.select(I1, &shorter, I32, &length, &capacity);
                let fits = blk.icmp_ult(I32, &max_index.to_string(), &limit);
                match d.len_bound {
                    Some(bound) => {
                        let len_f = blk.uitofp(I32, &limit, DOUBLE);
                        let within = blk.fcmp("ole", bound, &len_f);
                        blk.and(I1, &fits, &within)
                    }
                    None => fits,
                }
            }
        };
        let pass = blk.and(I1, &proto_ok, &fits);
        let base = blk.array_elements_addr_with_capacity(&handle, &capacity);
        blk.br(&join_label);
        (pass, base)
    };
    ctx.current_block = join_idx;
    let blk = ctx.block();
    let pass = blk.phi(
        I1,
        &[
            ("false", &pre_label),
            ("false", &deref_label),
            (&pass, &cap_label),
        ],
    );
    let base = blk.phi(
        I64,
        &[("0", &pre_label), ("0", &deref_label), (&base, &cap_label)],
    );
    blk.store(I64, &base, base_slot);
    pass
}

/// The `Float64Array` twin of [`emit_array_region_guard`]'s dense facts
/// (#10741): a heap pointer whose GC header names a typed array (never
/// forwarded: typed arrays live in the non-moving space), of element kind
/// `Float64`, with an owning byte-cell header (Inline or OutOfLine), and
/// the same bounds against its `length` (payload `+0`). On a pass it stores
/// the element base into `base_slot`; F-body's reads and stores are then the
/// same raw `f64` slots as a dense array's, its reads canonicalising a NaN
/// (a typed array may hold any NaN payload). A typed array's length and kind
/// never change and it is never moved, so nothing but a view creation (JS)
/// can invalidate the facts.
pub(crate) fn emit_typed_f64_region_guard(
    ctx: &mut FnCtx<'_>,
    recv_box: &str,
    max_index: u32,
    len_bound: Option<&str>,
    base_slot: &str,
) -> String {
    let deref_idx = ctx.new_block("rloop.ta.deref");
    let len_idx = ctx.new_block("rloop.ta.len");
    let join_idx = ctx.new_block("rloop.ta.join");
    let deref_label = ctx.block_label(deref_idx);
    let len_label = ctx.block_label(len_idx);
    let join_label = ctx.block_label(join_idx);
    let pre_label = ctx.block().label.clone();
    let band_offset = {
        let blk = ctx.block();
        let bits = blk.bitcast_double_to_i64(recv_box);
        let band_offset = blk.sub(I64, &bits, HEAP_POINTER_BAND_BASE_I64);
        let in_band = blk.icmp_ult(I64, &band_offset, HEAP_POINTER_BAND_SPAN_I64);
        blk.cond_br(&in_band, &deref_label, &join_label);
        band_offset
    };
    ctx.current_block = deref_idx;
    let (handle, word) = {
        let blk = ctx.block();
        let handle = blk.add(
            I64,
            &band_offset,
            crate::expr::receiver_range::HANDLE_FLOOR_LITERAL,
        );
        let word = emit_array_guard_word(blk, &handle);
        let masked = blk.and(I32, &word, &(0xffu32 | (1 << 15)).to_string());
        let ok = blk.icmp_eq(
            I32,
            &masked,
            &crate::expr::byte_cell::brand_for_kind(7).to_string(),
        );
        blk.cond_br(&ok, &len_label, &join_label);
        (handle, word)
    };
    ctx.current_block = len_idx;
    let (pass, base, data_end) = {
        let blk = ctx.block();
        let length_ptr = blk.inttoptr(I64, &handle);
        let length = blk.load(I32, &length_ptr);
        let mut fits = blk.icmp_ult(I32, &max_index.to_string(), &length);
        if let Some(bound) = len_bound {
            let len_f = blk.uitofp(I32, &length, DOUBLE);
            let within = blk.fcmp("ole", bound, &len_f);
            fits = blk.and(I1, &fits, &within);
        }
        let word = blk.zext(I32, &word, I64);
        let base = super::super::byte_cell::owner_data(ctx, &handle, &word, "rloop.ta.data");
        let data_end = ctx.block().label.clone();
        ctx.block().br(&join_label);
        (fits, base, data_end)
    };
    ctx.current_block = join_idx;
    let blk = ctx.block();
    let pass = blk.phi(
        I1,
        &[
            ("false", &pre_label),
            ("false", &deref_label),
            (&pass, &data_end),
        ],
    );
    let base = blk.phi(
        I64,
        &[("0", &pre_label), ("0", &deref_label), (&base, &data_end)],
    );
    blk.store(I64, &base, base_slot);
    pass
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
    lower_guarded_array_index_get_with_index(
        ctx,
        arr_box,
        GuardedArrayIndex::Signed(idx_i32),
        block_prefix,
        require_numeric_layout,
        coerce_numeric_fallback,
        receiver_slot,
    )
}

/// The same indexed read with a compiler proof of a nonnegative u32 index.
/// Retain its original JS number on the cold path, including indices > i32::MAX.
pub(super) fn lower_unsigned_array_index_get(
    ctx: &mut FnCtx<'_>,
    arr_box: &str,
    index: &str,
    receiver_slot: Option<&str>,
) -> Result<String> {
    let idx_i32 = ctx.block().fptoui(DOUBLE, index, I32);
    lower_guarded_array_index_get_with_index(
        ctx,
        arr_box,
        GuardedArrayIndex::Unsigned {
            i32_value: &idx_i32,
            number: index,
        },
        "array_record_index",
        false,
        false,
        receiver_slot,
    )
}

enum GuardedArrayIndex<'a> {
    Signed(&'a str),
    Unsigned { i32_value: &'a str, number: &'a str },
}

fn lower_guarded_array_index_get_with_index(
    ctx: &mut FnCtx<'_>,
    arr_box: &str,
    index: GuardedArrayIndex<'_>,
    block_prefix: &str,
    require_numeric_layout: bool,
    coerce_numeric_fallback: bool,
    receiver_slot: Option<&str>,
) -> Result<String> {
    let (idx_i32, unsigned_index) = match index {
        GuardedArrayIndex::Signed(value) => (value, None),
        GuardedArrayIndex::Unsigned { i32_value, number } => (i32_value, Some(number)),
    };
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
    // Shape is the authority. Own non-hole elements need no prototype test.
    // Forwarding, descriptors, sparse indices and holes use the same cold ABI.
    let fast_idx = ctx.new_block(&format!("{}.fast", block_prefix));
    let fallback_idx = ctx.new_block(&format!("{}.fallback", block_prefix));
    let merge_idx = ctx.new_block(&format!("{}.merge", block_prefix));
    let fast_label = ctx.block_label(fast_idx);
    let fallback_label = ctx.block_label(fallback_idx);
    let merge_label = ctx.block_label(merge_idx);
    let deref_idx = ctx.new_block(&format!("{}.guard.deref", block_prefix));
    let range_idx = ctx.new_block(&format!("{}.guard.range", block_prefix));
    let deref_label = ctx.block_label(deref_idx);
    let range_label = ctx.block_label(range_idx);
    let bits = ctx.block().bitcast_double_to_i64(arr_box);
    let offset = ctx.block().sub(I64, &bits, HEAP_POINTER_BAND_BASE_I64);
    let heap = ctx
        .block()
        .icmp_ult(I64, &offset, HEAP_POINTER_BAND_SPAN_I64);
    ctx.block().cond_br(&heap, &deref_label, &fallback_label);

    ctx.current_block = deref_idx;
    let handle = ctx.block().add(I64, &offset, crate::expr::receiver_range::HANDLE_FLOOR_LITERAL);
    let word = emit_array_guard_word(ctx.block(), &handle);
    let live = emit_array_guard_word_ok(ctx.block(), &word);
    ctx.block().cond_br(&live, &range_label, &fallback_label);

    ctx.current_block = range_idx;
    let blk = ctx.block();
    let ptr = blk.inttoptr(I64, &handle);
    let length = blk.load(I32, &ptr);
    let cap_ptr = blk.gep(I8, &ptr, &[(I64, "4")]);
    let capacity = blk.load(I32, &cap_ptr);
    // Unsigned bounds reject negative keys and presized sparse storage.
    let logical = blk.icmp_ult(I32, idx_i32, &length);
    let backed = blk.icmp_ult(I32, idx_i32, &capacity);
    let mut admitted = blk.and(I1, &logical, &backed);
    if require_numeric_layout {
        // Dense raw-f64 is the existing numeric contract. Holes are cold even
        // in numeric context, so no prototype protector is needed at the site.
        let dense = blk.and(I32, &word, "8388608");
        let dense = blk.icmp_ne(I32, &dense, "0");
        admitted = blk.and(I1, &admitted, &dense);
    }
    if typed_feedback_emission_enabled() {
        // Diagnostic builds retain their established observing guard.
        let guard_fn = if require_numeric_layout {
            "js_typed_feedback_numeric_array_index_get_guard"
        } else {
            "js_typed_feedback_plain_array_index_get_guard"
        };
        let guard = blk.call(
            I32,
            guard_fn,
            &[
                (I64, &feedback_site_id),
                (DOUBLE, arr_box),
                (I32, idx_i32),
                (I32, "1"),
            ],
        );
        let observed = blk.icmp_ne(I32, &guard, "0");
        admitted = blk.and(I1, &admitted, &observed);
    }
    blk.cond_br(&admitted, &fast_label, &fallback_label);

    ctx.current_block = fallback_idx;
    crate::expr::store_census::bump(ctx, crate::expr::store_census::ELEM_READ_FALLBACK);
    let idx_box = unsigned_index
        .map(str::to_owned)
        .unwrap_or_else(|| ctx.block().sitofp(I32, idx_i32, DOUBLE));
    let fallback_boxed = emit_read_cold(
        ctx,
        arr_box,
        idx_i32,
        &idx_box,
        &feedback_site_id,
        true,
        coerce_numeric_fallback,
        receiver_slot,
    );
    let fallback_val = fallback_boxed.clone();
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
    let blk = ctx.block();
    let idx = blk.zext(I32, idx_i32, I64);
    let offset = blk.shl(I64, &idx, "3");
    // Reuse the canonical storage machinery, including shifted/named arrays.
    let base = blk.array_elements_addr_with_capacity(&handle, &capacity);
    let addr = blk.add(I64, &base, &offset);
    let ptr = blk.inttoptr(I64, &addr);
    let bits = blk.load(I64, &ptr);
    let fast_val = blk.bitcast_i64_to_double(&bits);
    let fast_end_label = blk.label.clone();
    if require_numeric_layout {
        blk.br(&merge_label);
    } else {
        let hole = blk.icmp_eq(I64, &bits, crate::nanbox::TAG_HOLE_I64);
        if crate::expr::store_census::enabled() {
            let hole_idx = ctx.new_block(&format!("{}.census.hole", block_prefix));
            let hole_label = ctx.block_label(hole_idx);
            ctx.block().cond_br(&hole, &hole_label, &merge_label);
            ctx.current_block = hole_idx;
            crate::expr::store_census::bump(ctx, crate::expr::store_census::ELEM_READ_HOLE);
            ctx.block().br(&fallback_label);
        } else {
            ctx.block().cond_br(&hole, &fallback_label, &merge_label);
        }
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
    Ok(ctx.block().phi(
        DOUBLE,
        &[
            (&fast_val, &fast_end_label),
            (&fallback_val, &fallback_end_label),
        ],
    ))
}

/// One link-coalesced cold body for the operation. It keeps the exact generic
/// property-key getter and numeric consumer's coercion, without a second
/// numeric-layout promotion path. No site or module duplicates the body.
pub(super) fn emit_read_cold(
    ctx: &mut FnCtx<'_>,
    receiver: &str,
    _index: &str,
    boxed_index: &str,
    site: &str,
    numeric_layout: bool,
    coerce: bool,
    _receiver_slot: Option<&str>,
) -> String {
    let name = "perry_array_read_cold_shared";
    if !ctx.pending_helpers.iter().any(|f| f.name == name) {
        let mut helper = crate::function::LlFunction::new(
            name,
            DOUBLE,
            vec![
                (DOUBLE, "%receiver".into()),
                (DOUBLE, "%key".into()),
                (I64, "%site".into()),
                (I1, "%numeric".into()),
                (I1, "%coerce".into()),
            ],
        );
        // Identical across modules: the linker keeps one body per program.
        helper.linkage = "linkonce_odr".into();
        helper.no_inline = true;
        let saved_func = std::mem::replace(ctx.func, helper);
        let saved_block = ctx.current_block;
        ctx.current_block = ctx.new_block("entry");
        crate::expr::store_census::bump(ctx, crate::expr::store_census::ELEM_READ_COLD);
        let keyed = ctx.new_block("runtime_key");
        let numeric = ctx.new_block("numeric_miss");
        let convert = ctx.new_block("convert");
        let done = ctx.new_block("return");
        let kl = ctx.block_label(keyed);
        let nl = ctx.block_label(numeric);
        let cl = ctx.block_label(convert);
        let dl = ctx.block_label(done);
        ctx.block().cond_br("%numeric", &nl, &kl);
        ctx.current_block = keyed;
        let keyed_value =
            super::runtime_key::emit_claimable_array_key_body(ctx, "%receiver", "%key");
        let kp = ctx.block().label.clone();
        ctx.block().br(&cl);
        ctx.current_block = numeric;
        let boxed = ctx.block().call(
            DOUBLE,
            "js_typed_feedback_array_index_get_fallback_boxed",
            &[(I64, "%site"), (DOUBLE, "%receiver"), (DOUBLE, "%key")],
        );
        let np = ctx.block().label.clone();
        ctx.block().br(&cl);
        ctx.current_block = convert;
        let value = ctx
            .block()
            .phi(DOUBLE, &[(&boxed, &np), (&keyed_value, &kp)]);
        let cp = ctx.block().label.clone();
        let coercion = ctx.new_block("coercion");
        let col = ctx.block_label(coercion);
        ctx.block().cond_br("%coerce", &col, &dl);
        ctx.current_block = coercion;
        let numeric = ctx
            .block()
            .call(DOUBLE, "js_number_coerce", &[(DOUBLE, &value)]);
        let np = ctx.block().label.clone();
        ctx.block().br(&dl);
        ctx.current_block = done;
        let result = ctx.block().phi(DOUBLE, &[(&value, &cp), (&numeric, &np)]);
        ctx.block().ret(DOUBLE, &result);
        let helper = std::mem::replace(ctx.func, saved_func);
        ctx.current_block = saved_block;
        ctx.pending_helpers.push(helper);
    }
    ctx.block().call(
        DOUBLE,
        name,
        &[
            (DOUBLE, receiver),
            (DOUBLE, boxed_index),
            (I64, site),
            (I1, if numeric_layout { "true" } else { "false" }),
            (I1, if coerce { "true" } else { "false" }),
        ],
    )
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

/// An entry-proven ordinary array and the record's unsigned cursor use the
/// same header word and slot load as this backend. All non-packed operations
/// share its established runtime getter through the record's cold ABI.
pub(crate) fn lower_record_index(
    ctx: &mut FnCtx<'_>,
    array: &str,
    index: &str,
    record: &str,
    payload_slot: &str,
    index_slot: &str,
) -> Result<String> {
    let fast = ctx.new_block("record.read.fast");
    let load = ctx.new_block("record.read.load");
    let cold = ctx.new_block("record.read.cold");
    let merge = ctx.new_block("record.read.merge");
    let fl = ctx.block_label(fast);
    let ll = ctx.block_label(load);
    let cl = ctx.block_label(cold);
    let ml = ctx.block_label(merge);
    let bits = ctx.block().bitcast_double_to_i64(array);
    let handle = ctx.block().and(I64, &bits, crate::nanbox::POINTER_MASK_I64);
    let word = emit_array_guard_word(ctx.block(), &handle);
    let safe = emit_array_guard_word_ok(ctx.block(), &word);
    let invalidated = ctx
        .block()
        .load(I8, "@PERRY_ARRAY_INDEX_FAST_PATH_INVALIDATED");
    let prototype_clear = ctx.block().icmp_eq(I8, &invalidated, "0");
    let safe = ctx.block().and(I1, &safe, &prototype_clear);
    ctx.block().cond_br(&safe, &fl, &cl);
    ctx.current_block = fast;
    let i = ctx.block().fptoui(DOUBLE, index, I32);
    let length_ptr = ctx.block().inttoptr(I64, &handle);
    let length = ctx.block().load(I32, &length_ptr);
    let in_bounds = ctx.block().icmp_ult(I32, &i, &length);
    ctx.block().cond_br(&in_bounds, &ll, &cl);
    ctx.current_block = load;
    let value = lower_trusted_plain_array_index_get(ctx, &handle, &i);
    let bits = ctx.block().bitcast_double_to_i64(&value);
    let hole = ctx.block().icmp_eq(I64, &bits, crate::nanbox::TAG_HOLE_I64);
    let fp = ctx.block().label.clone();
    ctx.block().cond_br(&hole, &cl, &ml);
    ctx.current_block = cold;
    ctx.block().store_volatile(DOUBLE, array, payload_slot);
    ctx.block().store(DOUBLE, index, index_slot);
    let value_cold = ctx.block().call(
        DOUBLE,
        "js_array_record_stack_dispatch",
        &[(crate::types::PTR, record), (I32, "5")],
    );
    let cp = ctx.block().label.clone();
    ctx.block().br(&ml);
    ctx.current_block = merge;
    Ok(ctx
        .block()
        .phi(DOUBLE, &[(&value, &fp), (&value_cold, &cp)]))
}

/// One guard for the record's live length and value. The cold operation owns
/// both ArrayIterator Get and the captured protocol's IteratorStepValue.
pub(crate) fn lower_record_next(
    ctx: &mut FnCtx<'_>,
    array: &str,
    index: &str,
    protocol: &str,
    record: &str,
    index_slot: &str,
    value_slot: &str,
) -> Result<(String, String)> {
    let guard = ctx.new_block("record.next.guard");
    let bounds = ctx.new_block("record.next.bounds");
    let load = ctx.new_block("record.next.load");
    let exhausted = ctx.new_block("record.next.exhausted");
    let cold = ctx.new_block("record.next.cold");
    let merge = ctx.new_block("record.next.merge");
    let gl = ctx.block_label(guard);
    let bl = ctx.block_label(bounds);
    let ll = ctx.block_label(load);
    let el = ctx.block_label(exhausted);
    let cl = ctx.block_label(cold);
    let ml = ctx.block_label(merge);
    ctx.block().cond_br(protocol, &cl, &gl);
    ctx.current_block = guard;
    let bits = ctx.block().bitcast_double_to_i64(array);
    let handle = ctx.block().and(I64, &bits, crate::nanbox::POINTER_MASK_I64);
    let word = emit_array_guard_word(ctx.block(), &handle);
    let safe = emit_array_guard_word_ok(ctx.block(), &word);
    let invalidated = ctx
        .block()
        .load(I8, "@PERRY_ARRAY_INDEX_FAST_PATH_INVALIDATED");
    let clear = ctx.block().icmp_eq(I8, &invalidated, "0");
    let safe = ctx.block().and(I1, &safe, &clear);
    ctx.block().cond_br(&safe, &bl, &cl);
    ctx.current_block = bounds;
    let i = ctx.block().fptoui(DOUBLE, index, I32);
    let ptr = ctx.block().inttoptr(I64, &handle);
    let len = ctx.block().load(I32, &ptr);
    let ready = ctx.block().icmp_ult(I32, &i, &len);
    ctx.block().cond_br(&ready, &ll, &el);
    ctx.current_block = load;
    let fast_value = lower_trusted_plain_array_index_get(ctx, &handle, &i);
    let fp = ctx.block().label.clone();
    ctx.block().br(&ml);
    ctx.current_block = exhausted;
    let absent = crate::nanbox::double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED));
    let ep = ctx.block().label.clone();
    ctx.block().br(&ml);
    ctx.current_block = cold;
    ctx.block().store(DOUBLE, index, index_slot);
    let done = ctx.block().call(
        DOUBLE,
        "js_array_record_stack_dispatch",
        &[(crate::types::PTR, record), (I32, "6")],
    );
    let bits = ctx.block().bitcast_double_to_i64(&done);
    let cold_ready = ctx
        .block()
        .icmp_eq(I64, &bits, crate::nanbox::TAG_FALSE_I64);
    let cold_value = ctx.block().load_volatile(DOUBLE, value_slot);
    ctx.block().store_volatile(DOUBLE, &absent, value_slot);
    let cp = ctx.block().label.clone();
    ctx.block().br(&ml);
    ctx.current_block = merge;
    let value = ctx.block().phi(
        DOUBLE,
        &[(&fast_value, &fp), (&absent, &ep), (&cold_value, &cp)],
    );
    let ready = ctx
        .block()
        .phi(I1, &[("true", &fp), ("false", &ep), (&cold_ready, &cp)]);
    Ok((value, ready))
}
#[cfg(test)]
mod compact_read_tests {
    use crate::{compile_module, CompileOptions};
    use perry_hir::types::Type;
    use perry_hir::{Expr, Function, Module, Param, Stmt};

    fn ir(body: Vec<Stmt>) -> String {
        let mut module = Module::new("compact_array_reads");
        module.functions.push(Function {
            id: 1,
            name: "read".into(),
            type_params: vec![],
            params: vec![
                Param {
                    id: 1,
                    name: "array".into(),
                    ty: Type::Array(Box::new(Type::Any)),
                    default: None,
                    decorators: vec![],
                    is_rest: false,
                    arguments_object: None,
                },
                Param {
                    id: 2,
                    name: "key".into(),
                    ty: Type::Any,
                    default: None,
                    decorators: vec![],
                    is_rest: false,
                    arguments_object: None,
                },
            ],
            return_type: Type::Any,
            body,
            is_async: false,
            is_generator: false,
            is_strict: true,
            is_exported: true,
            captures: vec![],
            decorators: vec![],
            was_plain_async: false,
            was_unrolled: false,
        });
        let ll = String::from_utf8(
            compile_module(
                &module,
                CompileOptions {
                    emit_ir_only: true,
                    ..Default::default()
                },
            )
            .unwrap(),
        )
        .unwrap();
        #[cfg(feature = "llvm-inprocess")]
        crate::testing::verify_ir(&ll, "compact_array_reads").unwrap();
        ll
    }

    #[test]
    fn compact_array_read_guards_own_slot_and_sends_holes_cold() {
        let ll = ir(vec![Stmt::Return(Some(Expr::IndexGet {
            object: Box::new(Expr::LocalGet(1)),
            index: Box::new(Expr::Integer(1)),
        }))]);
        assert!(
            ll.contains("67141887"),
            "type, forwarding and descriptor word"
        );
        assert!(ll.contains("icmp ult i32"), "logical and backing bounds");
        assert!(
            ll.contains(crate::nanbox::TAG_HOLE_I64),
            "hole must side-exit"
        );
        assert!(ll.contains("call double @perry_array_read_cold_shared"));
        assert!(ll.contains("@js_typed_feedback_array_index_get_fallback_boxed"));
        assert!(!ll.contains("call double @js_array_refresh_local_head"));
        assert!(!ll.contains("guard.follow") && !ll.contains("guard.oob"));
        assert!(!ll.contains("load i8, ptr @PERRY_ARRAY_INDEX_FAST_PATH_INVALIDATED"));
    }

    #[test]
    fn compact_array_length_uses_header_and_generic_miss() {
        let ll = ir(vec![Stmt::Return(Some(Expr::PropertyGet {
            object: Box::new(Expr::LocalGet(1)),
            property: "length".into(),
            byte_offset: 0,
        }))]);
        assert!(ll.contains("plen.check_gc") && ll.contains("33023"));
        assert!(ll.contains("uitofp i32"));
        assert!(ll.contains("call double @js_value_length_property_f64"));
        assert!(
            !ll.contains("perry_length_cold_") && !ll.contains("call i64 @js_array_length_leaf")
        );
    }

    #[test]
    fn rejected_keys_share_one_body_across_sites() {
        let read = || Expr::IndexGet {
            object: Box::new(Expr::LocalGet(1)),
            index: Box::new(Expr::LocalGet(2)),
        };
        let ll = ir(vec![Stmt::Expr(read()), Stmt::Return(Some(read()))]);
        assert_eq!(
            ll.lines()
                .filter(|l| l.starts_with("define ") && l.contains("@perry_array_read_cold_"))
                .count(),
            1
        );
        assert!(ll.contains("define linkonce_odr double @perry_array_read_cold_shared"));
        assert!(!ll.contains("perry_array_key_cold_"));
        assert!(
            !ll.contains("call i32 @js_typed_feedback_numeric_array_index_get_guard"),
            "plain-key misses must not retain the numeric promotion runtime"
        );
    }
}
