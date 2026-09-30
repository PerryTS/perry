//! A reused stack slot is not proof its boxed declaration executed (#10048).
//! Generator continuations can emit the declaration on mutually exclusive paths.
//! Preserve live var cells, but create a missing cell before a self-capturing init.
use crate::expr::FnCtx;
use crate::types::{I32, I64};

pub(super) fn ensure_reused_box_is_initialized(ctx: &mut FnCtx<'_>, id: u32) {
    if !ctx.boxed_vars.contains(&id)
        || ctx.prealloc_boxes.contains(&id)
        || ctx.module_globals.contains_key(&id)
    {
        return;
    }
    let Some(slot) = ctx.locals.get(&id).cloned() else {
        return;
    };
    let pointer = ctx.block().load(I64, &slot);
    let missing = ctx
        .block()
        .icmp_eq(I64, &pointer, crate::nanbox::TAG_UNDEFINED_I64);
    let allocate = ctx.new_block("boxed.reuse.allocate");
    let ready = ctx.new_block("boxed.reuse.ready");
    let allocate_label = ctx.block_label(allocate);
    let ready_label = ctx.block_label(ready);
    ctx.block().cond_br(&missing, &allocate_label, &ready_label);
    ctx.current_block = allocate;
    let cell = if crate::expr::is_compiler_private_async_i32_control_local(ctx, id) {
        ctx.block().call(I64, "js_i32_box_alloc", &[(I32, "0")])
    } else if crate::expr::is_compiler_private_async_i1_control_local(ctx, id) {
        ctx.block().call(I64, "js_bool_box_alloc", &[(I32, "0")])
    } else {
        ctx.block().call(
            I64,
            "js_box_alloc_bits",
            &[(I64, crate::nanbox::TAG_UNDEFINED_I64)],
        )
    };
    ctx.block().store(I64, &cell, &slot);
    super::record_boxed_slot_js_value_bits(ctx, id, &cell, "boxed_let.reused_missing_box");
    ctx.block().br(&ready_label);
    ctx.current_block = ready;
}

/// End a TDZ-seeded binding's dead zone (`let x;` with no initializer) by
/// storing `undefined` into its cell — a per-binding box or a scope slot.
pub(super) fn end_dead_zone(ctx: &mut FnCtx<'_>, id: u32) -> anyhow::Result<()> {
    let undef_bits = crate::nanbox::TAG_UNDEFINED_I64.to_string();
    if crate::scope_env::access::write_scoped(ctx, id, &undef_bits)? {
        return Ok(());
    }
    if let Some(slot) = ctx.locals.get(&id).cloned() {
        let blk = ctx.block();
        let bptr = blk.load(I64, &slot);
        blk.call_void("js_box_set_bits", &[(I64, &bptr), (I64, &undef_bits)]);
    }
    Ok(())
}
