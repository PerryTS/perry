use anyhow::Result;
use perry_hir::Expr;

use crate::expr::{lower_expr, unbox_to_i64, FnCtx};
use crate::nanbox::double_literal;
use crate::type_analysis::receiver_class_name;
use crate::types::{DOUBLE, I64};

fn is_event_target_expr(ctx: &FnCtx<'_>, e: &Expr) -> bool {
    matches!(receiver_class_name(ctx, e).as_deref(), Some("EventTarget"))
}

pub(super) fn lower_event_target_call(
    ctx: &mut FnCtx<'_>,
    object: &Expr,
    property: &str,
    args: &[Expr],
) -> Result<Option<String>> {
    if !is_event_target_expr(ctx, object) {
        return Ok(None);
    }
    if (property == "addEventListener" || property == "removeEventListener") && args.len() >= 2 {
        let target_box = lower_expr(ctx, object)?;
        let event_box = lower_expr(ctx, &args[0])?;
        let listener_box = lower_expr(ctx, &args[1])?;
        let options_box = if let Some(options) = args.get(2) {
            lower_expr(ctx, options)?
        } else {
            double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED))
        };
        for a in args.iter().skip(3) {
            let _ = lower_expr(ctx, a)?;
        }
        let blk = ctx.block();
        let target = unbox_to_i64(blk, &target_box);
        let event = blk.call(
            I64,
            "js_get_string_pointer_unified",
            &[(DOUBLE, &event_box)],
        );
        let listener = unbox_to_i64(blk, &listener_box);
        let runtime = if property == "addEventListener" {
            "js_event_target_add_event_listener_with_options"
        } else {
            "js_event_target_remove_event_listener_with_options"
        };
        blk.call_void(
            runtime,
            &[
                (I64, &target),
                (I64, &event),
                (I64, &listener),
                (DOUBLE, &options_box),
            ],
        );
        return Ok(Some(double_literal(f64::from_bits(
            crate::nanbox::TAG_UNDEFINED,
        ))));
    }
    if property == "dispatchEvent" {
        let target_box = lower_expr(ctx, object)?;
        let event_box = if let Some(event) = args.first() {
            lower_expr(ctx, event)?
        } else {
            double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED))
        };
        for a in args.iter().skip(1) {
            let _ = lower_expr(ctx, a)?;
        }
        let blk = ctx.block();
        let target = unbox_to_i64(blk, &target_box);
        return Ok(Some(blk.call(
            DOUBLE,
            "js_event_target_dispatch_event",
            &[(I64, &target), (DOUBLE, &event_box)],
        )));
    }
    Ok(None)
}
