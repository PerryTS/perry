//! `apply_box_style` and `emit_dim_setter` — perry/tui `Box(...)`
//! style-options destructure helpers. Split out of
//! `lower_call/native.rs` (~205 LOC) so the parent module stays
//! under the 2000-line cap.

use perry_hir::Expr;

use crate::expr::{lower_expr, FnCtx};
use crate::nanbox::double_literal;
use crate::types::{DOUBLE, I64};

use super::*;

/// Apply a perry/tui Box style options object — recognized as a
/// trailing arg in `Box({ flexDirection: "row", gap: 1 }, [children])`
/// — by emitting per-field `js_perry_tui_box_set_*` FFI calls. The
/// parent handle is reloaded from `parent_slot` for each setter so
/// inter-call SSA isn't an issue. Unknown fields are silently dropped
/// (forward-compat).
pub(super) fn apply_box_style(
    ctx: &mut FnCtx<'_>,
    parent_slot: &str,
    style_arg: &Expr,
) -> anyhow::Result<()> {
    let Some(props) = extract_options_fields(ctx, style_arg) else {
        return Ok(());
    };
    for (key, val) in &props {
        // Reload parent handle each iteration so the SSA name is
        // valid in the current block (apply_inline_style does the
        // same thing). The slot holds a raw i64 handle now (see the
        // Box recognizer's call to js_perry_tui_box) so no unbox.
        let blk = ctx.block();
        let parent_handle = blk.load(I64, parent_slot);
        match key.as_str() {
            "flexDirection" => {
                let s = get_raw_string_ptr(ctx, val)?;
                ctx.pending_declares.push((
                    "js_perry_tui_box_set_flex_direction".to_string(),
                    DOUBLE,
                    vec![I64, I64],
                ));
                ctx.block().call(
                    DOUBLE,
                    "js_perry_tui_box_set_flex_direction",
                    &[(I64, &parent_handle), (I64, &s)],
                );
            }
            "justifyContent" => {
                let s = get_raw_string_ptr(ctx, val)?;
                ctx.pending_declares.push((
                    "js_perry_tui_box_set_justify_content".to_string(),
                    DOUBLE,
                    vec![I64, I64],
                ));
                ctx.block().call(
                    DOUBLE,
                    "js_perry_tui_box_set_justify_content",
                    &[(I64, &parent_handle), (I64, &s)],
                );
            }
            "alignItems" => {
                let s = get_raw_string_ptr(ctx, val)?;
                ctx.pending_declares.push((
                    "js_perry_tui_box_set_align_items".to_string(),
                    DOUBLE,
                    vec![I64, I64],
                ));
                ctx.block().call(
                    DOUBLE,
                    "js_perry_tui_box_set_align_items",
                    &[(I64, &parent_handle), (I64, &s)],
                );
            }
            "gap" => {
                let v = lower_expr(ctx, val)?;
                ctx.pending_declares.push((
                    "js_perry_tui_box_set_gap".to_string(),
                    DOUBLE,
                    vec![I64, DOUBLE],
                ));
                ctx.block().call(
                    DOUBLE,
                    "js_perry_tui_box_set_gap",
                    &[(I64, &parent_handle), (DOUBLE, &v)],
                );
            }
            "padding" => {
                // Two shapes: a number (uniform), or an object literal
                // `{ top, right, bottom, left }` (per-side, #405). The
                // nested object literal lands as `Expr::New { class_name:
                // __AnonShape_… }` after HIR lowering, so use
                // `extract_options_fields` (which handles both shapes).
                if let Some(fields) = extract_options_fields(ctx, val) {
                    let pad_side = |key: &str| -> Expr {
                        fields
                            .iter()
                            .find(|(k, _)| k == key)
                            .map(|(_, v)| v.clone())
                            .unwrap_or(Expr::Number(0.0))
                    };
                    let top = lower_expr(ctx, &pad_side("top"))?;
                    let right = lower_expr(ctx, &pad_side("right"))?;
                    let bottom = lower_expr(ctx, &pad_side("bottom"))?;
                    let left = lower_expr(ctx, &pad_side("left"))?;
                    ctx.pending_declares.push((
                        "js_perry_tui_box_set_padding_each".to_string(),
                        DOUBLE,
                        vec![I64, DOUBLE, DOUBLE, DOUBLE, DOUBLE],
                    ));
                    ctx.block().call(
                        DOUBLE,
                        "js_perry_tui_box_set_padding_each",
                        &[
                            (I64, &parent_handle),
                            (DOUBLE, &top),
                            (DOUBLE, &right),
                            (DOUBLE, &bottom),
                            (DOUBLE, &left),
                        ],
                    );
                } else {
                    let v = lower_expr(ctx, val)?;
                    ctx.pending_declares.push((
                        "js_perry_tui_box_set_padding".to_string(),
                        DOUBLE,
                        vec![I64, DOUBLE],
                    ));
                    ctx.block().call(
                        DOUBLE,
                        "js_perry_tui_box_set_padding",
                        &[(I64, &parent_handle), (DOUBLE, &v)],
                    );
                }
            }
            "width" => emit_dim_setter(
                ctx,
                &parent_handle,
                val,
                "js_perry_tui_box_set_width",
                "js_perry_tui_box_set_width_pct",
            )?,
            "height" => emit_dim_setter(
                ctx,
                &parent_handle,
                val,
                "js_perry_tui_box_set_height",
                "js_perry_tui_box_set_height_pct",
            )?,
            "flexGrow" => {
                let v = lower_expr(ctx, val)?;
                ctx.pending_declares.push((
                    "js_perry_tui_box_set_flex_grow".to_string(),
                    DOUBLE,
                    vec![I64, DOUBLE],
                ));
                ctx.block().call(
                    DOUBLE,
                    "js_perry_tui_box_set_flex_grow",
                    &[(I64, &parent_handle), (DOUBLE, &v)],
                );
            }
            "flexShrink" => {
                let v = lower_expr(ctx, val)?;
                ctx.pending_declares.push((
                    "js_perry_tui_box_set_flex_shrink".to_string(),
                    DOUBLE,
                    vec![I64, DOUBLE],
                ));
                ctx.block().call(
                    DOUBLE,
                    "js_perry_tui_box_set_flex_shrink",
                    &[(I64, &parent_handle), (DOUBLE, &v)],
                );
            }
            "flexBasis" => emit_dim_setter(
                ctx,
                &parent_handle,
                val,
                "js_perry_tui_box_set_flex_basis",
                "js_perry_tui_box_set_flex_basis_pct",
            )?,
            _ => {} // Unknown field — silently drop for forward-compat.
        }
    }
    Ok(())
}

/// Emit a width / height / flex-basis setter call. If `val` is a
/// string literal ending in `%`, parse the prefix and dispatch to the
/// percent variant; otherwise lower as a number and dispatch to the
/// cells variant. Other string shapes (e.g. dynamic strings) fall
/// through the cells path with an undefined-as-NaN value — out of
/// scope for this fix; users with dynamic dimensions pass numbers.
/// (#405 Phase 3.5.)
pub(super) fn emit_dim_setter(
    ctx: &mut FnCtx<'_>,
    parent_handle: &str,
    val: &Expr,
    cells_fn: &str,
    pct_fn: &str,
) -> anyhow::Result<()> {
    if let Expr::String(s) = val {
        if let Some(rest) = s.strip_suffix('%') {
            if let Ok(pct) = rest.trim().parse::<f64>() {
                let lit = double_literal(pct);
                ctx.pending_declares
                    .push((pct_fn.to_string(), DOUBLE, vec![I64, DOUBLE]));
                ctx.block()
                    .call(DOUBLE, pct_fn, &[(I64, parent_handle), (DOUBLE, &lit)]);
                return Ok(());
            }
        }
    }
    let v = lower_expr(ctx, val)?;
    ctx.pending_declares
        .push((cells_fn.to_string(), DOUBLE, vec![I64, DOUBLE]));
    ctx.block()
        .call(DOUBLE, cells_fn, &[(I64, parent_handle), (DOUBLE, &v)]);
    Ok(())
}
