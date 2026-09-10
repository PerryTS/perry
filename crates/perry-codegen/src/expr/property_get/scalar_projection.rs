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
        || property.len() > 7
        || crate::target_layout::target_is_ilp32(ctx.target_triple)
        || !property.is_ascii()
        || property.as_bytes().contains(&b'"')
        || property.as_bytes().contains(&b'\\')
        || super::super::typed_feedback_emission_enabled()
    {
        return Ok(None);
    }
    // IndexGet retains all its existing specialization order. Only its generic
    // dynamic-index tier installs the optional lazy-brand edge; specialized
    // ordinary reads are therefore lowered exactly as before.
    let mut projection = super::super::index_get::ScalarProjection::new(property);
    let element =
        super::super::index_get::lower_with_scalar_projection(ctx, object, Some(&mut projection))?;
    let ordinary = generic_dispatch::lower_generic_property_get_value(
        ctx,
        object,
        property,
        byte_offset,
        &element,
    )?;
    let Some(merge_block) = projection.merge_block else {
        return Ok(Some(ordinary));
    };
    let ordinary_end = ctx.block().label.clone();
    let merge_label = ctx.block_label(merge_block);
    ctx.block().br(&merge_label);
    ctx.current_block = merge_block;
    projection.incoming.push((ordinary, ordinary_end));
    let incoming: Vec<(&str, &str)> = projection
        .incoming
        .iter()
        .map(|(value, label)| (value.as_str(), label.as_str()))
        .collect();
    Ok(Some(ctx.block().phi(DOUBLE, &incoming)))
}
