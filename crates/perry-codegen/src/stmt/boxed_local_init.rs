//! A reused stack slot is not proof its boxed declaration executed (#10048).
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
