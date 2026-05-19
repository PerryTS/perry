//! Array index-set fast-path lowering (extracted from `expr.rs`, issue
//! #1098). Pure move — no logic changes.

use anyhow::{anyhow, Result};

use super::{emit_write_barrier_slot_on_block, nanbox_pointer_inline, FnCtx};
use crate::block::LlBlock;
use crate::nanbox::POINTER_MASK_I64;
use crate::types::{DOUBLE, I32, I64, I8};

/// Inline fast-path lowering for `local_arr[i] = v`.
///
/// Compiles to:
///
/// ```text
///   <current>:
///     %arr_handle = unbox(arr_box)
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
///     br i1 %within_cap, label %extend_inline, label %realloc
///
///   extend_inline:
///     store double %v, ptr %element_ptr
///     %new_len = add i32 %idx, 1
///     store i32 %new_len, ptr @ arr_handle+0
///     br merge
///
///   realloc:
///     %new_handle = call i64 @js_array_set_f64_extend(...)
///     %new_box = nanbox_pointer_inline(new_handle)
///     store double %new_box, ptr %local_slot
///     br merge
///
///   merge:
///     <continues here>
/// ```
///
/// The first two paths are pure inline IR — no function calls, no extra
/// memory loads. The third path only fires when the array actually has
/// to grow (~17 times for a 100K-element build with doubling growth).
pub(crate) fn lower_index_set_fast(
    ctx: &mut FnCtx<'_>,
    arr_box: &str,
    idx_double: &str,
    val_double: &str,
    local_id: u32,
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

    // Issue #233: detect FORWARDED arrays (post-grow stale pointers
    // from async-fn parameter handoff) and route to the realloc slow
    // path. The slow path's `js_array_set_f64_extend` →
    // `clean_arr_ptr_mut` follows the forwarding chain and writes
    // into the live new array. Without this guard, length+capacity
    // read at offsets 0/4 would be the lower 32 bits of the
    // forwarding pointer (garbage) and the inline element store at
    // arr+8+idx*8 would corrupt unrelated memory.
    let gc_flags_addr = blk.sub(I64, &arr_handle, "7");
    let gc_flags_ptr = blk.inttoptr(I64, &gc_flags_addr);
    let gc_flags = blk.load(I8, &gc_flags_ptr);
    let fwd_bits = blk.and(I8, &gc_flags, "128"); // GC_FLAG_FORWARDED
    let is_fwd = blk.icmp_ne(I8, &fwd_bits, "0");

    let fwd_idx = ctx.new_block("idxset.fwd");
    let nofwd_idx = ctx.new_block("idxset.nofwd");
    let inbounds_idx = ctx.new_block("idxset.inbounds");
    let check_cap_idx = ctx.new_block("idxset.check_cap");
    let extend_inline_idx = ctx.new_block("idxset.extend_inline");
    let realloc_idx = ctx.new_block("idxset.realloc");
    let merge_idx = ctx.new_block("idxset.merge");

    let fwd_label = ctx.block_label(fwd_idx);
    let nofwd_label = ctx.block_label(nofwd_idx);
    let inbounds_label = ctx.block_label(inbounds_idx);
    let check_cap_label = ctx.block_label(check_cap_idx);
    let extend_inline_label = ctx.block_label(extend_inline_idx);
    let realloc_label = ctx.block_label(realloc_idx);
    let merge_label = ctx.block_label(merge_idx);

    ctx.block().cond_br(&is_fwd, &fwd_label, &nofwd_label);

    // FORWARDED branch: same shape as the realloc slow path —
    // js_array_set_f64_extend handles forwarding via clean_arr_ptr.
    ctx.current_block = fwd_idx;
    {
        let blk = ctx.block();
        let new_handle = blk.call(
            I64,
            "js_array_set_f64_extend",
            &[(I64, &arr_handle), (I32, &idx_i32), (DOUBLE, val_double)],
        );
        let new_box = nanbox_pointer_inline(blk, &new_handle);
        blk.store(DOUBLE, &new_box, &slot);
        let val_bits = blk.bitcast_double_to_i64(val_double);
        emit_write_barrier_slot_on_block(blk, &arr_handle, "0", &val_bits);
        blk.br(&merge_label);
    }

    ctx.current_block = nofwd_idx;
    // Load length from offset 0 (null-guarded).
    let length = ctx.block().safe_load_i32_from_ptr(&arr_handle);
    let in_bounds = ctx.block().icmp_ult(I32, &idx_i32, &length);
    ctx.block()
        .cond_br(&in_bounds, &inbounds_label, &check_cap_label);

    // Helper: compute element_ptr = arr_ptr + 8 + idx*8 and emit a store.
    fn store_element(
        blk: &mut LlBlock,
        arr_handle: &str,
        idx_i32: &str,
        val_double: &str,
    ) -> String {
        let idx_i64 = blk.zext(I32, idx_i32, I64);
        let byte_offset = blk.shl(I64, &idx_i64, "3"); // *8
        let with_header = blk.add(I64, &byte_offset, "8"); // +8 for header
        let element_addr = blk.add(I64, arr_handle, &with_header);
        let element_ptr = blk.inttoptr(I64, &element_addr);
        blk.store(DOUBLE, val_double, &element_ptr);
        element_addr
    }

    // FASTEST: in-bounds path. Store directly, jump to merge.
    ctx.current_block = inbounds_idx;
    {
        let blk = ctx.block();
        let element_addr = store_element(blk, &arr_handle, &idx_i32, val_double);
        let val_bits = blk.bitcast_double_to_i64(val_double);
        emit_write_barrier_slot_on_block(blk, &arr_handle, &element_addr, &val_bits);
        blk.br(&merge_label);
    }

    // MEDIUM: idx >= length but < capacity. Store + bump length.
    ctx.current_block = check_cap_idx;
    let capacity = {
        let blk = ctx.block();
        // Load capacity from offset 4 — we need a typed pointer that
        // points 4 bytes into the array header. Use inttoptr after add.
        let cap_addr = blk.add(I64, &arr_handle, "4");
        let cap_ptr = blk.inttoptr(I64, &cap_addr);
        blk.load(I32, &cap_ptr)
    };
    let within_cap = ctx.block().icmp_ult(I32, &idx_i32, &capacity);
    ctx.block()
        .cond_br(&within_cap, &extend_inline_label, &realloc_label);

    ctx.current_block = extend_inline_idx;
    {
        let blk = ctx.block();
        let element_addr = store_element(blk, &arr_handle, &idx_i32, val_double);
        // Bump length: store idx+1 to arr_ptr+0.
        let new_len = blk.add(I32, &idx_i32, "1");
        let len_ptr = blk.inttoptr(I64, &arr_handle); // length is at offset 0
        blk.store(I32, &new_len, &len_ptr);
        let val_bits = blk.bitcast_double_to_i64(val_double);
        emit_write_barrier_slot_on_block(blk, &arr_handle, &element_addr, &val_bits);
        blk.br(&merge_label);
    }

    // SLOW: realloc needed. Call the runtime, write new ptr to local.
    ctx.current_block = realloc_idx;
    {
        let blk = ctx.block();
        let new_handle = blk.call(
            I64,
            "js_array_set_f64_extend",
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
