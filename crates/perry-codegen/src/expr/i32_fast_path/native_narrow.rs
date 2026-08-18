use anyhow::Result;
use perry_hir::Expr;

use crate::native_value::LoweredValue;
use crate::types::{DOUBLE, I16, I64, I8};

use super::{
    i16_lowered, i8_lowered, isize_lowered, lower_expr, native_expr_kind, u16_lowered, u8_lowered,
    FnCtx,
};

pub(super) fn lower_expr_native_u8(ctx: &mut FnCtx<'_>, e: &Expr) -> Result<LoweredValue> {
    let value = match e {
        Expr::Integer(n) if u8::try_from(*n).is_ok() => (*n as u8).to_string(),
        _ => {
            let value = lower_expr(ctx, e)?;
            ctx.block().fptoui(DOUBLE, &value, I8)
        }
    };
    record_narrow(ctx, e, "lower_expr_native_u8", u8_lowered(value))
}

pub(super) fn lower_expr_native_i8(ctx: &mut FnCtx<'_>, e: &Expr) -> Result<LoweredValue> {
    let value = match e {
        Expr::Integer(n) if i8::try_from(*n).is_ok() => (*n as i8).to_string(),
        _ => {
            let value = lower_expr(ctx, e)?;
            ctx.block().fptosi(DOUBLE, &value, I8)
        }
    };
    record_narrow(ctx, e, "lower_expr_native_i8", i8_lowered(value))
}

pub(super) fn lower_expr_native_i16(ctx: &mut FnCtx<'_>, e: &Expr) -> Result<LoweredValue> {
    let value = match e {
        Expr::Integer(n) if i16::try_from(*n).is_ok() => (*n as i16).to_string(),
        _ => {
            let value = lower_expr(ctx, e)?;
            ctx.block().fptosi(DOUBLE, &value, I16)
        }
    };
    record_narrow(ctx, e, "lower_expr_native_i16", i16_lowered(value))
}

pub(super) fn lower_expr_native_u16(ctx: &mut FnCtx<'_>, e: &Expr) -> Result<LoweredValue> {
    let value = match e {
        Expr::Integer(n) if u16::try_from(*n).is_ok() => (*n as u16).to_string(),
        _ => {
            let value = lower_expr(ctx, e)?;
            ctx.block().fptoui(DOUBLE, &value, I16)
        }
    };
    record_narrow(ctx, e, "lower_expr_native_u16", u16_lowered(value))
}

pub(super) fn lower_expr_native_isize(ctx: &mut FnCtx<'_>, e: &Expr) -> Result<LoweredValue> {
    let value = match e {
        Expr::Integer(n) => n.to_string(),
        _ => {
            let value = lower_expr(ctx, e)?;
            ctx.block().fptosi(DOUBLE, &value, I64)
        }
    };
    record_narrow(ctx, e, "lower_expr_native_isize", isize_lowered(value))
}

fn record_narrow(
    ctx: &mut FnCtx<'_>,
    e: &Expr,
    consumer: &'static str,
    lowered: LoweredValue,
) -> Result<LoweredValue> {
    ctx.record_lowered_value(
        native_expr_kind(e),
        None,
        consumer,
        &lowered,
        None,
        None,
        None,
        false,
        false,
        Vec::new(),
    );
    Ok(lowered)
}
