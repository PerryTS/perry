//! An inline, guarded, strictly-in-bounds element store for an array receiver
//! that is **not** a plain stack local.
//!
//! `lower_index_set_fast` (`expr/index.rs`) gives `a[i] = v` a full guarded
//! diamond, but only when `a` is a `LocalGet` with a slot — it needs the slot
//! to write a realloc'd head back to. Every other receiver shape —
//! `this.vals[i] = v`, `obj.arr[i] = v`, a closure-captured array — fell
//! straight through to a five-argument
//! `js_typed_feedback_array_set_f64_extend` call, **with no inline arm at
//! all**, while the matching READ (`index_get/guarded_array.rs`) has had a
//! complete inline diamond for both tiers all along.
//!
//! `gc-handoff/apps/pipeline.ts` is the shape that costs: `Registry.set`'s
//! `this.vals[i] = v` runs 1.44 M times, always onto an existing index.
//!
//! # What the guard proves, and why the fast arm is then sound
//!
//! | tested here | why |
//! |---|---|
//! | the box minus `POINTER_TAG << 48 \| 1 MiB` is below `2^48 - 1 MiB` | a heap pointer above the runtime-id band (the element read's band test) |
//! | `([h-8] as i32) & 0x0407_80FF == GC_TYPE_ARRAY` | ONE header word: an ordinary array, not a growth stub, no element descriptors, not frozen / sealed / non-extensible (DESIGN arrayread §3) |
//! | `index <u capacity`, then the slot is not `TAG_HOLE` | the slot holds an own data property. `[length, capacity)` holds holes (`array_truncate_length`), so this is also `index < length`: the store cannot add an element, change `length`, or reach a prototype setter, which is why the prototype facts are not tested here |
//!
//! A forwarded stub fails the word; the cold arm follows one edge (no binding
//! to heal for `this.vals[i]`) and requires the destination's word. The
//! element kind is settled before the value is written
//! ([`emit_array_store_kind_value`]).
//!
//! The slot write itself reuses the store emitters of `lower_index_set_fast`'s
//! in-bounds arm, and both paths settle the element kind through the same
//! [`emit_array_store_kind_value`], so the string addref, the GC layout note,
//! the write barrier and the F64 -> Any transition are one implementation.

use anyhow::Result;

use crate::types::{DOUBLE, I1, I16, I32, I64, I8};

use super::index_get::guarded_array::{
    emit_array_guard_word, ARRAY_STORE_GUARD_EXPECT_I32, ARRAY_STORE_GUARD_MASK_I32,
    HEAP_POINTER_BAND_BASE_I64, HEAP_POINTER_BAND_SPAN_I64,
};
use super::write_barrier::{
    emit_jsvalue_slot_store_deferred_layout_note_without_addref_on_block,
    emit_layout_note_slot_aware_on_block, emit_layout_pointer_bearing_check,
};
use super::{
    emit_array_numeric_write_note_on_block, emit_jsvalue_slot_store_scalar_aware_on_block,
    emit_write_barrier_slot_value_and_generation_tested, FnCtx,
};

/// The canonical element index a DOUBLE key names, as an `i32`, or `-1` when
/// it names none (fractional, negative, non-finite, above `i32::MAX`, or any
/// NaN-boxed non-number such as a string or Symbol key). `-1` is exactly the
/// value [`emit_guarded_inbounds_array_store`]'s guard declines, so a caller
/// can hand every key to it and keep ONE slow arm for the rejected ones.
///
/// Both numeric encodings are recognised: a plain double and an
/// `INT32_TAG`-boxed integer (a loop counter that was boxed on the way in).
/// The conversion never feeds `fptosi` an out-of-range or NaN input, which
/// would be poison.
pub(super) fn emit_canonical_element_index_i32(ctx: &mut FnCtx<'_>, idx_double: &str) -> String {
    let blk = ctx.block();
    let raw_ge_zero = blk.fcmp("oge", idx_double, "0.0");
    let raw_le_i32_max = blk.fcmp("ole", idx_double, "2147483647.0");
    let raw_in_range = blk.and(I1, &raw_ge_zero, &raw_le_i32_max);
    // `fptosi` is poison for NaN/out-of-range input: convert the
    // range-sanitized value.
    let safe_raw = blk.select(I1, &raw_in_range, DOUBLE, idx_double, "0.0");
    let raw_i32 = blk.fptosi(DOUBLE, &safe_raw, I32);
    let raw_round_trip = blk.sitofp(I32, &raw_i32, DOUBLE);
    let raw_is_integral = blk.fcmp("oeq", &raw_round_trip, idx_double);
    let raw_is_canonical = blk.and(I1, &raw_in_range, &raw_is_integral);
    let bits = blk.bitcast_double_to_i64(idx_double);
    let top16 = blk.lshr(I64, &bits, "48");
    let is_boxed_i32 = blk.icmp_eq(I64, &top16, crate::nanbox::INT32_TAG_TOP16_I64);
    let boxed_i32 = blk.trunc(I64, &bits, I32);
    let boxed_nonnegative = blk.icmp_sge(I32, &boxed_i32, "0");
    let boxed_is_canonical = blk.and(I1, &is_boxed_i32, &boxed_nonnegative);
    let canonical = blk.or(I1, &raw_is_canonical, &boxed_is_canonical);
    let idx_i32 = blk.select(I1, &is_boxed_i32, I32, &boxed_i32, &raw_i32);
    blk.select(I1, &canonical, I32, &idx_i32, "-1")
}

/// The raw-f64 element-kind bits in `_reserved`: `GC_ARRAY_RAW_F64_LAYOUT`
/// (0x80, dense) | `GC_ARRAY_RAW_F64_HOLES` (0x1000). Either one makes every
/// non-hole slot a Number stored as its canonical double.
const ARRAY_F64_KIND_BITS_I16: &str = "4224"; // 0x1080
/// `0x7FF8_0000_0000_0000`, the one NaN an F64 slot may hold.
const CANONICAL_NAN_I64: &str = "9221120237041090560";

/// The array's `_reserved` half-word (its own flags), loaded from `handle`.
pub(super) fn emit_array_reserved(blk: &mut crate::block::LlBlock, handle: &str) -> String {
    let reserved_addr = blk.sub(I64, handle, "6");
    let reserved_ptr = blk.inttoptr(I64, &reserved_addr);
    blk.load(I16, &reserved_ptr)
}

/// The value an element store into the live array `arr_handle` must write,
/// with the array's element kind settled first (DESIGN arrayread §3):
///
/// * kind Any (both F64 bits clear): `val_double` unchanged. This is the hot
///   test, `reserved & 0x1080` and one branch.
/// * kind F64, `val_double` a plain double: the double with any NaN collapsed
///   to the canonical one (a select, no call).
/// * kind F64, anything NaN-boxed: the cold arm. `js_array_note_numeric_write`
///   runs BEFORE the store, so a non-Number clears the F64 bits before its
///   bits land in a slot a raw-f64 reader trusts; a Number (an `INT32` box)
///   keeps the kind and is written as its double.
///
/// Emits into the current block and leaves a fresh join block current.
pub(super) fn emit_array_store_kind_value(
    ctx: &mut FnCtx<'_>,
    arr_handle: &str,
    val_double: &str,
    block_prefix: &str,
) -> String {
    let f64_idx = ctx.new_block(&format!("{}.kind.f64", block_prefix));
    let canon_idx = ctx.new_block(&format!("{}.kind.canon", block_prefix));
    let cold_idx = ctx.new_block(&format!("{}.kind.cold", block_prefix));
    let done_idx = ctx.new_block(&format!("{}.kind.done", block_prefix));
    let f64_label = ctx.block_label(f64_idx);
    let canon_label = ctx.block_label(canon_idx);
    let cold_label = ctx.block_label(cold_idx);
    let done_label = ctx.block_label(done_idx);
    let entry_end = {
        let blk = ctx.block();
        let reserved = emit_array_reserved(blk, arr_handle);
        let kind_bits = blk.and(I16, &reserved, ARRAY_F64_KIND_BITS_I16);
        let is_f64 = blk.icmp_ne(I16, &kind_bits, "0");
        blk.cond_br(&is_f64, &f64_label, &done_label);
        blk.label.clone()
    };
    ctx.current_block = f64_idx;
    {
        let blk = ctx.block();
        let bits = blk.bitcast_double_to_i64(val_double);
        let top16 = blk.lshr(I64, &bits, "48");
        // NaN-boxed iff the top 16 bits are in 0x7FF9..=0x7FFF.
        let tag_offset = blk.sub(I64, &top16, "32761"); // 0x7FF9
        let is_boxed = blk.icmp_ult(I64, &tag_offset, "7");
        blk.cond_br(&is_boxed, &cold_label, &canon_label);
    }
    ctx.current_block = canon_idx;
    let canonical = {
        let blk = ctx.block();
        let is_nan = blk.fcmp("uno", val_double, val_double);
        let nan = blk.bitcast_i64_to_double(CANONICAL_NAN_I64);
        let v = blk.select(I1, &is_nan, DOUBLE, &nan, val_double);
        blk.br(&done_label);
        v
    };
    ctx.current_block = cold_idx;
    super::store_census::bump(ctx, super::store_census::ELEM_STORE_F64_COLD);
    let cold_value = {
        let blk = ctx.block();
        let bits = blk.bitcast_double_to_i64(val_double);
        // The header write comes first: this clears both F64 bits unless the
        // value is a Number.
        emit_array_numeric_write_note_on_block(blk, arr_handle, &bits);
        let reserved = emit_array_reserved(blk, arr_handle);
        let kind_bits = blk.and(I16, &reserved, ARRAY_F64_KIND_BITS_I16);
        let still_f64 = blk.icmp_ne(I16, &kind_bits, "0");
        let as_double = blk.call(
            DOUBLE,
            "js_array_numeric_value_to_raw_f64",
            &[(DOUBLE, val_double)],
        );
        let v = blk.select(I1, &still_f64, DOUBLE, &as_double, val_double);
        blk.br(&done_label);
        v
    };
    ctx.current_block = done_idx;
    let canon_end = ctx.block_label(canon_idx);
    let cold_end = ctx.block_label(cold_idx);
    ctx.block().phi(
        DOUBLE,
        &[
            (val_double, &entry_end),
            (&canonical, &canon_end),
            (&cold_value, &cold_end),
        ],
    )
}

/// Emit the guarded diamond. `fallback` emits the original slow arm (the
/// runtime call plus whatever bookkeeping it owns) into the block that is
/// current when it runs.
///
/// `idx_i32` must already be materialized in the *entry* block — it is used by
/// the guard and by the fast arm, and both dominate.
#[allow(clippy::too_many_arguments)]
pub(super) fn emit_guarded_inbounds_array_store(
    ctx: &mut FnCtx<'_>,
    arr_box: &str,
    idx_i32: &str,
    val_double: &str,
    block_prefix: &str,
    layout_note_needed: bool,
    write_barrier_needed: bool,
    value_is_numeric: bool,
    fallback: impl FnOnce(&mut FnCtx<'_>) -> Result<()>,
) -> Result<()> {
    emit_guarded_inbounds_array_store_keyed(
        ctx,
        arr_box,
        StoreIndex::I32(idx_i32),
        val_double,
        block_prefix,
        layout_note_needed,
        write_barrier_needed,
        value_is_numeric,
        fallback,
    )
}

/// The key of a guarded store: an already-materialized `i32`, or a DOUBLE key
/// whose canonical element index ([`emit_canonical_element_index_i32`]) is
/// computed only once the receiver has branded as an Array, so a declining
/// receiver (the untyped route's typed arrays and Buffers) never pays for it.
#[derive(Clone, Copy)]
pub(super) enum StoreIndex<'a> {
    I32(&'a str),
    CanonicalOfDouble(&'a str),
}

/// [`emit_guarded_inbounds_array_store`] with a [`StoreIndex`] key.
#[allow(clippy::too_many_arguments)]
pub(super) fn emit_guarded_inbounds_array_store_keyed(
    ctx: &mut FnCtx<'_>,
    arr_box: &str,
    index: StoreIndex<'_>,
    val_double: &str,
    block_prefix: &str,
    layout_note_needed: bool,
    write_barrier_needed: bool,
    // Unused since the kind is settled before the store for every value
    // (`emit_array_store_kind_value`); kept for the callers' signature.
    _value_is_numeric: bool,
    fallback: impl FnOnce(&mut FnCtx<'_>) -> Result<()>,
) -> Result<()> {
    // An operand may have emitted a throw + unreachable. LlBlock drops
    // instructions after a terminator; opening the store diamond would then
    // reference those dropped values from fresh blocks (#11450). Match the
    // local-array fast path and leave the terminated path untouched.
    if ctx.block().is_terminated() {
        return Ok(());
    }

    let deref_idx = ctx.new_block(&format!("{}.deref", block_prefix));
    let fast_idx = ctx.new_block(&format!("{}.fast", block_prefix));
    let slow_idx = ctx.new_block(&format!("{}.slow", block_prefix));
    let merge_idx = ctx.new_block(&format!("{}.merge", block_prefix));
    let deref_label = ctx.block_label(deref_idx);
    let fast_label = ctx.block_label(fast_idx);
    let slow_label = ctx.block_label(slow_idx);
    let merge_label = ctx.block_label(merge_idx);

    let probe_idx = ctx.new_block(&format!("{}.probe", block_prefix));
    let probe_label = ctx.block_label(probe_idx);
    // ONE header word is the structural guard (DESIGN arrayread §3): the
    // read's mask plus the integrity bits. The receiver is a heap pointer iff
    // its box, less `POINTER_TAG << 48 | 1 MiB`, lands in the band below
    // 2^48 - 1 MiB; the handle is then that offset plus 1 MiB.
    let band_offset = {
        let blk = ctx.block();
        let arr_bits = blk.bitcast_double_to_i64(arr_box);
        let band_offset = blk.sub(I64, &arr_bits, HEAP_POINTER_BAND_BASE_I64);
        let in_band = blk.icmp_ult(I64, &band_offset, HEAP_POINTER_BAND_SPAN_I64);
        blk.cond_br(&in_band, &deref_label, &slow_label);
        band_offset
    };

    ctx.current_block = deref_idx;
    let follow_idx = ctx.new_block(&format!("{}.deref.follow", block_prefix));
    let follow_label = ctx.block_label(follow_idx);
    let follow_check_idx = ctx.new_block(&format!("{}.deref.follow.check", block_prefix));
    let follow_check_label = ctx.block_label(follow_check_idx);
    let live_deref_idx = ctx.new_block(&format!("{}.deref.live", block_prefix));
    let live_deref_label = ctx.block_label(live_deref_idx);
    let (arr_handle, deref_end) = {
        let blk = ctx.block();
        let arr_handle = blk.add(I64, &band_offset, "1048576");
        let word = emit_array_guard_word(blk, &arr_handle);
        let masked = blk.and(I32, &word, ARRAY_STORE_GUARD_MASK_I32);
        let word_ok = blk.icmp_eq(I32, &masked, ARRAY_STORE_GUARD_EXPECT_I32);
        blk.cond_br(&word_ok, &live_deref_label, &follow_label);
        (arr_handle, blk.label.clone())
    };

    // Cold: a growth/evacuation stub. `this.vals[i] = v` has no binding to
    // heal, so the stub is followed one edge here, off the hot path, and the
    // destination must pass the same word. A receiver that is not an Array
    // (the untyped route's typed arrays and Buffers) leaves on its type byte.
    ctx.current_block = follow_idx;
    let forwarding_target = {
        let blk = ctx.block();
        let gc_type_ptr = {
            let addr = blk.sub(I64, &arr_handle, "8");
            blk.inttoptr(I64, &addr)
        };
        let gc_type = blk.load(I8, &gc_type_ptr);
        let is_array = blk.icmp_eq(I8, &gc_type, "1"); // GC_TYPE_ARRAY
        let gc_flags_ptr = {
            let addr = blk.sub(I64, &arr_handle, "7");
            blk.inttoptr(I64, &addr)
        };
        let gc_flags = blk.load(I8, &gc_flags_ptr);
        let forwarded_bits = blk.and(I8, &gc_flags, "128");
        let is_forwarded = blk.icmp_ne(I8, &forwarded_bits, "0");
        let is_stub = blk.and(I1, &is_array, &is_forwarded);
        let target_idx = ctx.new_block(&format!("{}.deref.follow.target", block_prefix));
        let target_label = ctx.block_label(target_idx);
        ctx.block().cond_br(&is_stub, &target_label, &slow_label);
        ctx.current_block = target_idx;
        let blk = ctx.block();
        let original_arr_ptr = blk.inttoptr(I64, &arr_handle);
        let target = blk.load(I64, &original_arr_ptr);
        // A forwarding word is not trusted until its address is in the heap
        // band: never read the destination header speculatively.
        let target_top = blk.lshr(I64, &target, "48");
        let target_top_clear = blk.icmp_eq(I64, &target_top, "0");
        let target_above_band = blk.icmp_ugt(I64, &target, "1048575");
        let target_ok = blk.and(I1, &target_top_clear, &target_above_band);
        blk.cond_br(&target_ok, &follow_check_label, &slow_label);
        target
    };
    ctx.current_block = follow_check_idx;
    {
        let blk = ctx.block();
        let word = emit_array_guard_word(blk, &forwarding_target);
        let masked = blk.and(I32, &word, ARRAY_STORE_GUARD_MASK_I32);
        let word_ok = blk.icmp_eq(I32, &masked, ARRAY_STORE_GUARD_EXPECT_I32);
        blk.cond_br(&word_ok, &live_deref_label, &slow_label);
    }

    ctx.current_block = live_deref_idx;
    let live_handle = {
        let follow_end = ctx.block_label(follow_check_idx);
        ctx.block().phi(
            I64,
            &[(&arr_handle, &deref_end), (&forwarding_target, &follow_end)],
        )
    };
    let idx_i32 = match index {
        StoreIndex::I32(idx) => idx.to_string(),
        StoreIndex::CanonicalOfDouble(idx_double) => {
            emit_canonical_element_index_i32(ctx, idx_double)
        }
    };
    let idx_i32 = idx_i32.as_str();
    {
        // `idx <u capacity` (a negative index wraps high). With the
        // `[length, capacity)` hole invariant (`array_truncate_length`), a
        // non-hole slot below capacity is below `length`, so the probe below
        // is the whole bounds test.
        let blk = ctx.block();
        let arr_ptr = blk.inttoptr(I64, &live_handle);
        let capacity_ptr = blk.gep(I8, &arr_ptr, &[(I64, "4")]);
        let capacity = blk.load(I32, &capacity_ptr);
        let within_capacity = blk.icmp_ult(I32, idx_i32, &capacity);
        blk.cond_br(&within_capacity, &probe_label, &slow_label);
    }

    // The slot must hold a value: an own data property, so the store is a
    // plain overwrite that never consults the prototype chain. A hole (or a
    // slot past `length`) is an add, which may reach an inherited setter and
    // may change `length`: the slow arm.
    ctx.current_block = probe_idx;
    let element_addr = {
        let blk = ctx.block();
        let idx_i64 = blk.zext(I32, idx_i32, I64);
        let byte_offset = blk.shl(I64, &idx_i64, "3");
        let elements_addr = blk.array_elements_addr(&live_handle);
        let element_addr = blk.add(I64, &elements_addr, &byte_offset);
        let element_ptr = blk.inttoptr(I64, &element_addr);
        let old = blk.load(I64, &element_ptr);
        let is_hole = blk.icmp_eq(I64, &old, crate::nanbox::TAG_HOLE_I64);
        blk.cond_br(&is_hole, &slow_label, &fast_label);
        element_addr
    };

    ctx.current_block = fast_idx;
    super::store_census::bump(ctx, super::store_census::ELEM_STORE_INBOUNDS);
    // #7715 B3: the barrier is emitted separately, behind an inline live test
    // of the stored VALUE and then of the parent array's generation, so the
    // store emitter is told not to emit it. Everything else — the slot write,
    // the string addref, the layout note, and their ordering — is unchanged,
    // and the barrier still lands between the layout note and the
    // numeric-write note, exactly where it did.
    //
    // The layout note deliberately stays OUTSIDE the value test even though
    // the class-field emitter puts its own note inside one: `layout_note_slot`
    // funnels `crate::array::note_element_store` (#7480), and a non-pointer
    // stored over a pointer is exactly the store that must clear
    // `GC_ARRAY_ELEMENT_SHAPE`. Class fields have no such per-slot array
    // invariant, which is why that half of #7511's argument does not transfer.
    // The kind is settled BEFORE the value is written: an F64 array takes a
    // Number as its canonical double, and anything else clears the F64 bits
    // first (`emit_array_store_kind_value`).
    let stored_value = emit_array_store_kind_value(ctx, &live_handle, val_double, block_prefix);
    let val_double = stored_value.as_str();
    let (arr_handle, element_addr, value_bits, layout_note) = {
        let reserved = emit_array_reserved(ctx.block(), &live_handle);
        let blk = ctx.block();
        // The live (possibly forwarded-once) head proved by `deref.live`.
        let arr_handle = live_handle.clone();
        let element_ptr = blk.inttoptr(I64, &element_addr);
        // Same call, same argument order, as `lower_index_set_fast`'s
        // in-bounds arm: the guard proved the slot holds a valid value, so the
        // scalar-aware note can skip the layout hashmap on a
        // scalar-over-scalar store (#5094).
        if !layout_note_needed {
            let value_bits = emit_jsvalue_slot_store_scalar_aware_on_block(
                blk,
                &element_ptr,
                val_double,
                &arr_handle,
                idx_i32,
                false,
                &arr_handle,
                &element_addr,
                false,
            )
            .unwrap_or_else(|| blk.bitcast_double_to_i64(val_double));
            (arr_handle, element_addr, value_bits, None)
        } else {
            // The scalar-aware note itself, opened up: the runtime
            // (`layout_note_slot_aware`) returns without acting when the old
            // and new values share a pointer classification — unless both are
            // pointers AND the array carries an element-shape proof
            // (`GC_ARRAY_ELEMENT_SHAPE` in the `_reserved` word `deref.live`
            // already loaded), which the pointer-over-pointer arm maintains. A
            // classification change must always reach `layout_note_slot`.
            // Decide that inline with the exact runtime predicate and call the
            // note only when it has work: the ECS `ents[id] = arch` store is a
            // pointer over a pointer into a proof-free array on every iteration.
            let (value_bits, old_bits) =
                emit_jsvalue_slot_store_deferred_layout_note_without_addref_on_block(
                    blk,
                    &element_ptr,
                    val_double,
                );
            // `write_barrier_needed == false` is the caller's proof that the
            // value carries non-pointer bits by construction
            // (`array_store_needs_write_barrier`), so its classification is a
            // constant and only the RETIRED value's needs testing.
            let new_is_pointer = if write_barrier_needed {
                emit_layout_pointer_bearing_check(blk, &value_bits)
            } else {
                "false".to_string()
            };
            let old_is_pointer = emit_layout_pointer_bearing_check(blk, &old_bits);
            let classification_changed = blk.icmp_ne(I1, &new_is_pointer, &old_is_pointer);
            let shape_bits = blk.and(I16, &reserved, "2048"); // GC_ARRAY_ELEMENT_SHAPE
            let has_element_shape = blk.icmp_ne(I16, &shape_bits, "0");
            let pointer_over_pointer_noted = blk.and(I1, &new_is_pointer, &has_element_shape);
            let note_needed = blk.or(I1, &classification_changed, &pointer_over_pointer_noted);
            (
                arr_handle,
                element_addr,
                value_bits,
                Some((old_bits, note_needed)),
            )
        }
    };
    if layout_note.is_some() && write_barrier_needed {
        // The string demote the deferred store leaves to its caller, with the
        // helper's own `STRING_TAG` test hoisted inline. A value with
        // non-pointer bits by construction (no barrier needed) cannot be a
        // heap string, so it needs neither the test nor the call.
        super::helpers::emit_string_addref_if_heap_string(ctx, val_double);
    }
    if let Some((old_bits, note_needed)) = layout_note {
        let note_idx = ctx.new_block(&format!("{}.laynote", block_prefix));
        let note_done_idx = ctx.new_block(&format!("{}.laynote.done", block_prefix));
        let note_label = ctx.block_label(note_idx);
        let note_done_label = ctx.block_label(note_done_idx);
        ctx.block()
            .cond_br(&note_needed, &note_label, &note_done_label);
        ctx.current_block = note_idx;
        emit_layout_note_slot_aware_on_block(
            ctx.block(),
            &arr_handle,
            idx_i32,
            &value_bits,
            &old_bits,
        );
        ctx.block().br(&note_done_label);
        ctx.current_block = note_done_idx;
    }
    if write_barrier_needed {
        // `arr_handle` is the live head `deref.live` just proved through its
        // own `obj_type == GC_TYPE_ARRAY` / `!GC_FLAG_FORWARDED` header reads,
        // so it is a live, non-forwarded GC array user pointer — the
        // precondition for reading its header byte. (LLVM CSEs that byte load with the
        // guard's, so the gate costs the test and the branch, not a reload.)
        emit_write_barrier_slot_value_and_generation_tested(
            ctx,
            &arr_handle,
            &arr_handle,
            &element_addr,
            &value_bits,
            block_prefix,
        );
    }
    ctx.block().br(&merge_label);

    ctx.current_block = slow_idx;
    super::store_census::bump(ctx, super::store_census::ELEM_STORE_GUARD_MISS);
    super::store_census::bump(ctx, super::store_census::ELEM_STORE_FALLBACK);
    fallback(ctx)?;
    ctx.block().br(&merge_label);

    ctx.current_block = merge_idx;
    Ok(())
}

#[cfg(test)]
mod tests {
    use perry_hir::{types::Type, Expr, Stmt};

    fn store_ir(value: Expr, ty: Type) -> String {
        let mut module = perry_hir::Module::new("terminated_guarded_store");
        module.init = vec![Stmt::Let {
            id: 1,
            name: "items".into(),
            ty,
            mutable: true,
            init: Some(Expr::Array(vec![])),
        }];
        module.functions.push(perry_hir::Function {
            id: 2,
            name: "store".into(),
            type_params: vec![],
            params: vec![],
            return_type: Type::Any,
            body: vec![Stmt::Expr(Expr::IndexSet {
                object: Box::new(Expr::LocalGet(1)),
                index: Box::new(Expr::Integer(0)),
                value: Box::new(value),
            })],
            is_async: false,
            is_generator: false,
            is_strict: true,
            is_exported: false,
            captures: vec![],
            decorators: vec![],
            was_plain_async: false,
            was_unrolled: false,
        });
        String::from_utf8(
            crate::compile_module(&module, crate::temp_root_coverage::entry_opts())
                .expect("store module compiles"),
        )
        .unwrap()
    }

    // Since the unresolved Worker continues in a predecessor-less block
    // (#11450, dyn_extern_i18n.rs), the store after it is emitted as dead
    // code rather than skipped; the `is_terminated` guards above stay as the
    // defense for any other operand that ends its block. Either way the
    // emitted module must parse and verify.
    #[test]
    fn throwing_operand_store_emits_valid_ir() {
        for (ty, block) in [
            (Type::Array(Box::new(Type::Any)), "idxset.recv_global.deref"),
            (Type::Any, "tav.set.fast"),
        ] {
            let live = store_ir(Expr::Number(42.0), ty.clone());
            assert!(live.contains(block), "store arm {block} not exercised");
            let dead = store_ir(
                Expr::WorkerNew {
                    paths: vec![],
                    filename: Box::new(Expr::String("unresolved-worker".into())),
                    options: None,
                    is_eval: false,
                    partial: false,
                },
                ty,
            );
            assert!(dead.contains("call void @js_throw_error_with_code("));
            let llvm = inkwell::context::Context::create();
            let parsed = crate::inprocess::parse_ir_text(&llvm, &dead, block)
                .unwrap_or_else(|e| panic!("{block}: {e:#}\n{dead}"));
            parsed
                .verify()
                .unwrap_or_else(|e| panic!("{block}: LLVM verifier: {}\n{dead}", e.to_string()));
        }
    }
}
