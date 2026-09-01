//! Raw reads backed by a scoped, prevalidated string-array window.

use perry_hir::Expr;

use crate::types::{DOUBLE, I32, I64};

use super::{lower_expr, lower_expr_as_i32, FnCtx, StringWindowArrayFact};

/// Find the innermost active fact whose validated window covers `index`.
pub(crate) fn fact_for_index(
    ctx: &FnCtx<'_>,
    array_local_id: u32,
    index: &Expr,
) -> Option<StringWindowArrayFact> {
    let (lo, hi) = crate::collectors::static_index_window(index)?;
    ctx.string_window_array_facts
        .iter()
        .rev()
        .find(|fact| {
            fact.array_local_id == array_local_id
                && lo >= fact.min_idx
                && hi < fact.max_idx_exclusive
        })
        .cloned()
}

/// Whether an `IndexGet` is covered by an active string-window proof.
pub(crate) fn proves_index_string(ctx: &FnCtx<'_>, expr: &Expr) -> bool {
    let Expr::IndexGet { object, index } = expr else {
        return false;
    };
    let Expr::LocalGet(array_local_id) = object.as_ref() else {
        return false;
    };
    fact_for_index(ctx, *array_local_id, index).is_some()
}

/// Lower a covered `array[index]` to `load double` at
/// `ArrayHeader + index * sizeof(JSValue)`. Re-reading the binding at every
/// access observes any GC move while still bypassing the generic array
/// guard/descriptor/hole diamond.
pub(crate) fn try_lower_index_get(
    ctx: &mut FnCtx<'_>,
    object: &Expr,
    index: &Expr,
) -> anyhow::Result<Option<String>> {
    let Expr::LocalGet(array_local_id) = object else {
        return Ok(None);
    };
    if fact_for_index(ctx, *array_local_id, index).is_none() {
        return Ok(None);
    }
    let array_box = lower_expr(ctx, object)?;
    let index_i32 = lower_expr_as_i32(ctx, index)?;
    let handle = super::receiver_descriptor_handle_i64(ctx, Some(*array_local_id), &array_box);
    let block = ctx.block();
    let index_i64 = block.zext(I32, &index_i32, I64);
    let byte_offset = block.shl(I64, &index_i64, "3");
    let slot_offset = block.add(I64, &byte_offset, "8");
    let slot_addr = block.add(I64, &handle, &slot_offset);
    let slot_ptr = block.inttoptr(I64, &slot_addr);
    Ok(Some(block.load(DOUBLE, &slot_ptr)))
}
