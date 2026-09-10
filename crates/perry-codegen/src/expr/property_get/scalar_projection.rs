//! Fuse a pure-local indexed property read over a lazy JSON record.
//! General receivers and side-effecting indices keep ordinary lowering.

use super::*;

pub(super) fn try_lower(
    ctx: &mut FnCtx<'_>,
    object: &Expr,
    property: &str,
    byte_offset: u32,
) -> Result<Option<String>> {
    let Expr::IndexGet {
        object: base,
        index,
    } = object
    else {
        return Ok(None);
    };
    if !matches!(base.as_ref(), Expr::LocalGet(_))
        || !matches!(
            index.as_ref(),
            Expr::LocalGet(_) | Expr::Integer(_) | Expr::Number(_)
        )
        || rooting::operand_may_collect(ctx, base)
        || rooting::operand_may_collect(ctx, index)
        || !property.is_ascii()
        || property.as_bytes().contains(&b'"')
        || property.as_bytes().contains(&b'\\')
        || super::super::typed_feedback_emission_enabled()
    {
        return Ok(None);
    }
    // Both operands are reads/literals proven not to call or collect. The
    // runtime probe validates that the index is numeric without coercion;
    // an object/string/Symbol key takes the unchanged ordinary expression.
    // scalar probe also cannot collect or enter user code, so an ordinary
    // fallback may reload these same locals without duplicating any observable
    // evaluation. Do not widen admission to getters/calls or coercions without
    // capturing and rooting their once-evaluated operands across the fallback.
    let base_value = lower_expr(ctx, base)?;
    let index_value = lower_expr(ctx, index)?;
    let key_idx = ctx.strings.intern(property);
    let key_bytes = format!("@{}", ctx.strings.entry(key_idx).bytes_global);
    let key_len = property.len().to_string();
    let projected = ctx.block().call(
        DOUBLE,
        "js_json_lazy_index_scalar",
        &[
            (DOUBLE, &base_value),
            (DOUBLE, &index_value),
            (PTR, &key_bytes),
            (I64, &key_len),
        ],
    );
    let bits = ctx.block().bitcast_double_to_i64(&projected);
    let missed = ctx.block().icmp_eq(I64, &bits, crate::nanbox::TAG_HOLE_I64);
    let hit_end = ctx.block().label.clone();
    let miss_block = ctx.new_block("json.scalar.miss");
    let merge_block = ctx.new_block("json.scalar.merge");
    let miss_label = ctx.block_label(miss_block);
    let merge_label = ctx.block_label(merge_block);
    ctx.block().cond_br(&missed, &miss_label, &merge_label);

    ctx.current_block = miss_block;
    let ordinary =
        generic_dispatch::lower_generic_property_get_ordinary(ctx, object, property, byte_offset)?;
    let miss_end = ctx.block().label.clone();
    ctx.block().br(&merge_label);

    ctx.current_block = merge_block;
    Ok(Some(ctx.block().phi(
        DOUBLE,
        &[(&projected, &hit_end), (&ordinary, &miss_end)],
    )))
}
