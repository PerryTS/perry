//! Allocate a GC cell and publish it into its frame's precise root slot.
use crate::expr::FnCtx;
use crate::types::{LlvmType, I64};

pub(crate) fn mint_frame_cell(
    ctx: &mut FnCtx<'_>,
    slot: &str,
    alloc_fn: &str,
    args: &[(LlvmType, &str)],
) -> String {
    let cell = ctx.block().call(I64, alloc_fn, args);
    ctx.block().store(I64, &cell, slot);
    cell
}
