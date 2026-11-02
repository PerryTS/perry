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
use crate::types::{DOUBLE, I1, I32, I64, I8};

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
    let raw = lower_trusted_array_slot(ctx, array_handle, idx_i32);
    let blk = ctx.block();
    let raw_bits = blk.bitcast_double_to_i64(&raw);
    let is_hole = blk.icmp_eq(I64, &raw_bits, crate::nanbox::TAG_HOLE_I64);
    let undefined = blk.bitcast_i64_to_double(crate::nanbox::TAG_UNDEFINED_I64);
    blk.select(I1, &is_hole, DOUBLE, &undefined, &raw)
}

/// Load storage without changing a hole into undefined. Callers either prove
/// an own data element (the dense prefix) or test the raw hole before Get's
/// prototype-sensitive fallback.
fn lower_trusted_array_slot(ctx: &mut FnCtx<'_>, array_handle: &str, idx_i32: &str) -> String {
    crate::expr::store_census::bump(ctx, crate::expr::store_census::ELEM_READ_OTHER_TIER);
    let blk = ctx.block();
    let idx_i64 = blk.zext(I32, idx_i32, I64);
    let byte_offset = blk.shl(I64, &idx_i64, "3");
    let elements_addr = blk.array_elements_addr(array_handle);
    let element_addr = blk.add(I64, &elements_addr, &byte_offset);
    let element_ptr = blk.inttoptr(I64, &element_addr);
    blk.load(DOUBLE, &element_ptr)
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

/// `POINTER_TAG << 48 | 2 MiB`: subtracted from a NaN-boxed receiver, a heap
/// array handle lands in `[0, HEAP_POINTER_BAND_SPAN)`.
pub(in crate::expr) const HEAP_POINTER_BAND_BASE_I64: &str =
    crate::expr::receiver_range::RECEIVER_BIAS_LITERAL;
/// `2^48 - 2 MiB`: the handles above the runtime-id band.
pub(in crate::expr) const HEAP_POINTER_BAND_SPAN_I64: &str =
    crate::expr::receiver_range::RECEIVER_SPAN_LITERAL;
/// The array read's guard mask over the header word `[h-8]` read as an i32:
/// the type byte and `GC_FLAG_FORWARDED` (0x80 in byte 1). Element
/// descriptors live in the array's named layout, tested beside the word
/// (`emit_array_has_named_layout`).
const ARRAY_READ_GUARD_MASK_I32: &str = "33023"; // 0x0000_80FF
/// The masked word of a readable array: `GC_TYPE_ARRAY`, every masked flag clear.
const ARRAY_READ_GUARD_EXPECT_I32: &str = "1";
/// The array STORE's guard mask: the read's, plus the integrity bits a
/// write must respect, `FROZEN | SEALED | NO_EXTEND` (0x1..0x4 in `_reserved`).
pub(in crate::expr) const ARRAY_STORE_GUARD_MASK_I32: &str = "491775"; // 0x0007_80FF
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

/// `(word & MASK) == EXPECT` and a null named layout: an ordinary array, not
/// forwarded, no element descriptors.
pub(in crate::expr) fn emit_array_guard_word_ok(
    blk: &mut crate::block::LlBlock,
    word: &str,
    handle: &str,
) -> String {
    let masked = blk.and(I32, word, ARRAY_READ_GUARD_MASK_I32);
    let word_ok = blk.icmp_eq(I32, &masked, ARRAY_READ_GUARD_EXPECT_I32);
    emit_and_no_named_layout(blk, &word_ok, handle)
}

/// `(word & MASK) == EXPECT`: an ordinary, unforwarded array. The part of
/// the read guard the dense-prefix test needs before it loads P.
fn emit_array_guard_word_typed(blk: &mut crate::block::LlBlock, word: &str) -> String {
    let masked = blk.and(I32, word, ARRAY_READ_GUARD_MASK_I32);
    blk.icmp_eq(I32, &masked, ARRAY_READ_GUARD_EXPECT_I32)
}

/// The confined guard's second half (see [`emit_confined_array_guard_word_ok`]):
/// the default prototype chain was never modified.
fn emit_confined_chain_pristine(blk: &mut crate::block::LlBlock) -> String {
    let chain = blk.load_volatile(crate::types::I8, "@PERRY_ARRAY_INDEX_FAST_PATH_INVALIDATED");
    blk.icmp_eq(crate::types::I8, &chain, "0")
}

/// The guard for a CONFINED receiver (`collectors::definite_init::confined`):
/// an ordinary, unforwarded array, and a default prototype chain that has never
/// been modified. Only the array's construction and element/`length` reads ever
/// name a confined array, so the one way user code could have defined an
/// element descriptor on it is an accessor or patched method on
/// `%Array.prototype%`/`%Object.prototype%` running with it as `this`; any
/// such modification sets the (sticky) invalidation byte. That byte test
/// replaces the receiver's descriptor-layout query, so it is used only in a
/// program that emits that query (`program_writes_element_descriptors`).
fn emit_confined_array_guard_word_ok(blk: &mut crate::block::LlBlock, word: &str) -> String {
    let masked = blk.and(I32, word, ARRAY_READ_GUARD_MASK_I32);
    let word_ok = blk.icmp_eq(I32, &masked, ARRAY_READ_GUARD_EXPECT_I32);
    let chain = blk.load_volatile(crate::types::I8, "@PERRY_ARRAY_INDEX_FAST_PATH_INVALIDATED");
    let pristine = blk.icmp_eq(crate::types::I8, &chain, "0");
    blk.and(crate::types::I1, &word_ok, &pristine)
}

/// `ok` and "the array at `handle` has no named layout" (no element
/// descriptors can intercept a raw slot access).
pub(in crate::expr) fn emit_and_no_named_layout(
    blk: &mut crate::block::LlBlock,
    ok: &str,
    handle: &str,
) -> String {
    let has_named = crate::expr::array_proto_guard::emit_array_element_descriptors(blk, handle);
    let no_named = blk.xor(crate::types::I1, &has_named, "true");
    blk.and(crate::types::I1, ok, &no_named)
}

/// Every caller already has a nonnegative signed-i32 index: the canonical
/// runtime-key split, numeric_index_has_integer_array_index_proof, or the
/// existing bounded receiver descriptor. No source-expression proof is added.
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
    let canonical = matches!(index, GuardedArrayIndex::Signed(_));
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
    let exotic_idx = ctx.new_block(&format!("{}.cold.exact_args", block_prefix));
    let absence_idx = ctx.new_block(&format!("{}.cold.absent_args", block_prefix));
    let exotic_label = ctx.block_label(exotic_idx);
    let absence_label = ctx.block_label(absence_idx);
    let deref_idx = ctx.new_block(&format!("{}.guard.deref", block_prefix));
    let range_idx = ctx.new_block(&format!("{}.guard.range", block_prefix));
    let backing_idx = ctx.new_block(&format!("{}.guard.backing", block_prefix));
    let deref_label = ctx.block_label(deref_idx);
    let range_label = ctx.block_label(range_idx);
    let backing_label = ctx.block_label(backing_idx);
    let bits = ctx.block().bitcast_double_to_i64(arr_box);
    let offset = ctx.block().sub(I64, &bits, HEAP_POINTER_BAND_BASE_I64);
    let heap = ctx
        .block()
        .icmp_ult(I64, &offset, HEAP_POINTER_BAND_SPAN_I64);
    ctx.block().cond_br(&heap, &deref_label, &exotic_label);

    ctx.current_block = deref_idx;
    let handle = if ctx.target_triple.starts_with("x86_64") {
        let blk = ctx.block();
        // The band proves the pointer tag. Keep the resulting native handle
        // opaque to constant folding: otherwise LLVM rematerializes separate
        // 64-bit boxed-address biases for the header and every storage load.
        // This existing tied-register identity emits no machine instruction;
        // it cannot allocate or collect. Other targets retain their mask.
        let raw = blk.sub(I64, &bits, crate::nanbox::POINTER_TAG_I64);
        let handle = blk.fresh_reg();
        blk.emit_raw(format!(
            "  {handle} = call i64 asm \"\", \"=r,0\"(i64 {raw}) \"gc-leaf-function\""
        ));
        handle
    } else {
        ctx.block().and(I64, &bits, POINTER_MASK_I64)
    };
    let word = emit_array_guard_word(ctx.block(), &handle);
    // The confined guard replaces the descriptor query; a program without a
    // descriptor writer emits no query, so the plain guard is already minimal.
    let confined = ctx.confined_array_read
        && crate::expr::array_proto_guard::program_writes_element_descriptors();
    // The dense-prefix hit (EK design §4): an ordinary, unforwarded array
    // and `index <u P`. Every slot below P is a non-hole own data element
    // without a descriptor and below `min(length, capacity)`, so the hit
    // needs no descriptor query, no length or capacity bound and no hole
    // test. A miss takes the full guard below, unchanged. Diagnostic builds
    // keep their observing guard on every read.
    let prefix_hit = if typed_feedback_emission_enabled() {
        let live = if confined {
            emit_confined_array_guard_word_ok(ctx.block(), &word)
        } else {
            emit_array_guard_word_ok(ctx.block(), &word, &handle)
        };
        ctx.block().cond_br(&live, &range_label, &exotic_label);
        None
    } else {
        let prefix_idx = ctx.new_block(&format!("{}.guard.prefix", block_prefix));
        let hit_idx = ctx.new_block(&format!("{}.prefix.hit", block_prefix));
        let rest_idx = ctx.new_block(&format!("{}.guard.rest", block_prefix));
        let prefix_label = ctx.block_label(prefix_idx);
        let hit_label = ctx.block_label(hit_idx);
        let rest_label = ctx.block_label(rest_idx);
        let typed = emit_array_guard_word_typed(ctx.block(), &word);
        ctx.block().cond_br(&typed, &prefix_label, &exotic_label);
        ctx.current_block = prefix_idx;
        let prefix = ctx.block().array_ek_prefix(&handle);
        let in_prefix = ctx.block().icmp_ult(I32, idx_i32, &prefix);
        ctx.block().cond_br(&in_prefix, &hit_label, &rest_label);
        ctx.current_block = rest_idx;
        let live = if confined {
            emit_confined_chain_pristine(ctx.block())
        } else {
            emit_and_no_named_layout(ctx.block(), "true", &handle)
        };
        ctx.block().cond_br(&live, &range_label, &exotic_label);
        Some(hit_idx)
    };

    ctx.current_block = range_idx;
    let blk = ctx.block();
    let ptr = blk.inttoptr(I64, &handle);
    let length = blk.load(I32, &ptr);
    // Distinct miss predecessors carry a constant absence mode. Computing a
    // select of modes here made LLVM emit setcc/lea on every packed hit.
    let logical = blk.icmp_ult(I32, idx_i32, &length);
    blk.cond_br(&logical, &backing_label, &absence_label);

    ctx.current_block = backing_idx;
    let blk = ctx.block();
    let cap_ptr = blk.gep(I8, &ptr, &[(I64, "4")]);
    let capacity = blk.load(I32, &cap_ptr);
    // Numeric context does not consult the array layout bit: a plain double
    // has the same bits in a raw and in a boxed array, so the loaded element
    // carries its own proof (tested after the load, below). Holes, INT32
    // boxes and every tagged value miss that test and take the shared cold
    // helper, so no prototype protector is needed at the site.
    let mut admitted = blk.icmp_ult(I32, idx_i32, &capacity);
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
    blk.cond_br(&admitted, &fast_label, &exotic_label);

    // Carry the existing guard's absence proof to the shared operation. No
    // receiver/key classification or own-slot read is repeated for that arm.
    let census_hole_idx = (!require_numeric_layout && crate::expr::store_census::enabled())
        .then(|| ctx.new_block(&format!("{}.census.hole", block_prefix)));
    // Materialize cold arguments only after a failed guard. The existing
    // empty tied-register asm has no machine instruction; its sideeffect
    // marker prevents CFG/phi lowering from pulling these moves onto hits.
    ctx.current_block = exotic_idx;
    let exact_control = ctx.block().fresh_reg();
    ctx.block().emit_raw(format!(
        "  {exact_control} = call i64 asm sideeffect \"# array-get exact args\", \"=r,0\"(i64 2) \"gc-leaf-function\""
    ));
    ctx.block().br(&fallback_label);
    ctx.current_block = absence_idx;
    let absent_control = ctx.block().fresh_reg();
    let admitted_control = if canonical { handle.as_str() } else { "4" };
    ctx.block().emit_raw(format!(
        "  {absent_control} = call i64 asm sideeffect \"# array-get absent args\", \"=r,0\"(i64 {admitted_control}) \"gc-leaf-function\""
    ));
    ctx.block().br(&fallback_label);
    let cold_inputs = [
        (&exact_control[..], exotic_label.as_str()),
        (&absent_control[..], absence_label.as_str()),
    ];
    ctx.current_block = fallback_idx;
    let cold_mode = ctx.block().phi(I64, &cold_inputs);
    crate::expr::store_census::bump(ctx, crate::expr::store_census::ELEM_READ_FALLBACK);
    let idx_box = unsigned_index
        .map(str::to_owned)
        .unwrap_or_else(|| ctx.block().sitofp(I32, idx_i32, DOUBLE));
    let fallback_boxed = emit_read_cold(
        ctx,
        arr_box,
        &cold_mode,
        &idx_box,
        &feedback_site_id,
        true,
        coerce_numeric_fallback,
        unsigned_index.and(receiver_slot),
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
    // Reuse the canonical backing geometry, including shifted/named arrays.
    let addr = blk.array_element_addr_with_capacity(&handle, &capacity, idx_i32);
    let ptr = blk.inttoptr(I64, &addr);
    let bits = blk.load(I64, &ptr);
    let fast_val = blk.bitcast_i64_to_double(&bits);
    let fast_end_label = blk.label.clone();
    if require_numeric_layout {
        // The element is the proof (types never prove): the one emitted
        // is_number predicate. A miss shares the exact-args cold arm.
        let is_number = crate::codegen::emit_plain_number_bits_test(blk, &bits);
        blk.cond_br(&is_number, &merge_label, &exotic_label);
    } else {
        let hole = blk.icmp_eq(I64, &bits, crate::nanbox::TAG_HOLE_I64);
        if crate::expr::store_census::enabled() {
            let hole_idx = census_hole_idx.expect("hole census predecessor");
            let hole_label = ctx.block_label(hole_idx);
            ctx.block().cond_br(&hole, &hole_label, &merge_label);
            ctx.current_block = hole_idx;
            crate::expr::store_census::bump(ctx, crate::expr::store_census::ELEM_READ_HOLE);
            ctx.block().br(&absence_label);
        } else {
            ctx.block().cond_br(&hole, &absence_label, &merge_label);
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

    let mut incoming = vec![
        (fast_val, fast_end_label),
        (fallback_val, fallback_end_label),
    ];
    if let Some(hit_idx) = prefix_hit {
        ctx.current_block = hit_idx;
        crate::expr::store_census::bump(ctx, crate::expr::store_census::ELEM_READ_PREFIX);
        let blk = ctx.block();
        let ptr = blk.inttoptr(I64, &handle);
        let cap_ptr = blk.gep(I8, &ptr, &[(I64, "4")]);
        let capacity = blk.load(I32, &cap_ptr);
        let addr = blk.array_element_addr_with_capacity(&handle, &capacity, idx_i32);
        let ptr = blk.inttoptr(I64, &addr);
        let bits = blk.load(I64, &ptr);
        let hit_val = blk.bitcast_i64_to_double(&bits);
        let hit_end = blk.label.clone();
        if require_numeric_layout {
            // P proves a non-hole element, not a Number: the numeric tier
            // keeps its one is_number test on the loaded element.
            let is_number = crate::codegen::emit_plain_number_bits_test(blk, &bits);
            blk.cond_br(&is_number, &merge_label, &exotic_label);
        } else {
            blk.br(&merge_label);
        }
        incoming.push((hit_val, hit_end));
    }

    ctx.current_block = merge_idx;
    let incoming: Vec<(&str, &str)> = incoming
        .iter()
        .map(|(v, l)| (v.as_str(), l.as_str()))
        .collect();
    Ok(ctx.block().phi(DOUBLE, &incoming))
}

/// One runtime cold body for the operation. The mode carries only this
/// guard's existing own-absence proof; exotic keys/receivers retain their
/// exact getter and numeric coercion. No site or module duplicates the body.
pub(super) fn emit_read_cold(
    ctx: &mut FnCtx<'_>,
    receiver: &str,
    mode: &str,
    boxed_index: &str,
    site: &str,
    _numeric_layout: bool,
    coerce: bool,
    _receiver_slot: Option<&str>,
) -> String {
    crate::expr::store_census::bump(ctx, crate::expr::store_census::ELEM_READ_COLD);
    // Low bit carries coercion for either a small operation code or the
    // already-admitted, aligned array pointer. Pointer controls reuse the
    // canonical split's nonnegative check and the existing receiver guard.
    let flags = if coerce {
        ctx.block().or(I64, mode, "1")
    } else {
        mode.to_string()
    };
    let scratch = (_receiver_slot.is_some() || typed_feedback_emission_enabled())
        .then(|| ctx.func.alloca_entry_array(I64, 3));
    let (control, source) = if let Some(scratch) = &scratch {
        ctx.block().store(I64, site, scratch);
        let flags_ptr = ctx.block().gep(I8, scratch, &[(I64, "8")]);
        ctx.block().store(I64, &flags, &flags_ptr);
        let source = ctx.block().gep(I8, scratch, &[(I64, "16")]);
        let pointer = ctx.block().ptrtoint(scratch, I64);
        let control = ctx.block().or(
            I64,
            &pointer,
            if _receiver_slot.is_some() { "6" } else { "2" },
        );
        (control, Some(source))
    } else {
        (flags, None)
    };
    let value = ctx.block().call(
        DOUBLE,
        "perry_array_read_cold_shared",
        &[(DOUBLE, receiver), (DOUBLE, boxed_index), (I64, &control)],
    );
    if let (Some(source), Some(root)) = (source, _receiver_slot) {
        let live = ctx.block().load(DOUBLE, &source);
        ctx.block().store(DOUBLE, &live, root);
    }
    value
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
    let prefix = ctx.new_block("record.read.prefix");
    let hit = ctx.new_block("record.read.prefix.hit");
    let rest = ctx.new_block("record.read.rest");
    let fast = ctx.new_block("record.read.fast");
    let load = ctx.new_block("record.read.load");
    let hole_check = ctx.new_block("record.read.hole");
    let absent = ctx.new_block("record.read.absent");
    let cold = ctx.new_block("record.read.cold");
    let merge = ctx.new_block("record.read.merge");
    let pl = ctx.block_label(prefix);
    let hl = ctx.block_label(hit);
    let rl = ctx.block_label(rest);
    let fl = ctx.block_label(fast);
    let ll = ctx.block_label(load);
    let hole_label = ctx.block_label(hole_check);
    let absent_label = ctx.block_label(absent);
    let cl = ctx.block_label(cold);
    let ml = ctx.block_label(merge);
    let bits = ctx.block().bitcast_double_to_i64(array);
    let handle = ctx.block().and(I64, &bits, crate::nanbox::POINTER_MASK_I64);
    let word = emit_array_guard_word(ctx.block(), &handle);
    // Record entry proves an Array. Still reject forwarding before reading
    // the live header: the body can allocate, grow, or mutate the receiver.
    let typed = emit_array_guard_word_typed(ctx.block(), &word);
    ctx.block().cond_br(&typed, &pl, &cl);
    ctx.current_block = prefix;
    let i = ctx.block().fptoui(DOUBLE, index, I32);
    let own_prefix = ctx.block().array_ek_prefix(&handle);
    let own = ctx.block().icmp_ult(I32, &i, &own_prefix);
    ctx.block().cond_br(&own, &hl, &rl);
    ctx.current_block = hit;
    // P proves a descriptor-free, non-hole own element below both length and
    // capacity. Prototype writes cannot intercept this Get. Use the existing
    // header proof, reloaded at every step, just like ordinary indexed reads.
    let own_value = lower_trusted_array_slot(ctx, &handle, &i);
    let hp = ctx.block().label.clone();
    ctx.block().br(&ml);
    ctx.current_block = rest;
    let safe = emit_and_no_named_layout(ctx.block(), "true", &handle);
    ctx.block().cond_br(&safe, &fl, &cl);
    ctx.current_block = fast;
    let length_ptr = ctx.block().inttoptr(I64, &handle);
    let length = ctx.block().load(I32, &length_ptr);
    let logical = ctx.block().icmp_ult(I32, &i, &length);
    // A pre-sized array can have logical length above its backing capacity.
    let capacity_ptr = ctx.block().gep(I8, &length_ptr, &[(I64, "4")]);
    let capacity = ctx.block().load(I32, &capacity_ptr);
    let backed = ctx.block().icmp_ult(I32, &i, &capacity);
    let in_bounds = ctx.block().and(I1, &logical, &backed);
    ctx.block().cond_br(&in_bounds, &ll, &cl);
    ctx.current_block = load;
    // Test the raw slot, before hole canonicalization: holes must consult
    // current prototypes, including getters installed by the previous step.
    let value = lower_trusted_array_slot(ctx, &handle, &i);
    let bits = ctx.block().bitcast_double_to_i64(&value);
    let hole = ctx.block().icmp_eq(I64, &bits, crate::nanbox::TAG_HOLE_I64);
    let fp = ctx.block().label.clone();
    ctx.block().cond_br(&hole, &hole_label, &ml);
    ctx.current_block = hole_check;
    // Preserve the old inline undefined result for a pristine prototype
    // chain. Only absence depends on this protector; own slots never do.
    let reserved = ctx.block().lshr(I32, &word, "16");
    let reserved = ctx.block().trunc(I32, &reserved, crate::types::I16);
    let clear = crate::expr::array_proto_guard::emit_array_default_prototype_chain(
        ctx.block(),
        &reserved,
    );
    ctx.block().cond_br(&clear, &absent_label, &cl);
    ctx.current_block = absent;
    let undefined = crate::nanbox::double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED));
    let ap = ctx.block().label.clone();
    ctx.block().br(&ml);
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
    Ok(ctx.block().phi(
        DOUBLE,
        &[
            (&own_value, &hp),
            (&value, &fp),
            (&undefined, &ap),
            (&value_cold, &cp),
        ],
    ))
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
    let unloadable = ctx.new_block("record.next.unloadable");
    let load = ctx.new_block("record.next.load");
    let exhausted = ctx.new_block("record.next.exhausted");
    let cold = ctx.new_block("record.next.cold");
    let merge = ctx.new_block("record.next.merge");
    let gl = ctx.block_label(guard);
    let bl = ctx.block_label(bounds);
    let ul = ctx.block_label(unloadable);
    let ll = ctx.block_label(load);
    let el = ctx.block_label(exhausted);
    let cl = ctx.block_label(cold);
    let ml = ctx.block_label(merge);
    ctx.block().cond_br(protocol, &cl, &gl);
    ctx.current_block = guard;
    let bits = ctx.block().bitcast_double_to_i64(array);
    let handle = ctx.block().and(I64, &bits, crate::nanbox::POINTER_MASK_I64);
    let word = emit_array_guard_word(ctx.block(), &handle);
    let safe = emit_array_guard_word_ok(ctx.block(), &word, &handle);
    // A hole can be canonicalized inline only on a pristine default chain.
    // A custom receiver prototype is independent of the global protector.
    let reserved = ctx.block().lshr(I32, &word, "16");
    let reserved = ctx.block().trunc(I32, &reserved, crate::types::I16);
    let clear = crate::expr::array_proto_guard::emit_array_default_prototype_chain(
        ctx.block(),
        &reserved,
    );
    let safe = ctx.block().and(I1, &safe, &clear);
    ctx.block().cond_br(&safe, &bl, &cl);
    ctx.current_block = bounds;
    let i = ctx.block().fptoui(DOUBLE, index, I32);
    let ptr = ctx.block().inttoptr(I64, &handle);
    let len = ctx.block().load(I32, &ptr);
    let ready = ctx.block().icmp_ult(I32, &i, &len);
    // Below the logical length but at or past the backing store (a pre-sized
    // `new Array(n)` above the dense limit): not exhausted, and not loadable
    // raw. The cold dispatcher steps it as a hole.
    let capacity_ptr = ctx.block().gep(I8, &ptr, &[(I64, "4")]);
    let capacity = ctx.block().load(I32, &capacity_ptr);
    let backed = ctx.block().icmp_ult(I32, &i, &capacity);
    let loadable = ctx.block().and(I1, &ready, &backed);
    ctx.block().cond_br(&loadable, &ll, &ul);
    ctx.current_block = unloadable;
    ctx.block().cond_br(&ready, &cl, &el);
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
            byte_offset: 0,
            byte_end: 0,
            object: Box::new(Expr::LocalGet(1)),
            index: Box::new(Expr::Integer(1)),
        }))]);
        assert!(ll.contains("33023"), "type and forwarding word");
        assert!(
            !ll.contains("icmp ne i64"),
            "named properties do not guard elements"
        );
        assert!(
            !ll.contains("call i32 @perry_array_element_kind"),
            "no descriptor writer means no element-kind query"
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
            byte_end: 0,
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
            byte_offset: 0,
            byte_end: 0,
            object: Box::new(Expr::LocalGet(1)),
            index: Box::new(Expr::LocalGet(2)),
        };
        let ll = ir(vec![Stmt::Expr(read()), Stmt::Return(Some(read()))]);
        assert_eq!(
            ll.lines()
                .filter(|l| l.starts_with("define ") && l.contains("@perry_array_read_cold_"))
                .count(),
            0
        );
        assert!(ll.contains("declare double @perry_array_read_cold_shared"));
        assert!(!ll.contains("perry_array_key_cold_"));
        assert!(
            !ll.contains("call i32 @js_typed_feedback_numeric_array_index_get_guard"),
            "plain-key misses must not retain the numeric promotion runtime"
        );
    }
}
