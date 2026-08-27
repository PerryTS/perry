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
//! Exactly the conjunction `js_typed_feedback_plain_array_index_set_guard`
//! evaluates, plus a strictly stronger bounds test:
//!
//! | tested here | why |
//! |---|---|
//! | `POINTER_TAG`, handle above the small-handle band, below `is_valid_obj_ptr`'s 2^47 ceiling | `gc_header_for_user_addr` applies all three before the helper dereferences anything (#7396) |
//! | `obj_type == GC_TYPE_ARRAY`, `!GC_FLAG_FORWARDED` | a stale growth-forwarded head must follow its chain, which only the helper does |
//! | `_reserved & (FROZEN\|SEALED\|NO_EXTEND\|ARRAY_DESCRIPTORS) == 0` | a write may throw in strict mode or dispatch an accessor setter |
//! | `PERRY_ARRAY_INDEX_FAST_PATH_INVALIDATED == 0` | a polluted `Array.prototype` can intercept the store |
//! | `0 <= index < length` | **strictly** in bounds — the helper is the *extend* variant, so every growth, every hole-creating sparse write, and every `length` bump stays on the slow arm |
//! | `length <= capacity`, both `<= 16e6` | header sanity, same constants as the read tier |
//!
//! In bounds and non-exotic, the store is a plain slot overwrite that cannot
//! change `length` — which is the entire reason this arm may skip the
//! realloc-capable helper without needing a writeback slot, and therefore the
//! reason it works for receivers `lower_index_set_fast` cannot serve.
//!
//! The slot write itself reuses `emit_jsvalue_slot_store_scalar_aware_on_block`
//! + `emit_array_numeric_write_note_on_block` **verbatim** from
//! `lower_index_set_fast`'s own in-bounds arm, so the string addref, the GC
//! layout note, the write barrier and the raw-f64 layout downgrade are the same
//! code on both paths rather than a second implementation of them.

use anyhow::Result;

use crate::nanbox::POINTER_MASK_I64;
use crate::types::{I1, I16, I32, I64, I8};

use super::write_barrier::{
    emit_jsvalue_slot_store_deferred_layout_note_on_block, emit_layout_note_slot_aware_on_block,
    emit_layout_pointer_bearing_check,
};
use super::{
    emit_array_numeric_write_note_on_block, emit_jsvalue_slot_store_scalar_aware_on_block,
    emit_write_barrier_slot_value_and_generation_tested, FnCtx,
};

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
    let deref_idx = ctx.new_block(&format!("{}.deref", block_prefix));
    let fast_idx = ctx.new_block(&format!("{}.fast", block_prefix));
    let slow_idx = ctx.new_block(&format!("{}.slow", block_prefix));
    let merge_idx = ctx.new_block(&format!("{}.merge", block_prefix));
    let deref_label = ctx.block_label(deref_idx);
    let fast_label = ctx.block_label(fast_idx);
    let slow_label = ctx.block_label(slow_idx);
    let merge_label = ctx.block_label(merge_idx);

    {
        let blk = ctx.block();
        let arr_bits = blk.bitcast_double_to_i64(arr_box);
        let arr_handle = blk.and(I64, &arr_bits, POINTER_MASK_I64);
        let tag = blk.lshr(I64, &arr_bits, "48");
        let is_pointer = blk.icmp_eq(I64, &tag, "32765"); // POINTER_TAG
        let above_handle_band = blk.icmp_ugt(I64, &arr_handle, "1048575");
        // `is_valid_obj_ptr`'s upper bound (2^47), which
        // `gc_header_for_user_addr` applies before the helper this fronts
        // dereferences anything. Without it a corrupted POINTER_TAG box with a
        // payload in [2^47, 2^48) would be dereferenced here but rejected
        // there, making the inline tier weaker than the call it replaces.
        let below_heap_limit = blk.icmp_ult(I64, &arr_handle, "140737488355328");
        let mut heap_candidate = blk.and(I1, &is_pointer, &above_handle_band);
        heap_candidate = blk.and(I1, &heap_candidate, &below_heap_limit);
        blk.cond_br(&heap_candidate, &deref_label, &slow_label);
    }

    ctx.current_block = deref_idx;
    let live_deref_idx = ctx.new_block(&format!("{}.deref.live", block_prefix));
    let live_deref_label = ctx.block_label(live_deref_idx);
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

        // Array growth (and GC evacuation) leave the live user address in the
        // first payload word of a forwarded array stub. Follow one edge inline
        // and re-brand/re-check the destination below, exactly as the guarded
        // read tier does. This matters most for receivers that are NOT stack
        // locals: `this.vals[i] = v` has no writeback slot, so once the array
        // has grown past its initial capacity the object field keeps the
        // pre-grow stub forever. Before this the stub failed `not_forwarded`
        // on EVERY later store and the whole store went out of line through
        // the extend helper and the allocator/registry resolver (the ECS
        // add/remove hot path: `this._ent[id] = arch`, `this.sparse[x] = n`).
        // Longer or corrupt chains still take the slow arm.
        let original_arr_ptr = blk.inttoptr(I64, &arr_handle);
        let forwarding_target = blk.load(I64, &original_arr_ptr);
        let follow_forwarding = blk.and(I1, &is_array, &is_forwarded);
        let live_handle = blk.select(I1, &follow_forwarding, I64, &forwarding_target, &arr_handle);

        let live_top = blk.lshr(I64, &live_handle, "48");
        let live_top_clear = blk.icmp_eq(I64, &live_top, "0");
        let live_above_handle_band = blk.icmp_ugt(I64, &live_handle, "1048575");
        let live_below_heap_limit = blk.icmp_ult(I64, &live_handle, "140737488355328");
        let mut live_heap_candidate = blk.and(I1, &live_top_clear, &live_above_handle_band);
        live_heap_candidate = blk.and(I1, &live_heap_candidate, &live_below_heap_limit);
        // A forwarding word is not trusted until its address is in the heap
        // band: never read the destination header speculatively.
        blk.cond_br(&live_heap_candidate, &live_deref_label, &slow_label);
        live_handle
    };

    ctx.current_block = live_deref_idx;
    let reserved = {
        let blk = ctx.block();
        let arr_handle = live_handle.clone();

        let gc_type_addr = blk.sub(I64, &arr_handle, "8");
        let gc_type_ptr = blk.inttoptr(I64, &gc_type_addr);
        let gc_type = blk.load(I8, &gc_type_ptr);
        let is_array = blk.icmp_eq(I8, &gc_type, "1"); // GC_TYPE_ARRAY

        let gc_flags_addr = blk.sub(I64, &arr_handle, "7");
        let gc_flags_ptr = blk.inttoptr(I64, &gc_flags_addr);
        let gc_flags = blk.load(I8, &gc_flags_ptr);
        let forwarded_bits = blk.and(I8, &gc_flags, "128");
        let not_forwarded = blk.icmp_eq(I8, &forwarded_bits, "0");

        // FROZEN(0x1)|SEALED(0x2)|NO_EXTEND(0x4)|ARRAY_DESCRIPTORS(0x400).
        let reserved_addr = blk.sub(I64, &arr_handle, "6");
        let reserved_ptr = blk.inttoptr(I64, &reserved_addr);
        let reserved = blk.load(I16, &reserved_ptr);
        let integrity_bits = blk.and(I16, &reserved, "1031"); // 0x407
        let integrity_clean = blk.icmp_eq(I16, &integrity_bits, "0");

        let invalidated = blk.load_volatile(I8, "@PERRY_ARRAY_INDEX_FAST_PATH_INVALIDATED");
        let default_prototype_chain = blk.icmp_eq(I8, &invalidated, "0");

        let arr_ptr = blk.inttoptr(I64, &arr_handle);
        let length = blk.load(I32, &arr_ptr);
        let capacity_ptr = blk.gep(I8, &arr_ptr, &[(I64, "4")]);
        let capacity = blk.load(I32, &capacity_ptr);
        let index_negative = blk.icmp_slt(I32, idx_i32, "0");
        let index_nonnegative = blk.icmp_eq(I1, &index_negative, "false");
        // STRICTLY in bounds: `index == length` is an extend, which changes
        // `length` and may need a realloc, so it belongs on the slow arm.
        let index_in_bounds = blk.icmp_ult(I32, idx_i32, &length);
        let length_sane = blk.icmp_ule(I32, &length, "16000000");
        let capacity_sane = blk.icmp_ule(I32, &capacity, "16000000");
        let length_within_capacity = blk.icmp_ule(I32, &length, &capacity);

        let mut guard_ok = blk.and(I1, &is_array, &not_forwarded);
        guard_ok = blk.and(I1, &guard_ok, &integrity_clean);
        guard_ok = blk.and(I1, &guard_ok, &default_prototype_chain);
        guard_ok = blk.and(I1, &guard_ok, &index_nonnegative);
        guard_ok = blk.and(I1, &guard_ok, &index_in_bounds);
        guard_ok = blk.and(I1, &guard_ok, &length_sane);
        guard_ok = blk.and(I1, &guard_ok, &capacity_sane);
        guard_ok = blk.and(I1, &guard_ok, &length_within_capacity);
        blk.cond_br(&guard_ok, &fast_label, &slow_label);
        // The live head's `_reserved` word dominates the fast arm; the numeric
        // write note below is gated on it.
        reserved
    };

    ctx.current_block = fast_idx;
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
    let (arr_handle, element_addr, value_bits, layout_note) = {
        let blk = ctx.block();
        // The live (possibly forwarded-once) head proved by `deref.live`,
        // which is this block's only predecessor.
        let arr_handle = live_handle.clone();
        let idx_i64 = blk.zext(I32, idx_i32, I64);
        let byte_offset = blk.shl(I64, &idx_i64, "3");
        let with_header = blk.add(I64, &byte_offset, "8");
        let element_addr = blk.add(I64, &arr_handle, &with_header);
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
            let (value_bits, old_bits) = emit_jsvalue_slot_store_deferred_layout_note_on_block(
                blk,
                &element_ptr,
                val_double,
            );
            let new_is_pointer = emit_layout_pointer_bearing_check(blk, &value_bits);
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
    if !value_is_numeric {
        // A non-numeric store into a raw-f64-flagged array downgrades the
        // layout. The runtime note is exactly "clear GC_ARRAY_RAW_F64_LAYOUT |
        // GC_ARRAY_RAW_F64_HOLES if the value is not a Number", so an array
        // whose header already has both bits clear has nothing to note. Test
        // the `_reserved` word `deref.live` just loaded and skip the call —
        // which re-resolved the receiver through the allocator/registry
        // resolver on EVERY pointer store (the ECS archetype moves) — unless a
        // raw-f64 bit is actually set. Nothing between that load and here can
        // SET a raw-f64 bit: the slot store, string addref, layout note and
        // write barrier only ever clear them; the bits are set solely by the
        // explicit verify/rebuild paths.
        let note_idx = ctx.new_block(&format!("{}.numnote", block_prefix));
        let note_done_idx = ctx.new_block(&format!("{}.numnote.done", block_prefix));
        let note_label = ctx.block_label(note_idx);
        let note_done_label = ctx.block_label(note_done_idx);
        {
            let blk = ctx.block();
            // GC_ARRAY_RAW_F64_LAYOUT (0x80) | GC_ARRAY_RAW_F64_HOLES (0x1000).
            let raw_bits = blk.and(I16, &reserved, "4224");
            let has_raw_layout = blk.icmp_ne(I16, &raw_bits, "0");
            blk.cond_br(&has_raw_layout, &note_label, &note_done_label);
        }
        ctx.current_block = note_idx;
        {
            let blk = ctx.block();
            emit_array_numeric_write_note_on_block(blk, &arr_handle, &value_bits);
            blk.br(&note_done_label);
        }
        ctx.current_block = note_done_idx;
    }
    ctx.block().br(&merge_label);

    ctx.current_block = slow_idx;
    fallback(ctx)?;
    ctx.block().br(&merge_label);

    ctx.current_block = merge_idx;
    Ok(())
}
