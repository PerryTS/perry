//! Codegen access to a scoped binding's slot.
//!
//! A scoped binding's frame "slot" (`ctx.locals[id]`) is the group's single
//! root, shared by every member, and its closure capture index is the group's
//! capture slot. Both hold the scope object's base pointer. The binding's cell
//! is `base + 8 * index`; that interior address is derived after every load of
//! the base and never held across a collection point (the reload pass
//! re-derives it through `add`, which is a transparent derivation).
//!
//! A read is a plain load and a write a plain store plus the ordinary write
//! barrier with the object as parent, so neither is a call on the fast path
//! and neither is a safepoint. (The per-binding accessor `js_box_get_bits` is
//! a safepoint because its TDZ arm allocates; a scope slot takes that arm only
//! for a TDZ-seeded group, out of line.)

use anyhow::Result;

use super::ScopeSlot;
use crate::expr::FnCtx;
use crate::types::{DOUBLE, I64};

/// `id`'s place in its scope object, when it has one in this module. Module
/// globals keep their global storage even if a stale group names them.
pub(crate) fn slot(ctx: &FnCtx<'_>, id: u32) -> Option<ScopeSlot> {
    if ctx.module_globals.contains_key(&id) {
        return None;
    }
    ctx.scope_map.slot(id)
}

/// Byte offset of slot `index` from the object's user pointer.
pub(crate) fn slot_offset(index: u32) -> u64 {
    8 * index as u64
}

/// Load the scope object base for scoped `id`: the closure capture when the
/// binding is captured, otherwise the frame root. `None` when `id` is not
/// scoped or this function has no storage for it.
pub(crate) fn load_base(ctx: &mut FnCtx<'_>, id: u32) -> Result<Option<(ScopeSlot, String)>> {
    let Some(slot) = slot(ctx, id) else {
        return Ok(None);
    };
    if let Some(capture) = ctx.trusted_box_capture_ptrs.get(&id).cloned() {
        return Ok(Some((slot, capture.bits)));
    }
    if let Some(&capture_idx) = ctx.closure_captures.get(&id) {
        let closure_ptr = crate::expr::current_closure_ptr_value(ctx, "scoped capture")?;
        let offset = crate::target_layout::closure_header_size_bytes(ctx.target_triple)
            + 8 * capture_idx as u64;
        let blk = ctx.block();
        let slot_addr = blk.add(I64, &closure_ptr, &offset.to_string());
        let slot_ptr = blk.inttoptr(I64, &slot_addr);
        let base = blk.load(I64, &slot_ptr);
        return Ok(Some((slot, base)));
    }
    if let Some(root) = ctx.locals.get(&id).cloned() {
        let base = ctx.block().load(I64, &root);
        return Ok(Some((slot, base)));
    }
    Ok(None)
}

/// The cell address (`i64`) of `slot` in the object at `base`.
pub(crate) fn cell_addr(ctx: &mut FnCtx<'_>, slot: ScopeSlot, base: &str) -> String {
    if slot.index == 0 {
        return base.to_string();
    }
    ctx.block()
        .add(I64, base, &slot_offset(slot.index).to_string())
}

/// Read `id`'s current value bits. A plain load: no call, so no safepoint.
/// A group seeded for the Temporal Dead Zone adds a compare with a cold call
/// into the trusted box accessor, which raises the ReferenceError.
pub(crate) fn read_bits(ctx: &mut FnCtx<'_>, id: u32, slot: ScopeSlot, base: &str) -> String {
    let addr = cell_addr(ctx, slot, base);
    let ptr = ctx.block().inttoptr(I64, &addr);
    let bits = ctx.block().load(I64, &ptr);
    if !slot.tdz {
        return bits;
    }
    let is_tdz = ctx.block().icmp_eq(I64, &bits, crate::nanbox::TAG_TDZ_I64);
    let slow_idx = ctx.new_block("scope_slot.tdz");
    let merge_idx = ctx.new_block("scope_slot.read");
    let slow_label = ctx.block_label(slow_idx);
    let merge_label = ctx.block_label(merge_idx);
    let fast_label = ctx.block().label.clone();
    ctx.block().cond_br(&is_tdz, &slow_label, &merge_label);

    ctx.current_block = slow_idx;
    // The accessor throws for a real dead-zone read (allocating the error),
    // so a versioned-loop clone must poison its caller first.
    crate::expr::emit_versioned_loop_callback_deopt(ctx);
    let slow_bits = if let Some(name) = ctx.strings.tdz_binding_names.get(&id).cloned() {
        let interned = ctx.strings.intern(&name);
        let global = format!("@{}", ctx.strings.entry(interned).handle_global);
        let name = ctx.block().load(DOUBLE, &global);
        ctx.block().call(
            I64,
            "js_box_get_bits_trusted_named",
            &[(I64, &addr), (DOUBLE, &name)],
        )
    } else {
        ctx.block()
            .call(I64, "js_box_get_bits_trusted", &[(I64, &addr)])
    };
    let slow_end = ctx.block().label.clone();
    ctx.block().br(&merge_label);

    ctx.current_block = merge_idx;
    ctx.block()
        .phi(I64, &[(&bits, &fast_label), (&slow_bits, &slow_end)])
}

/// Store `bits` into the slot and shade the edge with the scope object as the
/// barrier parent.
pub(crate) fn write_bits(ctx: &mut FnCtx<'_>, slot: ScopeSlot, base: &str, bits: &str) {
    let addr = cell_addr(ctx, slot, base);
    let ptr = ctx.block().inttoptr(I64, &addr);
    ctx.block().store(I64, bits, &ptr);
    crate::expr::emit_write_barrier(ctx, base, bits);
}

/// Load `id`'s base and read it. `None` when `id` is not scoped here.
pub(crate) fn read_scoped(ctx: &mut FnCtx<'_>, id: u32) -> Result<Option<String>> {
    let Some((slot, base)) = load_base(ctx, id)? else {
        return Ok(None);
    };
    Ok(Some(read_bits(ctx, id, slot, &base)))
}

/// Load `id`'s base and store `bits`. Returns false when `id` is not scoped
/// here. `bits` must already be computed: the base is loaded after it.
pub(crate) fn write_scoped(ctx: &mut FnCtx<'_>, id: u32, bits: &str) -> Result<bool> {
    let Some((slot, base)) = load_base(ctx, id)? else {
        return Ok(false);
    };
    write_bits(ctx, slot, &base, bits);
    Ok(true)
}

/// Write a relocated array head (`new_box`, a NaN-boxed pointer) back to a
/// BOXED binding's cell — a scope slot or a per-binding box — instead of the
/// frame slot, which holds the cell's container, not the array. Returns false
/// for an unboxed binding (the caller stores into its frame slot as before).
pub(crate) fn write_back_boxed_local(ctx: &mut FnCtx<'_>, id: u32, new_box: &str) -> Result<bool> {
    if !ctx.boxed_vars.contains(&id) || ctx.module_globals.contains_key(&id) {
        return Ok(false);
    }
    let bits = ctx.block().bitcast_double_to_i64(new_box);
    if write_scoped(ctx, id, &bits)? {
        return Ok(true);
    }
    if let Some(cell) = crate::expr::load_boxed_local_pointer(ctx, id)? {
        ctx.block()
            .call_void("js_box_set_bits", &[(I64, &cell), (I64, &bits)]);
        crate::expr::emit_write_barrier(ctx, &cell, &bits);
    }
    Ok(true)
}
