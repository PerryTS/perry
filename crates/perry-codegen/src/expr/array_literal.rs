//! Array-literal lowering (extracted from `expr.rs`, issue #1098).
//! Pure move — no logic changes.

use anyhow::Result;
use perry_hir::Expr;

use super::{
    emit_layout_note_slot_on_block, emit_write_barrier_slot_on_block, lower_expr,
    nanbox_pointer_inline, FnCtx,
};
use crate::type_analysis::is_numeric_expr;
use crate::types::{DOUBLE, I32, I64, I8, PTR};

/// Lower an array literal `[a, b, c, …]`.
///
/// Fast path: element expressions are lowered first (any allocations
/// inside elements complete before we claim the arena bump slot for the
/// outer array), then for small literals (≤ 16 elements) we emit inline
/// bump-allocator IR — the same pattern `new ClassName()` uses when
/// `class_keys_globals` is populated. No extern call on the hot path:
/// a load of the per-function arena state, a bump-pointer check, one i64
/// store for the packed GcHeader, one i64 store for the packed ArrayHeader
/// (length and capacity share the same 8 bytes), and N `store double, ptr`
/// for the elements. The slow path (block overflow) calls
/// `js_inline_arena_slow_alloc`.
///
/// For N > 16 we fall back to the extern `js_array_alloc_literal` — the
/// inline path emits per-literal IR that's cheap at small N but grows with
/// each element store, so large literals benefit more from a compact call.
///
/// GC safety: the array header is written after the bump commits, so any
/// GC observing the partially-written arena block sees either a not-yet-
/// allocated slot (offset hasn't advanced past the `fits` check) or a
/// header with `length == capacity` and uninitialized elements. No
/// allocator call runs between the header write and the element stores,
/// so GC can't run in that window. Element expressions with their own
/// allocations lower to SSA values pinned by conservative stack scanning.
pub(crate) fn lower_array_literal(ctx: &mut FnCtx<'_>, elements: &[Expr]) -> Result<String> {
    let n = elements.len();

    // Empty literal: no elements to worry about, keep the simple path.
    if n == 0 {
        let arr = ctx.block().call(I64, "js_array_alloc", &[(I32, "0")]);
        return Ok(nanbox_pointer_inline(ctx.block(), &arr));
    }

    // Evaluate all element expressions *before* allocating. This keeps each
    // value in an SSA register (spilled to stack if needed; reachable by the
    // conservative stack scanner) so nested allocations inside element
    // expressions don't see a half-initialized outer array.
    let mut vals = Vec::with_capacity(n);
    let mut layout_notes_needed = Vec::with_capacity(n);
    for value_expr in elements {
        layout_notes_needed.push(!is_numeric_expr(ctx, value_expr));
        vals.push(lower_expr(ctx, value_expr)?);
    }

    // Inline bump-allocator path for small literals. Size threshold matches
    // `MAX_SCALAR_ARRAY_LEN` in collectors.rs so every candidate the escape
    // pass rejects can still benefit from the inline alloc.
    const INLINE_MAX_ELEMENTS: usize = 16;
    if n <= INLINE_MAX_ELEMENTS {
        // Layout constants — must match `ArrayHeader` in array.rs and
        // `GcHeader` in gc.rs. Duplicated here because codegen emits raw
        // byte offsets; the runtime declarations are authoritative.
        const GC_HEADER_SIZE: u64 = 8;
        const ARRAY_HEADER_SIZE: u64 = 8;
        const ELEMENT_SIZE: u64 = 8;
        const GC_TYPE_ARRAY: u64 = 1;
        const GC_FLAG_ARENA: u64 = 0x02;
        // PR #1146: pointer-free hint for slot-layout tracking — empty
        // inline literals only. The element-store loop below issues
        // per-slot `js_gc_note_slot_layout` for non-empty literals.
        const GC_LAYOUT_POINTER_FREE: u64 = 0x4000;

        let total_size = GC_HEADER_SIZE + ARRAY_HEADER_SIZE + (n as u64) * ELEMENT_SIZE;
        let total_size_str = total_size.to_string();

        // Lazy per-function slot for the arena state pointer. Reused for
        // `new ClassName()` inline allocs; first one to hit creates it.
        let arena_state_slot = if let Some(slot) = ctx.arena_state_slot.clone() {
            slot
        } else {
            let slot = ctx.func.entry_init_call_ptr("js_inline_arena_state");
            ctx.arena_state_slot = Some(slot.clone());
            slot
        };

        // Load state + compute bump check. `total_size` is always a
        // multiple of 8, every prior alloc rounds offset to 8, and blocks
        // start 8-aligned, so no align-up step is needed.
        let blk = ctx.block();
        let state_ptr = blk.load(PTR, &arena_state_slot);
        let offset_field_ptr = blk.gep(I8, &state_ptr, &[(I64, "8")]);
        let offset_val = blk.load(I64, &offset_field_ptr);
        let aligned_off = offset_val.clone();
        let new_offset = blk.add(I64, &aligned_off, &total_size_str);
        let size_field_ptr = blk.gep(I8, &state_ptr, &[(I64, "16")]);
        let size_val = blk.load(I64, &size_field_ptr);
        let fits = blk.icmp_ule(I64, &new_offset, &size_val);

        let fast_idx = ctx.new_block("arrlit.fast");
        let slow_idx = ctx.new_block("arrlit.slow");
        let merge_idx = ctx.new_block("arrlit.merge");
        let fast_label = ctx.block_label(fast_idx);
        let slow_label = ctx.block_label(slow_idx);
        let merge_label = ctx.block_label(merge_idx);

        ctx.block().cond_br(&fits, &fast_label, &slow_label);

        // Fast path: commit the bump, compute `data + offset`.
        ctx.current_block = fast_idx;
        let blk = ctx.block();
        blk.store(I64, &new_offset, &offset_field_ptr);
        let data_ptr = blk.load(PTR, &state_ptr);
        let raw_fast = blk.gep(I8, &data_ptr, &[(I64, &aligned_off)]);
        let fast_pred_label = blk.label.clone();
        blk.br(&merge_label);

        // Slow path: call the runtime slow-alloc (same one used by the
        // inline `new` path). Returns a fresh raw pointer (inclusive of
        // GcHeader space).
        ctx.current_block = slow_idx;
        let raw_slow = ctx.block().call(
            PTR,
            "js_inline_arena_slow_alloc",
            &[(PTR, &state_ptr), (I64, &total_size_str), (I64, "8")],
        );
        let slow_pred_label = ctx.block().label.clone();
        ctx.block().br(&merge_label);

        // Merge: phi the raw pointer and write everything.
        ctx.current_block = merge_idx;
        let blk = ctx.block();
        let raw = blk.phi(
            PTR,
            &[(&raw_fast, &fast_pred_label), (&raw_slow, &slow_pred_label)],
        );

        // Packed GcHeader (bits 0..7 obj_type, 8..15 gc_flags, 16..31
        // _reserved, 32..63 size). PR #1146 packs the layout-tag in the
        // reserved bits so the GC sees the array as pointer-free until
        // the element-store loop overrides per-slot via
        // `js_gc_note_slot_layout` below.
        let gc_packed: u64 = GC_TYPE_ARRAY
            | (GC_FLAG_ARENA << 8)
            | (GC_LAYOUT_POINTER_FREE << 16)
            | (total_size << 32);
        blk.store(I64, &gc_packed.to_string(), &raw);

        // Packed ArrayHeader at raw+8 (length low 32 / capacity high 32).
        let arr_header_addr = blk.gep(I8, &raw, &[(I64, "8")]);
        let arr_header_packed = (n as u64) | ((n as u64) << 32);
        blk.store(I64, &arr_header_packed.to_string(), &arr_header_addr);

        // User pointer = raw + GC_HEADER_SIZE. Computed before the
        // element loop so the per-slot layout notes target the correct
        // user-visible address.
        let user_ptr = blk.gep(I8, &raw, &[(I64, "8")]);
        let user_ptr_as_i64 = blk.ptrtoint(&user_ptr, I64);

        // Elements at raw+16 + i*8.
        for (i, v) in vals.iter().enumerate() {
            let offset = (16 + i * 8).to_string();
            let elem_ptr = blk.gep_inbounds(I8, &raw, &[(I64, &offset)]);
            blk.store(DOUBLE, v, &elem_ptr);
            if layout_notes_needed[i] {
                let value_bits = blk.bitcast_double_to_i64(v);
                let slot_index = i.to_string();
                emit_layout_note_slot_on_block(blk, &user_ptr_as_i64, &slot_index, &value_bits);
            }
        }

        return Ok(nanbox_pointer_inline(ctx.block(), &user_ptr_as_i64));
    }

    // Fallback for N > INLINE_MAX_ELEMENTS: keep the extern call + N inline
    // stores. Thin-LTO already inlines this call into user IR, so the cost
    // is ~1 inlined arena bump plus some LLVM churn around the arg pack.
    let cap_str = n.to_string();
    let arr = ctx
        .block()
        .call(I64, "js_array_alloc_literal", &[(I32, &cap_str)]);

    let arr_ptr = ctx.block().inttoptr(I64, &arr);
    for (i, v) in vals.iter().enumerate() {
        let offset = (8 + i * 8).to_string();
        let elem_ptr = ctx.block().gep_inbounds(I8, &arr_ptr, &[(I64, &offset)]);
        ctx.block().store(DOUBLE, v, &elem_ptr);
        if layout_notes_needed[i] {
            let value_bits = ctx.block().bitcast_double_to_i64(v);
            let elem_addr = ctx.block().ptrtoint(&elem_ptr, I64);
            let slot_index = i.to_string();
            emit_layout_note_slot_on_block(ctx.block(), &arr, &slot_index, &value_bits);
            emit_write_barrier_slot_on_block(ctx.block(), &arr, &elem_addr, &value_bits);
        }
    }

    Ok(nanbox_pointer_inline(ctx.block(), &arr))
}
